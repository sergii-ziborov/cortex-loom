const path = require('node:path');

const { execute, writeArtifact, writeCurrentReport } = require('./lib/harness');
const { detectEnvironment, ROOT } = require('./lib/manifest');
const { McpClient, estimateTokens } = require('./lib/mcp');
const { alternatingOrders } = require('./lib/schedule');
const { summarizeRows } = require('./lib/scoreboard');

const ARMS = ['git-native', 'weavatrix-mcp'];
const TASKS = ['recent-history', 'source-structural-diff', 'six-month-analytics'];

function parseJson(text) {
  try { return JSON.parse(text); } catch { return null; }
}

function parseHistoryIds(text) {
  const value = parseJson(text);
  return new Set((value && value.commits || []).map((commit) => commit.id).filter(Boolean));
}

function graphFile(node) {
  const id = node && (node.id || (node.after || {}).id || (node.before || {}).id);
  return typeof id === 'string' && id.startsWith('file:') ? id.slice(5).replace(/\\/g, '/') : null;
}

function parseGraphDiffFiles(text) {
  const value = parseJson(text);
  const files = new Set();
  for (const group of ['added', 'changed', 'removed']) {
    for (const node of value && value.nodes && value.nodes[group] || []) {
      const file = graphFile(node);
      if (file) files.add(file);
    }
  }
  return files;
}

function scoreExactSet(actual, truth) {
  const hit = [...actual].filter((value) => truth.has(value));
  const missed = [...truth].filter((value) => !actual.has(value));
  const extra = [...actual].filter((value) => !truth.has(value));
  return {
    hit, missed, extra, qualityEarned: hit.length,
    qualityPossible: truth.size + extra.length, taskSuccess: missed.length === 0 && extra.length === 0,
  };
}

function scoreRecallSet(actual, truth) {
  const score = scoreExactSet(actual, truth);
  return { ...score, qualityPossible: truth.size, taskSuccess: score.missed.length === 0 };
}

function nativeHistory(repository) {
  const result = execute('git', ['log', '-25', '--format=%H%x09%s'], { cwd: repository });
  if (!result.ok) throw new Error(result.stderr);
  const commits = result.stdout.trim().split(/\r?\n/).filter(Boolean).map((line) => {
    const [id, ...summary] = line.split('\t');
    return { id, summary: summary.join('\t') };
  });
  return { text: JSON.stringify({ commits }), truth: new Set(commits.map((commit) => commit.id)), latencyMs: result.latencyMs };
}

function nativeDiff(repository) {
  const result = execute('git', ['diff', '--name-only', 'HEAD~10', 'HEAD', '--', 'src'], { cwd: repository });
  if (!result.ok) throw new Error(result.stderr);
  const files = new Set(result.stdout.trim().split(/\r?\n/).filter((file) => file.endsWith('.rs')).map((file) => file.replace(/\\/g, '/')));
  return { text: JSON.stringify({ base: 'HEAD~10', head: 'HEAD', files: [...files] }), truth: files, latencyMs: result.latencyMs };
}

function changedFilesByCommit(repository) {
  const result = execute('git', ['log', '--first-parent', '--diff-merges=first-parent', '--since=6 months ago', '--format=@@%H', '--name-only'], { cwd: repository });
  if (!result.ok) throw new Error(result.stderr);
  const commits = [];
  let current = null;
  for (const raw of result.stdout.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith('@@')) {
      current = { id: line.slice(2), files: new Set() };
      commits.push(current);
    } else if (line && current) current.files.add(line.replace(/\\/g, '/'));
  }
  // The oldest in-window commit is the comparison baseline, not a change
  // inside the window. Weavatrix reports it with changed_files=0; mirror that
  // explicit interval contract instead of counting its parent diff.
  if (commits.length > 0) commits.at(-1).files.clear();
  return { commits, latencyMs: result.latencyMs };
}

