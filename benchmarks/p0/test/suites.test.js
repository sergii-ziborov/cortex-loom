const assert = require('node:assert/strict');
const test = require('node:test');

const {
  IMPLEMENTATION_FIELDS,
  LIVE_TASKS,
  SYMBOLS,
  gradeAnswer,
} = require('../fixtures');
const {
  groundTruthFromHits,
  scoreSet,
  serenaSet,
  weavatrixSet,
} = require('../symbol');
const {
  gradeImplementationCode,
  gradeImplementationContext,
  sanitizeRust,
} = require('../implementation');
const {
  parseGraphDiffFiles,
  parseHistoryIds,
  scoreExactSet,
} = require('../git');
const { expectedPayloadFormat, payloadGrade } = require('../schema-payload');
const {
  EXPECTED_REPORT_NAMES,
  REQUIRED_ENGINES,
  aggregateReports,
  validateCurrentInputs,
} = require('../run');
const { liveHarnessHash } = require('../live');

test('recovered live oracle keeps the original 3 4 5 quality denominators', () => {
  assert.deepEqual(LIVE_TASKS.map((task) => task.required.length), [3, 4, 5]);
  assert.deepEqual(LIVE_TASKS.map((task) => task.id), [
    'T1-entry-limit',
    'T2-multiline-blocks',
    'T3-silent-archive-miss',
  ]);
  assert.match(liveHarnessHash(), /^[0-9a-f]{64}$/);
});

test('implementation hidden oracle checks enabled plus every numeric field', () => {
  assert.deepEqual(IMPLEMENTATION_FIELDS, [
    'enabled',
    'max_archive_bytes',
    'max_entry_bytes',
    'max_expanded_bytes',
    'max_entries',
    'max_decoder_memory_bytes',
  ]);
});

test('symbol truth set retains all six hand-audited symbols', () => {
  assert.deepEqual(SYMBOLS.map((symbol) => symbol.name), [
    'read_limited',
    'finish_block',
    'quiet_match',
    'safe_virtual_path',
    'search_expanded_file',
    'ArchiveOptions',
  ]);
});

test('answer grading reports exact required facts instead of a boolean', () => {
  const task = LIVE_TASKS[0];
  const grade = gradeAnswer(task, 'read_limited enforces max_entry_bytes and returns an error');
  assert.deepEqual(grade.hit, ['option-field', 'enforcer', 'is-an-error']);
  assert.deepEqual(grade.missed, []);
  assert.equal(grade.qualityEarned, 3);
  assert.equal(grade.qualityPossible, 3);
});

test('symbol parsers compare function-level references on one canonical key', () => {
  const lines = [
    'src/a.rs:1:fn target() {}',
    'src/a.rs:5:    target();',
    'src/a.rs:9:use crate::target;',
  ];
  const source = ['fn target() {}', 'fn caller() {', '  target();', '}', '', '', '', '', 'use crate::target;'];
  const truth = groundTruthFromHits(lines, () => source, 'target').truth;
  assert.deepEqual([...truth], ['src/a.rs::caller']);

  const wx = weavatrixSet(JSON.stringify({ dependents: [
    { distance: 1, node: { kind: 'function', label: 'caller', span: { file: 'src/a.rs' } } },
    { distance: 2, node: { kind: 'function', label: 'indirect', span: { file: 'src/a.rs' } } },
  ] }));
  const serena = serenaSet(JSON.stringify({ 'src\\a.rs': { Function: [{ name_path: 'caller' }] } }));
  assert.deepEqual(scoreSet(wx, truth).missed, []);
  assert.deepEqual(scoreSet(serena, truth).extra, []);
});

test('implementation grading requires every field and strips only outer fences', () => {
  const code = sanitizeRust('```rust\nimpl ArchiveOptions { pub fn disabled() -> Self { Self { enabled: false, max_archive_bytes: 0, max_entry_bytes: 0, max_expanded_bytes: 0, max_entries: 0, max_decoder_memory_bytes: 0 } } }\n```');
  assert.equal(code.startsWith('impl ArchiveOptions'), true);
  assert.equal(gradeImplementationContext(code).complete, true);
  assert.equal(gradeImplementationCode(code).qualityEarned, 7);
  assert.equal(gradeImplementationCode(code.replace('max_entries: 0', 'max_entries: 1')).taskSuccess, false);
});

