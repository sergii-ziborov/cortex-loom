const fs = require('node:fs');
const path = require('node:path');

const { IMPLEMENTATION_FIELDS, IMPLEMENTATION_TASK, hiddenImplementationTest } = require('./fixtures');
const {
  createWorktree,
  execute,
  removeWorktree,
  resetWorktree,
  rg,
  writeArtifact,
  writeCurrentReport,
} = require('./lib/harness');
const { detectEnvironment, ROOT } = require('./lib/manifest');
const { McpClient, estimateTokens } = require('./lib/mcp');
const { generate } = require('./lib/ollama');
const { alternatingOrders } = require('./lib/schedule');
const { summarizeRows } = require('./lib/scoreboard');

const ARMS = ['agent-native', 'weavatrix-mcp', 'cortex-mcp'];
const FILE = 'src/options/types.rs';
const MODEL = 'qwen3.5:9b';
const MODEL_PARAMETERS = {
  temperature: 0,
  num_ctx: 32768,
  num_predict: 600,
  seed: 7,
  thinking: false,
};

function sanitizeRust(answer) {
  return answer.replace(/^\s*```(?:rust)?\s*/i, '').replace(/\s*```\s*$/i, '').trim();
}

function gradeImplementationContext(context) {
  const present = IMPLEMENTATION_FIELDS.filter((field) => new RegExp(`\\b${field}\\b`).test(context));
  return { present, missing: IMPLEMENTATION_FIELDS.filter((field) => !present.includes(field)), complete: present.length === IMPLEMENTATION_FIELDS.length };
}

function gradeImplementationCode(code) {
  const checks = [
    { id: 'enabled-false', pattern: /\benabled\s*:\s*false\b/ },
    ...IMPLEMENTATION_FIELDS.filter((field) => field !== 'enabled').map((field) => ({
      id: `${field}-zero`, pattern: new RegExp(`\\b${field}\\s*:\\s*0\\b`),
    })),
    { id: 'constructor', pattern: /pub\s+fn\s+disabled\s*\(\s*\)\s*->\s*Self/ },
  ];
  const hit = checks.filter((check) => check.pattern.test(code)).map((check) => check.id);
  const missed = checks.filter((check) => !check.pattern.test(code)).map((check) => check.id);
  return { hit, missed, qualityEarned: hit.length, qualityPossible: checks.length, taskSuccess: missed.length === 0 };
}

function nativeContext(repository) {
  const started = process.hrtime.bigint();
  const hits = rg(repository, ['-n', '-C', '4', 'ArchiveOptions', '.']);
  const body = fs.readFileSync(path.join(repository, FILE), 'utf8');
  return {
    context: `${hits}\n===== ${FILE} =====\n${body}`,
    calls: 2,
    latencyMs: Number(process.hrtime.bigint() - started) / 1e6,
    schemaTokens: 0,
    payloadFormat: 'native-text',
    sufficient: true,
    completeness: { complete: true },
  };
}

function parsedSufficiency(result) {
  const structured = result.result && result.result.structuredContent;
  let value = structured;
  if (!value) {
    try { value = JSON.parse(result.countedText); } catch { value = null; }
  }
  return value && value.sufficiency ? value.sufficiency.sufficient : null;
}

async function mcpContext(arm, server, repository) {
  if (arm === 'weavatrix-mcp') {
    await server.call('open_repo', { path: '.', output_format: 'text' });
    const calls = [];
    for (const [name, args] of [
      ['search_code', { query: 'ArchiveOptions', output_format: 'text' }],
      ['inspect_symbol', { label: 'ArchiveOptions', output_format: 'text' }],
      ['read_source', { path: FILE, output_format: 'text' }],
    ]) calls.push(await server.call(name, args));
    return {
      context: calls.map((call) => `===== weavatrix ${call.name} =====\n${call.countedText}`).join('\n'),
      calls: calls.length,
      latencyMs: calls.reduce((sum, call) => sum + call.latencyMs, 0),
      schemaTokens: server.schemaTokens,
      payloadFormat: [...new Set(calls.map((call) => call.format))].join('+'),
      sufficient: calls.every((call) => call.completeness.complete),
      completeness: calls.map((call) => call.completeness),
    };
  }
  const result = await server.call('weavatrix_context_compile', {
    repository, task: IMPLEMENTATION_TASK, symbol: 'ArchiveOptions', targeted: true, maxTokens: 4000,
  });
  return {
    context: result.countedText,
    calls: 1,
    latencyMs: result.latencyMs,
    schemaTokens: server.schemaTokens,
    payloadFormat: result.format,
    sufficient: parsedSufficiency(result),
    completeness: result.completeness,
  };
}

