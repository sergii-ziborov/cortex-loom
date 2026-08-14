const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { LIVE_TASKS, gradeAnswer, gradeContext } = require('./fixtures');
const {
  sha256,
  writeArtifact,
  writeCurrentReport,
  writeReport,
  rg,
  rustFiles,
} = require('./lib/harness');
const { detectEnvironment, ROOT } = require('./lib/manifest');
const { McpClient, estimateTokens } = require('./lib/mcp');
const { generate } = require('./lib/ollama');
const { alternatingOrders } = require('./lib/schedule');
const { summarizeRows } = require('./lib/scoreboard');

const ARMS = [
  'no-context',
  'naive',
  'agent-native',
  'agent-native+superpowers',
  'serena-mcp',
  'weavatrix-mcp',
  'cortex-mcp',
];
const MODEL = 'qwen3.5:9b';
const MODEL_PARAMETERS = {
  temperature: 0,
  num_ctx: 32768,
  num_predict: 400,
  seed: 7,
  thinking: false,
};

function liveHarnessHash() {
  const files = [
    __filename,
    path.join(__dirname, 'fixtures.js'),
    path.join(__dirname, 'lib', 'harness.js'),
    path.join(__dirname, 'lib', 'manifest.js'),
    path.join(__dirname, 'lib', 'mcp.js'),
    path.join(__dirname, 'lib', 'ollama.js'),
    path.join(__dirname, 'lib', 'schedule.js'),
    path.join(__dirname, 'lib', 'scoreboard.js'),
  ];
  return sha256(files.map((file) => (
    `${path.relative(ROOT, file).replace(/\\/g, '/')}\n${fs.readFileSync(file, 'utf8')}`
  )).join('\n'));
}

function superpowersRoot() {
  const base = path.join(os.homedir(), '.codex', 'plugins', 'cache', 'openai-curated-remote', 'superpowers');
  const versions = fs.readdirSync(base).filter((entry) => /^\d+\.\d+\.\d+$/.test(entry)).sort();
  if (versions.length === 0) throw new Error(`no Superpowers benchmark input under ${base}`);
  const version = versions.at(-1);
  return { version, root: path.join(base, version, 'skills') };
}