function nativeAnalytics(repository) {
  const observed = changedFilesByCommit(repository);
  const frequencies = new Map();
  const pairs = new Map();
  for (const commit of observed.commits) {
    const files = [...commit.files].sort();
    for (const file of files) frequencies.set(file, (frequencies.get(file) || 0) + 1);
    for (let left = 0; left < files.length; left += 1) {
      for (let right = left + 1; right < files.length; right += 1) {
        const key = `${files[left]}\u0000${files[right]}`;
        pairs.set(key, (pairs.get(key) || 0) + 1);
      }
    }
  }
  const summary = {
    commitsScanned: observed.commits.length,
    frequencies: Object.fromEntries(frequencies),
    pairs: Object.fromEntries(pairs),
  };
  return { text: JSON.stringify(summary), truth: summary, latencyMs: observed.latencyMs };
}

function analyticsScore(text, truth) {
  const value = parseJson(text);
  const analytics = value && value.analytics;
  const checks = [];
  checks.push({ id: 'commit-count', pass: analytics && analytics.commits_scanned === truth.commitsScanned });
  const hotspots = analytics && analytics.hotspots || [];
  checks.push({ id: 'hotspots-present', pass: truth.commitsScanned === 0 || hotspots.length > 0 });
  checks.push({ id: 'hotspot-frequencies', pass: hotspots.every((item) => truth.frequencies[item.path] === item.change_frequency) });
  const pairs = analytics && analytics.cochange_pairs || [];
  checks.push({ id: 'cochange-counts', pass: pairs.every((item) => {
    const left = item.left || item.path_a || item.first;
    const right = item.right || item.path_b || item.second;
    const key = [left, right].sort().join('\u0000');
    return Boolean(left && right) && truth.pairs[key] === (item.commits || item.count || item.change_frequency);
  }) });
  const hit = checks.filter((check) => check.pass).map((check) => check.id);
  return {
    checks, hit, missed: checks.filter((check) => !check.pass).map((check) => check.id),
    qualityEarned: hit.length, qualityPossible: checks.length, taskSuccess: hit.length === checks.length,
  };
}

function claimsComplete(text, fallback) {
  const value = parseJson(text);
  const states = [value && value.status, value && value.completeness, value && value.analytics && value.analytics.status].filter(Boolean);
  return states.some((state) => /^COMPLETE/.test(state)) || fallback;
}

