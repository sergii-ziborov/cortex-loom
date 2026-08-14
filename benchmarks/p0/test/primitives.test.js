const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');

const { archiveCurrentReport, writeCurrentReport } = require('../lib/harness');
const { extractPayload, modelVisibleText } = require('../lib/mcp');
const {
  commandInvocation,
  configuredWeavatrixVersion,
  parseCargoLock,
  modelIdentity,
  modelNotUsed,
} = require('../lib/manifest');
const { alternatingOrders } = require('../lib/schedule');
const { classifyFailure, summarizeRows } = require('../lib/scoreboard');
const { LIVE_TASKS, gradeContext } = require('../fixtures');

test('a compiled Cortex envelope is consumed as the inner packet', () => {
  const packet = '## [WX-DEF] definition\npub enabled: bool\n';
  const envelope = JSON.stringify({
    repository: 'repo',
    context: { content: packet, includedIds: ['WX-DEF'] },
    sufficiency: { sufficient: true },
  });
  assert.equal(modelVisibleText(envelope), packet);
  assert.equal(modelVisibleText('plain grep output'), 'plain grep output');
});

test('payload accounting counts exactly one client-visible representation', () => {
  const textOnly = extractPayload({ content: [{ type: 'text', text: '{"value":1}' }] });
  assert.equal(textOnly.format, 'text');
  assert.equal(textOnly.countedRepresentation, 'content');
  assert.equal(textOnly.countedText, '{"value":1}');

  const structuredOnly = extractPayload({ structuredContent: { value: 1 } });
  assert.equal(structuredOnly.format, 'structured');
  assert.equal(structuredOnly.countedRepresentation, 'structuredContent');

  const mirrored = extractPayload({
    content: [{ type: 'text', text: '{"value":1}' }],
    structuredContent: { value: 1 },
  });
  assert.equal(mirrored.format, 'mirrored');
  assert.equal(mirrored.countedRepresentation, 'content');

  const distinct = extractPayload({
    content: [{ type: 'text', text: 'value = 1' }],
    structuredContent: { value: 1 },
  });
  assert.equal(distinct.format, 'dual-distinct');
  assert.equal(distinct.countedRepresentation, 'content');
  assert.ok(distinct.wireTokens > distinct.countedTokens);
});

test('three-trial schedule alternates natural reverse and rotated order', () => {
  assert.deepEqual(alternatingOrders(['a', 'b', 'c', 'd'], 3), [
    ['a', 'b', 'c', 'd'],
    ['d', 'c', 'b', 'a'],
    ['c', 'd', 'a', 'b'],
  ]);
  assert.deepEqual(alternatingOrders(['a', 'b'], 3), [
    ['a', 'b'],
    ['b', 'a'],
    ['a', 'b'],
  ]);
});

test('failure attribution follows source to Weavatrix to Cortex to model', () => {
  assert.equal(classifyFailure({ harnessValid: false }), 'HARNESS_BUG');
  assert.equal(classifyFailure({
    harnessValid: true,
    taskSuccess: false,
    truthPresent: true,
    rawQueryExpectedLossless: true,
    rawWeavatrixPresent: false,
  }), 'WEAVATRIX_BUG');
  assert.equal(classifyFailure({
    harnessValid: true,
    taskSuccess: false,
    rawWeavatrixPresent: true,
    cortexPresent: false,
  }), 'CORTEX_BUG');
  assert.equal(classifyFailure({
    harnessValid: true,
    taskSuccess: false,
    cortexPresent: true,
    modelHadEvidence: true,
  }), 'MODEL_FAILURE');
});

test('scoreboard preserves samples and reports false confidence with median range', () => {
  const result = summarizeRows([
    { suite: 'live', task: 't1', arm: 'cortex', trial: 0, qualityEarned: 2, qualityPossible: 3, sufficient: true, taskSuccess: false, falseConfidence: false, latencyMs: 30 },
    { suite: 'live', task: 't1', arm: 'cortex', trial: 1, qualityEarned: 3, qualityPossible: 3, sufficient: true, taskSuccess: true, latencyMs: 10 },
    { suite: 'live', task: 't1', arm: 'cortex', trial: 2, qualityEarned: 3, qualityPossible: 3, sufficient: true, taskSuccess: true, latencyMs: 20 },
  ]);

  assert.equal(result.groups[0].samples.length, 3);
  assert.equal(result.groups[0].falseConfidence, 0);
  assert.deepEqual(result.groups[0].latencyMs, { median: 20, min: 10, max: 30 });
});

test('context grading does not count an echoed task prompt as evidence', () => {
  const task = LIVE_TASKS[0];
  assert.equal(gradeContext(task, task.question).taskSuccess, false);
  assert.equal(gradeContext(task, `${task.question}\nread_limited returns an error`).taskSuccess, false);
});

test('manifest parsers derive immutable versions from runtime artifacts', () => {
  const packages = parseCargoLock(`
[[package]]
name = "mcport"
version = "0.5.0"

[[package]]
name = "weavatrix-rust"
version = "2.5.0"
`);
  assert.equal(packages.mcport, '0.5.0');
  assert.equal(packages['weavatrix-rust'], '2.5.0');
  assert.equal(configuredWeavatrixVersion({
    mcpServers: { weavatrix: { args: ['-y', 'weavatrix@1.7.0', 'mcp'] } },
  }), '1.7.0');
  assert.deepEqual(modelIdentity('qwen3.5:9b', {
    models: [{ name: 'qwen3.5:9b', digest: 'sha256:abc', details: { parameter_size: '9.7B' } }],
  }, '0.32.6', { temperature: 0, seed: 7 }), {
    used: true,
    name: 'qwen3.5:9b',
    digest: 'sha256:abc',
    runtime: 'ollama',
    runtimeVersion: '0.32.6',
    parameters: { temperature: 0, seed: 7 },
    details: { parameter_size: '9.7B' },
  });
  assert.deepEqual(modelNotUsed(), {
    used: false,
    reason: 'suite does not invoke a model',
    name: null,
    digest: null,
    runtime: null,
    runtimeVersion: null,
    parameters: {},
    details: {},
  });
});

test('Windows command scripts execute through ComSpec', () => {
  assert.deepEqual(
    commandInvocation('npm.cmd', ['view', 'weavatrix@1.7.0'], 'win32', 'C:\\Windows\\cmd.exe'),
    {
      command: 'C:\\Windows\\cmd.exe',
      args: ['/d', '/c', 'npm.cmd', 'view', 'weavatrix@1.7.0'],
    },
  );
  assert.deepEqual(commandInvocation('git', ['status'], 'win32', 'C:\\Windows\\cmd.exe'), {
    command: 'git',
    args: ['status'],
  });
});

test('superseded reports are explicitly archived before current replacement', (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'cortex-p0-report-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const destination = path.join(directory, 'symbol.json');
  fs.writeFileSync(destination, JSON.stringify({ schemaVersion: 'old', historical: false }));

  const historicalPath = archiveCurrentReport(destination);
  assert.equal(JSON.parse(fs.readFileSync(destination, 'utf8')).historical, true);
  assert.equal(JSON.parse(fs.readFileSync(historicalPath, 'utf8')).historical, true);

  writeCurrentReport(destination, { schemaVersion: 'new' });
  assert.deepEqual(JSON.parse(fs.readFileSync(destination, 'utf8')), {
    schemaVersion: 'new',
    historical: false,
  });
});
