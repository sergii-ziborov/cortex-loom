const path = require('node:path');

const { writeArtifact, writeCurrentReport } = require('./lib/harness');
const { detectEnvironment, ROOT } = require('./lib/manifest');
const { McpClient } = require('./lib/mcp');
const { alternatingOrders } = require('./lib/schedule');
const { summarizeRows } = require('./lib/scoreboard');

const ARMS = ['weavatrix-text', 'weavatrix-json', 'weavatrix-structured', 'cortex-default', 'serena-default'];

function expectedPayloadFormat(arm) {
  if (['weavatrix-text', 'cortex-default', 'serena-default'].includes(arm)) return 'text';
  if (arm === 'weavatrix-json') return 'mirrored';
  if (arm === 'weavatrix-structured') return 'structured';
  return null;
}

function payloadGrade(client, result, arm) {
  const expected = expectedPayloadFormat(arm);
  const checks = [
    { id: 'initialized', pass: Boolean(client.initializeResult && client.initializeResult.serverInfo) },
    { id: 'tools-listed', pass: Array.isArray(client.tools) && client.tools.length > 0 && client.schemaTokens > 0 },
    { id: 'call-succeeded', pass: !result.isError && result.countedText.length > 0 },
    { id: 'one-representation-counted', pass: ['content', 'structuredContent'].includes(result.countedRepresentation) && result.countedTokens <= result.wireTokens },
    { id: 'format-contract', pass: expected === null || result.format === expected },
  ];
  const hit = checks.filter((check) => check.pass).map((check) => check.id);
  return {
    checks, hit, missed: checks.filter((check) => !check.pass).map((check) => check.id),
    qualityEarned: hit.length, qualityPossible: checks.length, taskSuccess: hit.length === checks.length,
  };
}

function definition(arm, repository, serenaCommit) {
  if (arm.startsWith('weavatrix-')) return {
    command: 'npx.cmd', args: ['-y', 'weavatrix@1.8.0', 'mcp', '.', '--profile=code'], cwd: repository,
    name: 'weavatrix', profile: 'code', format: arm.slice('weavatrix-'.length),
  };
  if (arm === 'cortex-default') return {
    command: path.join(ROOT, 'target', 'release', 'cortex-mcp.exe'), args: ['--profile', 'context'], cwd: repository,
    name: 'cortex', profile: 'context', format: null,
  };
  return {
    command: 'uvx',
    args: ['--from', `git+https://github.com/oraios/serena@${serenaCommit}`, 'serena', 'start-mcp-server', '--project', repository, '--context', 'ide-assistant', '--enable-web-dashboard', 'False', '--enable-gui-log-window', 'False'],
    cwd: repository, name: 'serena', profile: 'ide-assistant', format: null,
  };
}

async function representativeCall(client, arm, repository, format) {
  if (arm.startsWith('weavatrix-')) {
    await client.call('open_repo', { path: '.', output_format: format });
    return client.call('search_code', { query: 'ArchiveOptions', output_format: format });
  }
  if (arm === 'cortex-default') return client.call('weavatrix_context_compile', {
    repository, task: 'Locate ArchiveOptions and return its complete defining context.',
    symbol: 'ArchiveOptions', targeted: true, maxTokens: 4000,
  });
  return client.call('find_symbol', {
    name_path_pattern: 'ArchiveOptions', relative_path: 'src/options/types.rs', include_body: true,
  });
}

async function run(options = {}) {
  const repository = path.resolve(options.repository || path.join(ROOT, '..', 'weavatrix-search'));
  const outputRoot = path.resolve(options.outputRoot || path.join(ROOT, '.cortex-loom', 'bench', 'p0'));
  const trials = Number(options.trials || 3);
  const manifest = await detectEnvironment({
    suiteVersion: 'schema-payload-v2', targetRepository: repository,
    mcp: { protocolVersion: '2025-06-18', transport: 'stdio', representationCounted: 'content-first' },
  });
  const serenaCommit = manifest.serena.value && manifest.serena.value.commit;
  if (!serenaCommit) throw new Error(`cannot resolve current Serena: ${manifest.serena.reason}`);
  const rows = [];
  const defects = [];
  const schedule = alternatingOrders(ARMS, trials);
  for (const [trial, order] of schedule.entries()) {
    for (const arm of order) {
      process.stderr.write(`schema/payload trial ${trial + 1}/${trials}: ${arm}\n`);
      const spec = definition(arm, repository, serenaCommit);
      const client = new McpClient(spec);
      const started = process.hrtime.bigint();
      try {
        await client.start();
        const result = await representativeCall(client, arm, repository, spec.format);
        const totalMs = Number(process.hrtime.bigint() - started) / 1e6;
        const grade = payloadGrade(client, result, arm);
        const artifact = writeArtifact(outputRoot, `artifacts/schema-payload/${trial}-${arm}.txt`, result.countedText);
        const failureClass = grade.taskSuccess ? null : arm.startsWith('weavatrix-') ? 'WEAVATRIX_BUG' : arm === 'cortex-default' ? 'CORTEX_BUG' : 'COMPETITOR_GAP';
        rows.push({
          suite: 'schema-payload', task: 'initialize-list-call', arm, trial, order,
          qualityEarned: grade.qualityEarned, qualityPossible: grade.qualityPossible,
          taskSuccess: grade.taskSuccess, sufficient: null, falseConfidence: false, failureClass,
          selectedTokens: result.countedTokens, deliveredTokens: result.countedTokens,
          wireTokens: result.wireTokens, schemaTokens: client.schemaTokens, calls: 3,
          latencyMs: totalMs, callLatencyMs: result.latencyMs,
          payloadFormat: result.format, countedRepresentation: result.countedRepresentation,
          serverInfo: client.initializeResult.serverInfo, toolCount: client.tools.length,
          grade, artifact,
        });
        if (failureClass === 'WEAVATRIX_BUG') defects.push({
          classification: failureClass, suite: 'schema-payload', task: 'initialize-list-call', arm, trial,
          engineVersion: manifest.engines['npm-weavatrix'], expectedFormat: expectedPayloadFormat(arm),
          actual: { format: result.format, countedRepresentation: result.countedRepresentation, grade, artifact },
          fixDirection: 'Make the declared output_format match the emitted MCP representation without losing the client-visible payload.',
        });
      } catch (error) {
        rows.push({
          suite: 'schema-payload', task: 'initialize-list-call', arm, trial, order,
          qualityEarned: 0, qualityPossible: 5, taskSuccess: false, sufficient: null,
          falseConfidence: false, failureClass: 'HARNESS_BUG', error: error.message,
        });
        defects.push({ classification: 'HARNESS_BUG', suite: 'schema-payload', arm, trial, error: error.message });
      } finally {
        client.close();
      }
    }
  }
  const report = {
    schemaVersion: 'cortex-benchmark.v2', historical: false, manifest, schedule, rows, defects,
    scoreboard: summarizeRows(rows),
  };
  writeCurrentReport(path.join(outputRoot, 'schema-payload.json'), report);
  return report;
}

if (require.main === module) {
  run({ trials: process.env.P0_TRIALS || 3 }).catch((error) => {
    console.error(error.stack || error.message);
    process.exitCode = 1;
  });
}

module.exports = { ARMS, expectedPayloadFormat, payloadGrade, run };
