const fs = require('node:fs');
const path = require('node:path');

const {
  archiveCurrentReport,
  execute,
  sha256,
  writeCurrentReport,
} = require('./lib/harness');
const { ROOT } = require('./lib/manifest');

const REPORTS = ['live', 'implementation', 'symbol', 'git', 'schema-payload'];
const EXPECTED_REPORT_NAMES = [
  'deterministic-0',
  'deterministic-1',
  'deterministic-2',
  ...REPORTS,
];
const REQUIRED_ENGINES = [
  'blazingly-json',
  'mcport',
  'npm-weavatrix',
  'weavatrix-edit',
  'weavatrix-refactor-plan',
  'weavatrix-rust',
];

function observedValue(value) {
  if (value && typeof value === 'object' && Object.hasOwn(value, 'value')) return value.value;
  return value;
}

function range(values) {
  const present = values.filter(Number.isFinite).sort((left, right) => left - right);
  if (present.length === 0) return null;
  return { median: present[Math.floor(present.length / 2)], min: present[0], max: present.at(-1) };
}

function normalizedRow(row, sourceReport) {
  const tokens = row.tokens || {};
  return {
    sourceReport,
    suite: row.suite,
    task: row.task,
    arm: row.arm,
    trial: row.trial,
    qualityEarned: row.qualityEarned,
    qualityPossible: row.qualityPossible,
    taskSuccess: row.taskSuccess,
    sufficient: row.sufficient === undefined ? null : row.sufficient,
    falseConfidence: typeof row.falseConfidence === 'boolean'
      ? row.falseConfidence
      : row.sufficient === true && row.taskSuccess === false,
    failureClass: row.failureClass || null,
    tokens: {
      selected: row.selectedTokens ?? tokens.selected ?? null,
      delivered: row.deliveredTokens ?? tokens.delivered ?? null,
      schema: row.schemaTokens ?? null,
      modelPrefill: row.modelPrefillTokens ?? tokens.modelPrefill ?? null,
      modelGeneration: row.modelGenerationTokens ?? tokens.modelGeneration ?? null,
      wire: row.wireTokens ?? null,
    },
    calls: row.calls ?? null,
    latencyMs: row.latencyMs ?? null,
    payloadFormat: row.payloadFormat ?? null,
    artifact: row.artifact || row.contextArtifact || null,
    error: row.error || null,
  };
}