async function ask(context) {
  const prompt = 'You are implementing a small change in an unfamiliar Rust codebase.\n' +
    'Use ONLY the evidence below for struct names, field names, and field types.\n\n' +
    `=== EVIDENCE ===\n${context}\n=== END EVIDENCE ===\n\nTASK: ${IMPLEMENTATION_TASK}\n`;
  return generate(MODEL, prompt, MODEL_PARAMETERS);
}

function applyAndTest(repository, code, targetDirectory) {
  const destination = path.join(repository, FILE);
  const original = fs.readFileSync(destination, 'utf8');
  fs.writeFileSync(destination, `${original}\n${code}\n${hiddenImplementationTest()}`, 'utf8');
  const result = execute('cargo', ['test', '--lib', 'harness_disabled_check'], {
    cwd: repository,
    env: { ...process.env, CARGO_TARGET_DIR: targetDirectory, CARGO_BUILD_JOBS: '2' },
    timeoutMs: 900_000,
  });
  const output = `${result.stdout}\n${result.stderr}`;
  return {
    compiled: !/error(?:\[|:)/.test(output),
    hiddenTestPassed: result.ok && /test result: ok\. 1 passed/.test(output),
    cargoMs: result.latencyMs,
    output,
  };
}

async function run(options = {}) {
  const sourceRepository = path.resolve(options.repository || path.join(ROOT, '..', 'weavatrix-search'));
  const outputRoot = path.resolve(options.outputRoot || path.join(ROOT, '.cortex-loom', 'bench', 'p0'));
  const trials = Number(options.trials || 3);
  const commit = execute('git', ['-C', sourceRepository, 'rev-parse', 'HEAD']).stdout.trim();
  if (!commit) throw new Error('cannot resolve implementation target commit');
  const worktreeRoot = path.join(sourceRepository, '.cortex-loom', 'bench', 'p0', 'worktrees');
  const worktrees = Object.fromEntries(ARMS.map((arm) => [arm, path.join(worktreeRoot, `implementation-${arm}`)]));
  for (const arm of ARMS) {
    createWorktree(sourceRepository, worktrees[arm], commit);
    resetWorktree(worktrees[arm], commit);
  }
  const manifest = await detectEnvironment({
    suiteVersion: 'implementation-hidden-v2', targetRepository: sourceRepository,
    model: MODEL, modelParameters: MODEL_PARAMETERS,
    mcp: { protocolVersion: '2025-06-18', transport: 'stdio', representationCounted: 'content-first' },
  });
  const servers = {
    'weavatrix-mcp': new McpClient({ command: 'npx.cmd', args: ['-y', 'weavatrix@1.7.0', 'mcp', '.', '--profile=code'], cwd: worktrees['weavatrix-mcp'], name: 'weavatrix', profile: 'code' }),
    'cortex-mcp': new McpClient({ command: path.join(ROOT, 'target', 'release', 'cortex-mcp.exe'), args: ['--profile', 'context'], cwd: worktrees['cortex-mcp'], name: 'cortex', profile: 'context' }),
  };
  const rows = [];
  const defects = [];
  const schedule = alternatingOrders(ARMS, trials);
  const targetDirectory = path.join(ROOT, '.cortex-loom', 'bench', 'p0', 'cargo-target');
  try {
    await Promise.all(Object.values(servers).map((server) => server.start()));
    for (const [trial, order] of schedule.entries()) {
      for (const arm of order) {
        process.stderr.write(`implementation trial ${trial + 1}/${trials}: ${arm}\n`);
        const repository = worktrees[arm];
        resetWorktree(repository, commit, { clean: false });
        const built = arm === 'agent-native' ? nativeContext(repository) : await mcpContext(arm, servers[arm], repository);
        const contextGrade = gradeImplementationContext(built.context);
        const contextArtifact = writeArtifact(outputRoot, `artifacts/implementation/${trial}-${arm}-context.txt`, built.context);
        const model = await ask(built.context);
        const code = sanitizeRust(model.answer);
        const codeGrade = gradeImplementationCode(code);
        const codeArtifact = writeArtifact(outputRoot, `artifacts/implementation/${trial}-${arm}-code.rs`, code);
        const verdict = applyAndTest(repository, code, targetDirectory);
        const cargoArtifact = writeArtifact(outputRoot, `artifacts/implementation/${trial}-${arm}-cargo.txt`, verdict.output);
        const taskSuccess = verdict.hiddenTestPassed;
        let failureClass = null;
        if (!taskSuccess) {
          if (!contextGrade.complete) failureClass = arm === 'weavatrix-mcp' ? 'WEAVATRIX_BUG' : arm === 'cortex-mcp' ? 'CORTEX_BUG' : 'HARNESS_BUG';
          else failureClass = 'MODEL_FAILURE';
        }
        const row = {
          suite: 'implementation-hidden', task: 'ArchiveOptions::disabled', arm, trial, order,
          qualityEarned: taskSuccess ? codeGrade.qualityPossible : codeGrade.qualityEarned,
          qualityPossible: codeGrade.qualityPossible, taskSuccess,
          sufficient: built.sufficient,
          falseConfidence: built.sufficient === true && !contextGrade.complete,
          failureClass, selectedTokens: estimateTokens(built.context), deliveredTokens: estimateTokens(built.context),
          schemaTokens: built.schemaTokens, modelPrefillTokens: model.promptTokens,
          modelGenerationTokens: model.generationTokens, calls: built.calls,
          latencyMs: built.latencyMs + model.totalMs + verdict.cargoMs,
          retrievalMs: built.latencyMs, modelTotalMs: model.totalMs, cargoMs: verdict.cargoMs,
          compiled: verdict.compiled, hiddenTestPassed: verdict.hiddenTestPassed,
          contextGrade, codeGrade, payloadFormat: built.payloadFormat,
          contextArtifact, codeArtifact, cargoArtifact,
        };
        rows.push(row);
        if (failureClass) defects.push({
          classification: failureClass, suite: 'implementation-hidden', task: 'ArchiveOptions::disabled', arm, trial,
          targetCommit: manifest.target.commit.value, missingContextFields: contextGrade.missing,
          hiddenOracle: hiddenImplementationTest(), actual: { contextArtifact, codeArtifact, cargoArtifact, completeness: built.completeness },
          fixDirection: failureClass === 'WEAVATRIX_BUG'
            ? 'Return the complete defining source for an unbounded read_source call, or mark it incomplete.'
            : failureClass === 'CORTEX_BUG'
              ? 'Require all ArchiveOptions fields as evidence obligations before sufficiency can be true.'
              : 'Reproduce from the preserved context, model output, and cargo transcript.',
        });
      }
    }
  } finally {
    for (const server of Object.values(servers)) server.close();
    for (const arm of ARMS) removeWorktree(sourceRepository, worktrees[arm]);
  }
  const report = {
    schemaVersion: 'cortex-benchmark.v2', historical: false, manifest, schedule, rows, defects,
    scoreboard: summarizeRows(rows),
  };
  writeCurrentReport(path.join(outputRoot, 'implementation.json'), report);
  return report;
}

if (require.main === module) {
  run({ trials: process.env.P0_TRIALS || 3 }).catch((error) => {
    console.error(error.stack || error.message);
    process.exitCode = 1;
  });
}

module.exports = { ARMS, gradeImplementationCode, gradeImplementationContext, run, sanitizeRust };