async function run(options = {}) {
  const repository = path.resolve(options.repository || path.join(ROOT, '..', 'weavatrix-search'));
  const outputRoot = path.resolve(options.outputRoot || path.join(ROOT, '.cortex-loom', 'bench', 'p0'));
  const trials = Number(options.trials || 3);
  const manifest = await detectEnvironment({
    suiteVersion: 'git-v2', targetRepository: repository,
    mcp: { protocolVersion: '2025-06-18', transport: 'stdio', representationCounted: 'content-first' },
  });
  const server = new McpClient({ command: 'npx.cmd', args: ['-y', 'weavatrix@1.7.0', 'mcp', '.', '--profile=code'], cwd: repository, name: 'weavatrix', profile: 'code' });
  const oracles = {
    'recent-history': nativeHistory(repository),
    'source-structural-diff': nativeDiff(repository),
    'six-month-analytics': nativeAnalytics(repository),
  };
  const rows = [];
  const defects = [];
  const schedule = alternatingOrders(ARMS, trials);
  try {
    await server.start();
    await server.call('open_repo', { path: '.', output_format: 'text' });
    for (const [trial, order] of schedule.entries()) {
      for (const arm of order) {
        for (const task of TASKS) {
          process.stderr.write(`git trial ${trial + 1}/${trials}: ${arm} / ${task}\n`);
          const oracle = oracles[task];
          let result;
          if (arm === 'git-native') {
            result = { countedText: oracle.text, countedTokens: estimateTokens(oracle.text), latencyMs: oracle.latencyMs, format: 'native-text', completeness: { complete: true } };
          } else if (task === 'recent-history') {
            result = await server.call('git_history', { max_commits: 25, output_format: 'text' });
          } else if (task === 'source-structural-diff') {
            result = await server.call('graph_diff', { base_ref: 'HEAD~10', path: 'src', max_results: 10000, output_format: 'text' });
          } else {
            result = await server.call('git_history', { max_commits: 10000, first_parent: true, include_analytics: true, months: 6, top_n: 10, max_pairs: 10, min_pair_count: 2, output_format: 'text' });
          }
          let grade;
          if (task === 'recent-history') grade = scoreExactSet(arm === 'git-native' ? oracle.truth : parseHistoryIds(result.countedText), oracle.truth);
          else if (task === 'source-structural-diff') grade = scoreRecallSet(arm === 'git-native' ? oracle.truth : parseGraphDiffFiles(result.countedText), oracle.truth);
          else grade = arm === 'git-native'
            ? { qualityEarned: 4, qualityPossible: 4, taskSuccess: true, hit: ['commit-count', 'hotspots-present', 'hotspot-frequencies', 'cochange-counts'], missed: [] }
            : analyticsScore(result.countedText, oracle.truth);
          const artifact = writeArtifact(outputRoot, `artifacts/git/${trial}-${task}-${arm}.txt`, result.countedText);
          const sufficient = arm === 'git-native' ? true : claimsComplete(result.countedText, result.completeness.complete);
          const failureClass = grade.taskSuccess ? null : arm === 'weavatrix-mcp' ? 'WEAVATRIX_BUG' : 'HARNESS_BUG';
          rows.push({
            suite: 'git', task, arm, trial, order,
            qualityEarned: grade.qualityEarned, qualityPossible: grade.qualityPossible,
            taskSuccess: grade.taskSuccess, sufficient, falseConfidence: sufficient && !grade.taskSuccess,
            failureClass, selectedTokens: result.countedTokens, deliveredTokens: result.countedTokens,
            schemaTokens: arm === 'weavatrix-mcp' ? server.schemaTokens : 0,
            calls: 1, latencyMs: result.latencyMs, payloadFormat: result.format,
            grade, artifact,
          });
          if (failureClass === 'WEAVATRIX_BUG') defects.push({
            classification: failureClass, suite: 'git', task, trial,
            targetCommit: manifest.target.commit.value, engineVersion: manifest.engines['npm-weavatrix'],
            expected: oracle.truth instanceof Set ? [...oracle.truth] : oracle.truth,
            actual: { artifact, grade, completeness: result.completeness },
            reproduction: task === 'recent-history'
              ? { tool: 'git_history', arguments: { max_commits: 25, output_format: 'text' } }
              : task === 'source-structural-diff'
                ? { tool: 'graph_diff', arguments: { base_ref: 'HEAD~10', path: 'src', max_results: 10000, output_format: 'text' } }
                : { tool: 'git_history', arguments: { max_commits: 10000, first_parent: true, include_analytics: true, months: 6, top_n: 10, max_pairs: 10, min_pair_count: 2, output_format: 'text' } },
            fixDirection: 'Make COMPLETE Git output agree with the same immutable commit range, or downgrade completeness and state the unsupported evidence precisely.',
          });
        }
      }
    }
  } finally {
    server.close();
  }
  const report = {
    schemaVersion: 'cortex-benchmark.v2', historical: false, manifest, schedule, rows, defects,
    scoreboard: summarizeRows(rows),
  };
  writeCurrentReport(path.join(outputRoot, 'git.json'), report);
  return report;
}

if (require.main === module) {
  run({ trials: process.env.P0_TRIALS || 3 }).catch((error) => {
    console.error(error.stack || error.message);
    process.exitCode = 1;
  });
}

module.exports = { ARMS, analyticsScore, parseGraphDiffFiles, parseHistoryIds, run, scoreExactSet };
