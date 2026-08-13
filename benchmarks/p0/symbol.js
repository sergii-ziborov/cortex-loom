const fs = require('node:fs');
const path = require('node:path');

const { SYMBOLS } = require('./fixtures');
const { writeArtifact, writeCurrentReport, rg } = require('./lib/harness');
const { detectEnvironment, ROOT } = require('./lib/manifest');
const { McpClient } = require('./lib/mcp');
const { alternatingOrders } = require('./lib/schedule');
const { summarizeRows } = require('./lib/scoreboard');

const ARMS = ['weavatrix-mcp', 'serena-mcp'];

function canonicalFile(file) {
  return file.replace(/\\/g, '/');
}

function enclosingFunction(lines, line) {
  for (let index = line - 1; index >= 0; index -= 1) {
    const match = lines[index].match(/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:const\s+)?fn\s+(\w+)/);
    if (match) return match[1];
    if (/^(?:pub\s+)?(?:struct|enum|trait|mod)\s/.test(lines[index])) return '<item>';
  }
  return '<module>';
}

function nextFunction(lines, line) {
  for (let index = line; index < lines.length; index += 1) {
    const match = lines[index].match(/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:const\s+)?fn\s+(\w+)/);
    if (match) return match[1];
    if (index > line && /^\s*}\s*$/.test(lines[index])) break;
  }
  return '<item>';
}