function aggregateReports(inputs) {
  const rows = [];
  const defects = [];
  for (const input of inputs) {
    const reportRows = input.report.rows || input.report.scoreboard || [];
    for (const row of reportRows) rows.push(normalizedRow(row, input.name));
    for (const defect of input.report.defects || []) {
      defects.push({ sourceReport: input.name, ...defect });
    }
  }
  const groups = new Map();
  for (const row of rows) {
    const key = `${row.suite}\u0000${row.arm}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(row);
  }
  const summary = [...groups.values()].map((samples) => ({
    suite: samples[0].suite,
    arm: samples[0].arm,
    samples: samples.length,
    qualityEarned: samples.reduce((sum, row) => sum + (row.qualityEarned || 0), 0),
    qualityPossible: samples.reduce((sum, row) => sum + (row.qualityPossible || 0), 0),
    taskSuccesses: samples.filter((row) => row.taskSuccess).length,
    falseConfidence: samples.filter((row) => row.falseConfidence).length,
    tokens: {
      selected: range(samples.map((row) => row.tokens.selected)),
      delivered: range(samples.map((row) => row.tokens.delivered)),
      schema: range(samples.map((row) => row.tokens.schema)),
      modelPrefill: range(samples.map((row) => row.tokens.modelPrefill)),
      modelGeneration: range(samples.map((row) => row.tokens.modelGeneration)),
      wire: range(samples.map((row) => row.tokens.wire)),
    },
    calls: range(samples.map((row) => row.calls)),
    latencyMs: range(samples.map((row) => row.latencyMs)),
    failures: Object.fromEntries([...new Set(samples.map((row) => row.failureClass).filter(Boolean))]
      .map((classification) => [classification, samples.filter((row) => row.failureClass === classification).length])),
  })).sort((left, right) => `${left.suite}/${left.arm}`.localeCompare(`${right.suite}/${right.arm}`));
  return {
    schemaVersion: 'cortex-scoreboard.v1', historical: false,
    generatedAt: new Date().toISOString(),
    reports: inputs.map((input) => ({ name: input.name, manifest: input.report.manifest || null, sha256: input.sha256 || null })),
    rows, summary, defects,
    hasUnclassified: rows.some((row) => !row.taskSuccess && (!row.failureClass || row.failureClass === 'UNCLASSIFIED')),
    superpowersActive: inputs.some((input) => input.report.manifest && input.report.manifest.superpowers && input.report.manifest.superpowers.active !== false),
  };
}

function validateCurrentInputs(inputs) {
  const byName = new Map(inputs.map((input) => [input.name, input]));
  const missing = EXPECTED_REPORT_NAMES.filter((name) => !byName.has(name));
  if (missing.length > 0) throw new Error(`current scoreboard is missing reports: ${missing.join(', ')}`);

  const rows = [];
  for (const name of EXPECTED_REPORT_NAMES) {
    const report = byName.get(name).report;
    if (report.schemaVersion !== 'cortex-benchmark.v2') {
      throw new Error(`${name} is not a cortex-benchmark.v2 report`);
    }
    if (report.historical !== false) {
      throw new Error(`${name} is not explicitly marked historical:false`);
    }
    if (!report.manifest || !report.manifest.target || !report.manifest.engines || !report.manifest.mcp) {
      throw new Error(`${name} is missing benchmark identity metadata`);
    }
    if (!report.manifest.model || typeof report.manifest.model.parameters !== 'object') {
      throw new Error(`${name} is missing explicit model identity or non-use metadata`);
    }
    const missingEngines = REQUIRED_ENGINES.filter((engine) => (
      !observedValue(report.manifest.engines[engine])
    ));
    if (missingEngines.length > 0) {
      throw new Error(`${name} is missing engine versions: ${missingEngines.join(', ')}`);
    }
    for (const repository of ['cortex', 'target']) {
      const commit = observedValue(report.manifest[repository] && report.manifest[repository].commit);
      if (!/^[0-9a-f]{40}$/i.test(commit || '')) {
        throw new Error(`${name} is missing the ${repository} repository commit`);
      }
    }
    if (!name.startsWith('deterministic-')) {
      if (observedValue(report.manifest.registryWeavatrix) !== observedValue(report.manifest.engines['npm-weavatrix'])) {
        throw new Error(`${name} did not verify its configured Weavatrix version against the npm registry`);
      }
      const serenaCommit = report.manifest.serena && report.manifest.serena.value
        && report.manifest.serena.value.commit;
      if (!/^[0-9a-f]{40}$/i.test(serenaCommit || '')) {
        throw new Error(`${name} is missing the current Serena commit`);
      }
    }
    rows.push(...(report.rows || report.scoreboard || []));
  }

  const trialsByGroup = new Map();
  for (const row of rows) {
    const key = `${row.suite}\u0000${row.task}\u0000${row.arm}`;
    if (!trialsByGroup.has(key)) trialsByGroup.set(key, new Set());
    trialsByGroup.get(key).add(row.trial);
  }
  const incomplete = [...trialsByGroup.entries()]
    .filter(([, trials]) => ![0, 1, 2].every((trial) => trials.has(trial)))
    .map(([key]) => key.replaceAll('\u0000', '/'));
  if (incomplete.length > 0) {
    throw new Error(`current scoreboard groups lack trials 0, 1, and 2: ${incomplete.join(', ')}`);
  }
  return inputs;
}

function loadReport(file) {
  const body = fs.readFileSync(file, 'utf8');
  return { report: JSON.parse(body), sha256: sha256(body) };
}

function loadCurrentReports(outputRoot) {
  const inputs = [];
  for (let trial = 0; trial < 3; trial += 1) {
    const file = path.join(outputRoot, `deterministic-${trial}.json`);
    if (fs.existsSync(file)) inputs.push({ name: `deterministic-${trial}`, ...loadReport(file) });
  }
  for (const name of REPORTS) {
    const file = path.join(outputRoot, `${name}.json`);
    if (fs.existsSync(file)) inputs.push({ name, ...loadReport(file) });
  }
  return inputs;
}

function runDeterministic(outputRoot) {
  const binary = path.join(ROOT, 'target', 'release', 'cortex-bench.exe');
  for (let trial = 0; trial < 3; trial += 1) {
    const output = path.join(outputRoot, `deterministic-${trial}.json`);
    archiveCurrentReport(output);
    const result = execute(binary, [
      '--repo', ROOT, '--set', 'probe', '--budget', '4000', '--trial', String(trial),
      '--stamp', 'p0-current-2026-08-11', '--out', output,
    ], { cwd: ROOT, timeoutMs: 1_800_000 });
    if (!result.ok) throw new Error(`deterministic trial ${trial} failed: ${result.stderr}`);
  }
}

async function runSuite(name, outputRoot, repository) {
  if (name === 'deterministic') return runDeterministic(outputRoot, repository);
  const moduleName = name === 'schema-payload' ? './schema-payload' : `./${name}`;
  return require(moduleName).run({ outputRoot, repository, trials: 3 });
}

async function main() {
  const suiteIndex = process.argv.indexOf('--suite');
  const suite = suiteIndex >= 0 ? process.argv[suiteIndex + 1] : 'all';
  const outputRoot = path.join(ROOT, '.cortex-loom', 'bench', 'p0');
  const repository = path.resolve(ROOT, '..', 'weavatrix-search');
  if (suite === 'all') {
    for (const name of ['deterministic', ...REPORTS]) await runSuite(name, outputRoot, repository);
  } else if (suite !== 'aggregate') {
    await runSuite(suite, outputRoot, repository);
  }
  const inputs = loadCurrentReports(outputRoot);
  validateCurrentInputs(inputs);
  const scoreboard = aggregateReports(inputs);
  writeCurrentReport(path.join(outputRoot, 'current-scoreboard.json'), scoreboard);
  if (scoreboard.hasUnclassified) throw new Error('scoreboard contains unclassified failures');
  if (scoreboard.superpowersActive) throw new Error('Superpowers was active instead of read-only benchmark input');
  process.stdout.write(`${JSON.stringify(scoreboard.summary, null, 2)}\n`);
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stack || error.message);
    process.exitCode = 1;
  });
}

module.exports = {
  EXPECTED_REPORT_NAMES,
  REQUIRED_ENGINES,
  aggregateReports,
  loadCurrentReports,
  runSuite,
  validateCurrentInputs,
};