test('git parsers retain immutable commit ids and changed source files', () => {
  const history = parseHistoryIds(JSON.stringify({ commits: [{ id: 'a'.repeat(40) }, { id: 'b'.repeat(40) }] }));
  assert.deepEqual([...history], ['a'.repeat(40), 'b'.repeat(40)]);
  const files = parseGraphDiffFiles(JSON.stringify({ nodes: {
    added: [{ id: 'file:src/new.rs' }],
    changed: [{ after: { id: 'file:src/changed.rs' } }],
    removed: [{ id: 'file:src/old.rs' }],
  } }));
  assert.deepEqual([...files].sort(), ['src/changed.rs', 'src/new.rs', 'src/old.rs']);
  assert.equal(scoreExactSet(files, new Set(files)).taskSuccess, true);
});

test('schema payload contract distinguishes text, mirrored, and structured formats', () => {
  assert.equal(expectedPayloadFormat('weavatrix-text'), 'text');
  assert.equal(expectedPayloadFormat('weavatrix-json'), 'mirrored');
  assert.equal(expectedPayloadFormat('weavatrix-structured'), 'structured');
  assert.equal(expectedPayloadFormat('cortex-default'), 'text');
  const grade = payloadGrade({ initializeResult: { serverInfo: { version: '1.7.0' } }, tools: [{}], schemaTokens: 10 }, {
    isError: false, countedText: '{}', countedTokens: 1, wireTokens: 2, format: 'text', countedRepresentation: 'content',
  }, 'weavatrix-text');
  assert.equal(grade.taskSuccess, true);
});

test('unified scoreboard keeps quality confidence tokens calls and latency together', () => {
  const report = aggregateReports([{ name: 'sample', report: {
    historical: false,
    manifest: { suiteVersion: 'sample-v1' },
    defects: [{ classification: 'MODEL_FAILURE', task: 't1' }],
    rows: [{
      suite: 'sample', task: 't1', arm: 'a1', trial: 0,
      qualityEarned: 1, qualityPossible: 2, taskSuccess: false,
      sufficient: true, falseConfidence: false, failureClass: 'MODEL_FAILURE',
      selectedTokens: 10, deliveredTokens: 11, modelPrefillTokens: 12,
      modelGenerationTokens: 2, calls: 3, latencyMs: 4,
    }],
  } }]);
  assert.equal(report.historical, false);
  assert.equal(report.rows[0].falseConfidence, false);
  assert.deepEqual(report.summary[0].tokens.selected, { median: 10, min: 10, max: 10 });
  assert.equal(report.hasUnclassified, false);
  assert.deepEqual(report.defects, [{
    sourceReport: 'sample', classification: 'MODEL_FAILURE', task: 't1',
  }]);
});

test('current scoreboard gate requires every report and three trials per group', () => {
  const commit = 'a'.repeat(40);
  const inputs = EXPECTED_REPORT_NAMES.map((name) => ({
    name,
    report: {
      schemaVersion: 'cortex-benchmark.v2',
      historical: false,
      manifest: {
        cortex: { commit: { value: commit } },
        target: { commit: { value: commit } },
        engines: Object.fromEntries(REQUIRED_ENGINES.map((engine) => [engine, '1.0.0'])),
        registryWeavatrix: { value: '1.0.0' },
        serena: { value: { commit } },
        model: { used: false, parameters: {} },
        mcp: { transport: 'stdio' },
      },
      rows: name.startsWith('deterministic-') ? [{
        suite: 'deterministic-probe', task: 't1', arm: 'a1',
        trial: Number(name.at(-1)), taskSuccess: true,
      }] : [0, 1, 2].map((trial) => ({
        suite: name, task: 't1', arm: 'a1', trial, taskSuccess: true,
      })),
    },
  }));
  assert.equal(validateCurrentInputs(inputs), inputs);
  assert.throws(
    () => validateCurrentInputs(inputs.filter((input) => input.name !== 'live')),
    /missing reports: live/,
  );
  inputs.find((input) => input.name === 'symbol').report.historical = true;
  assert.throws(() => validateCurrentInputs(inputs), /symbol is not explicitly marked historical:false/);
});