function superpowersOverlay() {
  const source = superpowersRoot();
  let discovery = '';
  for (const entry of fs.readdirSync(source.root, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const skill = path.join(source.root, entry.name, 'SKILL.md');
    if (!fs.existsSync(skill)) continue;
    const raw = fs.readFileSync(skill, 'utf8');
    const header = raw.match(/^---\r?\n([\s\S]*?)\r?\n---/);
    if (header) discovery += `${header[1]}\n`;
  }
  const bootstrap = fs.readFileSync(path.join(source.root, 'using-superpowers', 'SKILL.md'), 'utf8');
  const debugging = fs.readFileSync(path.join(source.root, 'systematic-debugging', 'SKILL.md'), 'utf8');
  return {
    ...source,
    active: false,
    mode: 'read-only benchmark input',
    discoveryTokens: estimateTokens(discovery),
    text: `${bootstrap}\n${debugging}`,
  };
}

function nativeContext(repository, task) {
  const started = process.hrtime.bigint();
  const hits = rg(repository, ['-n', '-C', '3', task.searchPattern, '.']);
  const files = [...new Set((hits.match(/^[^\s:]+\.rs/gm) || []).map((file) => file.replace(/\\/g, '/')))].slice(0, 3);
  let windows = '';
  for (const file of files) {
    const absolute = path.join(repository, file);
    if (!fs.existsSync(absolute)) continue;
    windows += `\n===== ${file} (first 160 lines) =====\n`;
    windows += fs.readFileSync(absolute, 'utf8').split(/\r?\n/).slice(0, 160).join('\n');
  }
  return {
    context: hits + windows,
    calls: 1 + files.length,
    latencyMs: Number(process.hrtime.bigint() - started) / 1e6,
    schemaTokens: 0,
    sufficient: null,
    payloadFormat: 'native-text',
  };
}

function naiveContext(repository, task) {
  const started = process.hrtime.bigint();
  let context = '';
  for (const file of rustFiles(repository, task.naiveGlobs)) {
    context += `\n===== ${file} =====\n${fs.readFileSync(path.join(repository, file), 'utf8')}`;
  }
  return {
    context,
    calls: 0,
    latencyMs: Number(process.hrtime.bigint() - started) / 1e6,
    schemaTokens: 0,
    sufficient: null,
    payloadFormat: 'native-text',
  };
}

async function mcpContext(server, calls) {
  let context = '';
  let latencyMs = 0;
  let payloadFormat = null;
  let sufficient = null;
  const callRecords = [];
  for (const [tool, args] of calls) {
    const result = await server.call(tool, args);
    context += `\n===== ${server.definition.name} ${tool} =====\n${result.countedText}`;
    latencyMs += result.latencyMs;
    payloadFormat = result.format;
    callRecords.push(result);
    const parsed = (() => { try { return JSON.parse(result.countedText); } catch { return null; } })();
    if (parsed && parsed.sufficiency) sufficient = parsed.sufficiency.sufficient;
  }
  if (sufficient === null && server.definition.name === 'weavatrix') {
    sufficient = callRecords.every((call) => call.completeness.complete);
  }
  return {
    context,
    calls: calls.length,
    latencyMs,
    schemaTokens: server.schemaTokens,
    sufficient,
    payloadFormat,
    callRecords,
  };
}

async function buildContext(arm, repository, task, servers, superpowers) {
  if (arm === 'no-context') return { context: '', calls: 0, latencyMs: 0, schemaTokens: 0, sufficient: false, payloadFormat: 'none' };
  if (arm === 'naive') return naiveContext(repository, task);
  if (arm === 'agent-native') return nativeContext(repository, task);
  if (arm === 'agent-native+superpowers') {
    const built = nativeContext(repository, task);
    return { ...built, context: `${superpowers.text}\n${built.context}`, schemaTokens: superpowers.discoveryTokens };
  }
  if (arm === 'weavatrix-mcp') {
    return mcpContext(servers.weavatrix, [
      ['search_code', { query: task.searchPattern, is_regex: true, output_format: 'text' }],
      ['inspect_symbol', { label: task.symbol, output_format: 'text' }],
      ['get_dependents', { label: task.symbol, output_format: 'text' }],
    ]);
  }
  if (arm === 'cortex-mcp') {
    return mcpContext(servers.cortex, [[
      'weavatrix_context_compile',
      { repository, task: task.question, symbol: task.symbol, targeted: true, maxTokens: 4000 },
    ]]);
  }
  return mcpContext(servers.serena, [
    ['find_symbol', { name_path_pattern: task.symbol, relative_path: task.serenaFile, include_body: true }],
    ['find_referencing_symbols', { name_path: task.symbol, relative_path: task.serenaFile }],
  ]);
}

function failureClass(arm, answerGrade, contextGrade, sufficient) {
  const falseConfidence = sufficient === true && !contextGrade.taskSuccess;
  if (answerGrade.taskSuccess && !falseConfidence) return null;
  if (contextGrade.taskSuccess) return 'MODEL_FAILURE';
  if (arm === 'weavatrix-mcp') return 'WEAVATRIX_BUG';
  if (arm === 'cortex-mcp') return 'CORTEX_BUG';
  if (arm === 'serena-mcp') return 'COMPETITOR_GAP';
  if (arm === 'agent-native+superpowers') return 'METHODOLOGY_GAP';
  return 'CONTROL_GAP';
}

async function ask(context, question) {
  const prompt = 'You are answering a question about an unfamiliar Rust codebase.\n' +
    'Use ONLY the evidence below. If the evidence does not contain the answer, say so explicitly.\n' +
    'Be specific: name exact identifiers. Answer in at most 200 words.\n\n' +
    (context ? `=== EVIDENCE ===\n${context}\n=== END EVIDENCE ===\n\n` : '=== NO EVIDENCE PROVIDED ===\n\n') +
    `QUESTION: ${question}\n`;
  return generate(MODEL, prompt, MODEL_PARAMETERS);
}

async function run(options = {}) {
  const repository = path.resolve(options.repository || path.join(ROOT, '..', 'weavatrix-search'));
  const outputRoot = path.resolve(options.outputRoot || path.join(ROOT, '.cortex-loom', 'bench', 'p0'));
  const trials = Number(options.trials || 3);
  const resume = options.resume === true;
  const maxRows = options.maxRows ? Number(options.maxRows) : null;
  const manifest = await detectEnvironment({
    suiteVersion: 'live-v2',
    targetRepository: repository,
    model: MODEL,
    modelParameters: MODEL_PARAMETERS,
    mcp: { protocolVersion: '2025-06-18', transport: 'stdio', representationCounted: 'content-first' },
  });
  const serenaCommit = manifest.serena.value && manifest.serena.value.commit;
  if (!serenaCommit) throw new Error(`cannot resolve current Serena: ${manifest.serena.reason}`);
  const cortexBinary = path.join(ROOT, 'target', 'release', 'cortex-mcp.exe');
  const servers = {
    weavatrix: new McpClient({ command: 'npx.cmd', args: ['-y', 'weavatrix@1.8.0', 'mcp', '.', '--profile=code'], cwd: repository, name: 'weavatrix', profile: 'code' }),
    cortex: new McpClient({ command: cortexBinary, args: ['--profile', 'context'], cwd: repository, name: 'cortex', profile: 'context' }),
    serena: new McpClient({
      command: 'uvx',
      args: ['--from', `git+https://github.com/oraios/serena@${serenaCommit}`, 'serena', 'start-mcp-server', '--project', repository, '--context', 'ide-assistant', '--enable-web-dashboard', 'False', '--enable-gui-log-window', 'False'],
      cwd: repository,
      name: 'serena',
      profile: 'ide-assistant',
    }),
  };
  const checkpointPath = path.join(outputRoot, 'live.checkpoint.json');
  const superpowers = superpowersOverlay();
  const legacyConfiguration = {
    MODEL,
    MODEL_PARAMETERS,
    ARMS,
    tasks: LIVE_TASKS.map((task) => task.id),
    target: manifest.target.commit.value,
  };
  const legacyConfigurationHash = sha256(JSON.stringify(legacyConfiguration));
  const evidenceOracleLegacyHash = 'c2a5e8efca21505c003709ddae729371f5064da2062673133f5614a8c86aa055';
  const configurationHash = sha256(JSON.stringify({
    ...legacyConfiguration,
    trials,
    targetPath: manifest.target.path,
    engineIdentity: {
      cortexCommit: manifest.cortex.commit.value,
      cortexMcpSha256: sha256(fs.readFileSync(cortexBinary)),
      versions: manifest.engines,
      registryWeavatrix: manifest.registryWeavatrix.value,
      serenaCommit,
      modelDigest: manifest.model.digest,
      modelRuntimeVersion: manifest.model.runtimeVersion,
    },
    harnessHash: liveHarnessHash(),
    superpowers: {
      version: superpowers.version,
      textHash: sha256(superpowers.text),
      discoveryTokens: superpowers.discoveryTokens,
    },
  }));
  const checkpoint = resume && fs.existsSync(checkpointPath)
    ? JSON.parse(fs.readFileSync(checkpointPath, 'utf8'))
    : { schemaVersion: 'cortex-benchmark-checkpoint.v1', configurationHash, rows: [], defects: [], segments: [] };
  const migrationReasons = new Map([
    [legacyConfigurationHash, 'added trials, target path, harness hash, executable engine identities, and exact Superpowers overlay to resume identity'],
    [evidenceOracleLegacyHash, 'corrected false confidence to use the evidence oracle and excluded echoed task text from context grading'],
  ]);
  if (checkpoint.configurationHash !== configurationHash && !migrationReasons.has(checkpoint.configurationHash)) {
    throw new Error('live checkpoint differs from the current trials, model, arms, tasks, target, harness, or Superpowers overlay');
  }
  if (checkpoint.configurationHash !== configurationHash) {
    checkpoint.configurationMigrations = [...(checkpoint.configurationMigrations || []), {
      from: checkpoint.configurationHash,
      to: configurationHash,
      reason: migrationReasons.get(checkpoint.configurationHash),
    }];
    checkpoint.configurationHash = configurationHash;
  }
  const rows = checkpoint.rows || [];
  const defects = checkpoint.defects || [];
  const tasksById = new Map(LIVE_TASKS.map((task) => [task.id, task]));
  const defectsByRow = new Map(defects.map((defect) => [
    `${defect.trial}\u0000${defect.task}\u0000${defect.arm}`,
    defect,
  ]));
  for (const row of rows) {
    const task = tasksById.get(row.task);
    const artifactPath = row.contextArtifact && row.contextArtifact.path;
    if (!task || !artifactPath || !fs.existsSync(artifactPath)) continue;
    const contextGrade = gradeContext(task, fs.readFileSync(artifactPath, 'utf8'));
    row.contextQualityEarned = contextGrade.qualityEarned;
    row.contextMissed = contextGrade.missed;
    row.contextTaskSuccess = contextGrade.taskSuccess;
    row.falseConfidence = row.sufficient === true && !contextGrade.taskSuccess;
    row.failureClass = failureClass(row.arm, { taskSuccess: row.taskSuccess }, contextGrade, row.sufficient);
    const defect = defectsByRow.get(`${row.trial}\u0000${row.task}\u0000${row.arm}`);
    if (defect && row.failureClass) {
      defect.classification = row.failureClass;
      defect.expectedFacts = contextGrade.missed;
      defect.contextMissingFacts = contextGrade.missed;
      defect.suspectedLayer = row.failureClass === 'WEAVATRIX_BUG'
        ? 'raw MCP operation coverage'
        : row.failureClass === 'CORTEX_BUG'
          ? 'planner/gather/compiler/sufficiency'
          : 'model or control arm';
    }
  }
  checkpoint.segments = [...(checkpoint.segments || []), manifest];
  const completed = new Set(rows.map((row) => `${row.trial}\u0000${row.task}\u0000${row.arm}`));
  let newRows = 0;
  let stoppedEarly = false;
  function saveCheckpoint() {
    writeReport(checkpointPath, { ...checkpoint, rows, defects, completedRows: rows.length });
  }
  try {
    for (const server of Object.values(servers)) await server.start();
    await servers.weavatrix.call('open_repo', { path: '.', output_format: 'text' });
    trialsLoop: for (const [trial, order] of alternatingOrders(ARMS, trials).entries()) {
      for (const arm of order) {
        for (const task of LIVE_TASKS) {
          const key = `${trial}\u0000${task.id}\u0000${arm}`;
          if (completed.has(key)) continue;
          process.stderr.write(`live trial ${trial + 1}/${trials}: ${arm} / ${task.id}\n`);
          try {
            const built = await buildContext(arm, repository, task, servers, superpowers);
            const contextGrade = gradeContext(task, built.context);
            const contextArtifact = writeArtifact(outputRoot, `artifacts/live/${trial}-${task.id}-${arm}-context.txt`, built.context);
            const model = await ask(built.context, task.question);
            const answerGrade = gradeAnswer(task, model.answer);
            const answerArtifact = writeArtifact(outputRoot, `artifacts/live/${trial}-${task.id}-${arm}-answer.txt`, model.answer);
            const classification = failureClass(arm, answerGrade, contextGrade, built.sufficient);
            const row = {
              suite: 'live', task: task.id, arm, trial,
              order, warmState: trial === 0 ? 'cold' : 'warm',
              qualityEarned: answerGrade.qualityEarned,
              qualityPossible: answerGrade.qualityPossible,
              hit: answerGrade.hit, missed: answerGrade.missed, bonus: answerGrade.bonus,
              contextQualityEarned: contextGrade.qualityEarned,
              contextMissed: contextGrade.missed,
              contextTaskSuccess: contextGrade.taskSuccess,
              sufficient: built.sufficient,
              taskSuccess: answerGrade.taskSuccess,
              falseConfidence: built.sufficient === true && !contextGrade.taskSuccess,
              failureClass: classification,
              selectedTokens: estimateTokens(built.context),
              deliveredTokens: estimateTokens(built.context),
              schemaTokens: built.schemaTokens,
              modelPrefillTokens: model.promptTokens,
              modelGenerationTokens: model.generationTokens,
              calls: built.calls,
              latencyMs: built.latencyMs + model.totalMs,
              retrievalMs: built.latencyMs,
              modelTotalMs: model.totalMs,
              payloadFormat: built.payloadFormat,
              contextArtifact, answerArtifact,
            };
            rows.push(row);
            completed.add(key);
            if (classification) {
              const sourceEvidence = rg(repository, ['-n', task.searchPattern, 'src']).split(/\r?\n/).filter(Boolean).slice(0, 20);
              defects.push({
                classification, suite: 'live', task: task.id, arm, trial,
                targetCommit: manifest.target.commit.value,
                expectedFacts: contextGrade.missed,
                contextMissingFacts: contextGrade.missed,
                sourceEvidence,
                actualArtifact: contextArtifact,
                completeness: (built.callRecords || []).map((call) => call.completeness),
                suspectedLayer: classification === 'WEAVATRIX_BUG' ? 'raw MCP operation coverage' : classification === 'CORTEX_BUG' ? 'planner/gather/compiler/sufficiency' : 'model or control arm',
                fixDirection: classification === 'WEAVATRIX_BUG' ? 'Add an engine/tool regression that returns the cited source fact or marks the response incomplete.' : classification === 'CORTEX_BUG' ? 'Add the missing fact as an evidence obligation and fail sufficiency until it survives delivery.' : 'Use the preserved prompt, context, and answer artifacts to reproduce the failure.',
              });
            }
          } catch (error) {
            rows.push({ suite: 'live', task: task.id, arm, trial, order, taskSuccess: false, falseConfidence: false, failureClass: 'HARNESS_BUG', error: error.message });
            defects.push({ classification: 'HARNESS_BUG', suite: 'live', task: task.id, arm, trial, error: error.message });
            completed.add(key);
          }
          newRows += 1;
          saveCheckpoint();
          if (maxRows && newRows >= maxRows) {
            stoppedEarly = true;
            break trialsLoop;
          }
        }
      }
    }
  } finally {
    for (const server of Object.values(servers)) server.close();
  }
  if (stoppedEarly) return { partial: true, completedRows: rows.length, checkpointPath };
  const expectedRows = trials * ARMS.length * LIVE_TASKS.length;
  if (rows.length !== expectedRows) {
    throw new Error(`live checkpoint has ${rows.length} rows, expected ${expectedRows}`);
  }
  const report = {
    schemaVersion: 'cortex-benchmark.v2',
    historical: false,
    configurationHash,
    manifest: {
      ...manifest,
      executionSegments: checkpoint.segments,
      superpowers: { version: superpowers.version, active: false, mode: superpowers.mode },
    },
    schedule: alternatingOrders(ARMS, trials),
    rows,
    defects,
    scoreboard: summarizeRows(rows),
  };
  writeCurrentReport(path.join(outputRoot, 'live.json'), report);
  return report;
}

if (require.main === module) {
  run({
    trials: process.env.P0_TRIALS || 3,
    resume: process.env.P0_RESUME === '1',
    maxRows: process.env.P0_MAX_ROWS,
  }).then(
    () => process.exit(0),
    (error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    },
  );
}

module.exports = { ARMS, liveHarnessHash, run };