function escaped(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function groundTruthFromHits(hits, readLines, symbolName) {
  const rows = [];
  for (const hit of hits) {
    const match = hit.match(/^(.*?):(\d+):(.*)$/);
    if (!match) continue;
    const file = canonicalFile(match[1]);
    const line = Number(match[2]);
    const text = match[3];
    let classification = 'reference';
    if (new RegExp(`(?:fn|struct|enum|trait)\\s+${escaped(symbolName)}\\b`).test(text)) classification = 'definition';
    else if (/^\s*(?:pub\s+)?use\b/.test(text)) classification = 'import';
    else if (/^\s*\/\//.test(text)) classification = 'comment';
    const lines = readLines(file);
    const enclosing = classification === 'reference'
      ? /^\s*impl\b/.test(text) ? nextFunction(lines, line) : enclosingFunction(lines, line)
      : '';
    rows.push({ file, line, classification, enclosing, text: text.trim() });
  }
  return {
    rows,
    truth: new Set(rows
      .filter((row) => row.classification === 'reference' && !['<module>', '<item>'].includes(row.enclosing))
      .map((row) => `${row.file}::${row.enclosing}`)),
  };
}

function groundTruth(repository, symbol) {
  const body = rg(repository, ['-n', `\\b${symbol.name}\\b`, 'src']);
  const hits = body.trim().split(/\r?\n/).filter(Boolean);
  return groundTruthFromHits(
    hits,
    (file) => fs.readFileSync(path.join(repository, file), 'utf8').split(/\r?\n/),
    symbol.name,
  );
}

function weavatrixSet(text) {
  const found = new Set();
  let value;
  try { value = JSON.parse(text); } catch { return found; }
  for (const dependent of value.dependents || []) {
    const node = dependent.node || {};
    const file = canonicalFile(((node.span || {}).file) || '');
    if (dependent.distance === 1 && ['function', 'method'].includes(node.kind) && file) {
      found.add(`${file}::${node.label}`);
    }
  }
  return found;
}

function serenaSet(text) {
  const found = new Set();
  let value;
  try { value = JSON.parse(text); } catch { return found; }
  if (!value || typeof value !== 'object') return found;
  for (const [file, kinds] of Object.entries(value)) {
    for (const [kind, entries] of Object.entries(kinds || {})) {
      if (!['Function', 'Method'].includes(kind)) continue;
      for (const entry of entries || []) {
        const name = (entry.name_path || '').split('/').at(-1);
        if (name) found.add(`${canonicalFile(file)}::${name}`);
      }
    }
  }
  return found;
}

function scoreSet(actual, truth) {
  const hit = [...actual].filter((entry) => truth.has(entry));
  const missed = [...truth].filter((entry) => !actual.has(entry));
  const extra = [...actual].filter((entry) => !truth.has(entry));
  return {
    returned: actual.size,
    truth: truth.size,
    hit,
    missed,
    extra,
    recall: truth.size ? hit.length / truth.size : 1,
    precision: actual.size ? hit.length / actual.size : 1,
    qualityEarned: hit.length,
    qualityPossible: truth.size + extra.length,
    taskSuccess: missed.length === 0 && extra.length === 0,
  };
}

async function run(options = {}) {
  const repository = path.resolve(options.repository || path.join(ROOT, '..', 'weavatrix-search'));
  const outputRoot = path.resolve(options.outputRoot || path.join(ROOT, '.cortex-loom', 'bench', 'p0'));
  const trials = Number(options.trials || 3);
  const manifest = await detectEnvironment({
    suiteVersion: 'symbol-truth-v2',
    targetRepository: repository,
    mcp: { protocolVersion: '2025-06-18', transport: 'stdio', representationCounted: 'content-first' },
  });
  const serenaCommit = manifest.serena.value && manifest.serena.value.commit;
  if (!serenaCommit) throw new Error(`cannot resolve current Serena: ${manifest.serena.reason}`);
  const servers = {
    'weavatrix-mcp': new McpClient({ command: 'npx.cmd', args: ['-y', 'weavatrix@1.7.0', 'mcp', '.', '--profile=code'], cwd: repository, name: 'weavatrix', profile: 'code' }),
    'serena-mcp': new McpClient({
      command: 'uvx',
      args: ['--from', `git+https://github.com/oraios/serena@${serenaCommit}`, 'serena', 'start-mcp-server', '--project', repository, '--context', 'ide-assistant', '--enable-web-dashboard', 'False', '--enable-gui-log-window', 'False'],
      cwd: repository,
      name: 'serena',
      profile: 'ide-assistant',
    }),
  };
  const rows = [];
  const defects = [];
  const truthSets = new Map(SYMBOLS.map((symbol) => [symbol.name, groundTruth(repository, symbol)]));
  try {
    await Promise.all(Object.values(servers).map((server) => server.start()));
    await servers['weavatrix-mcp'].call('open_repo', { path: '.', output_format: 'text' });
    const schedule = alternatingOrders(ARMS, trials);
    for (const [trial, order] of schedule.entries()) {
      for (const arm of order) {
        for (const symbol of SYMBOLS) {
          process.stderr.write(`symbol trial ${trial + 1}/${trials}: ${arm} / ${symbol.name}\n`);
          const result = arm === 'weavatrix-mcp'
            ? await servers[arm].call('get_dependents', { label: symbol.name, output_format: 'text' })
            : await servers[arm].call('find_referencing_symbols', { name_path: symbol.name, relative_path: symbol.definitionFile });
          const actual = arm === 'weavatrix-mcp' ? weavatrixSet(result.countedText) : serenaSet(result.countedText);
          const truth = truthSets.get(symbol.name);
          const score = scoreSet(actual, truth.truth);
          const artifact = writeArtifact(outputRoot, `artifacts/symbol/${trial}-${symbol.name}-${arm}.txt`, result.countedText);
          const taskSuccess = score.taskSuccess;
          const failureClass = taskSuccess ? null : arm === 'weavatrix-mcp' ? 'WEAVATRIX_BUG' : 'COMPETITOR_GAP';
          rows.push({
            suite: 'symbol-truth', task: symbol.name, arm, trial, order,
            qualityEarned: score.qualityEarned, qualityPossible: score.qualityPossible,
            taskSuccess, sufficient: arm === 'weavatrix-mcp' ? result.completeness.complete : null,
            falseConfidence: arm === 'weavatrix-mcp' && result.completeness.complete && !taskSuccess,
            failureClass, selectedTokens: result.countedTokens, deliveredTokens: result.countedTokens,
            schemaTokens: servers[arm].schemaTokens, calls: 1, latencyMs: result.latencyMs,
            payloadFormat: result.format, score, artifact,
          });
          if (failureClass === 'WEAVATRIX_BUG') {
            defects.push({
              classification: failureClass,
              suite: 'symbol-truth', task: symbol.name, trial,
              targetCommit: manifest.target.commit.value,
              engineVersion: manifest.engines['npm-weavatrix'],
              reproduction: { tool: 'get_dependents', arguments: { label: symbol.name, output_format: 'text' } },
              expected: { references: [...truth.truth], sourceHits: truth.rows },
              actual: { references: [...actual], artifact, completeness: result.completeness },
              fixDirection: 'Make direct reference extraction include every mechanically verified enclosing function, or mark the result incomplete with a machine-readable reason.',
            });
          }
        }
      }
    }
  } finally {
    for (const server of Object.values(servers)) server.close();
  }
  const report = {
    schemaVersion: 'cortex-benchmark.v2', historical: false, manifest,
    schedule: alternatingOrders(ARMS, trials), rows, defects, scoreboard: summarizeRows(rows),
  };
  writeCurrentReport(path.join(outputRoot, 'symbol.json'), report);
  return report;
}

if (require.main === module) {
  run({ trials: process.env.P0_TRIALS || 3 }).catch((error) => {
    console.error(error.stack || error.message);
    process.exitCode = 1;
  });
}

module.exports = { ARMS, groundTruthFromHits, run, scoreSet, serenaSet, weavatrixSet };
