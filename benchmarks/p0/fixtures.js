const LIVE_TASKS = [
  {
    id: 'T1-entry-limit',
    difficulty: 'easy',
    question: 'In this Rust crate, what limits how many bytes a single archive member may expand to during search, and what happens when that limit is exceeded? Name the exact identifiers.',
    symbol: 'read_limited',
    naiveGlobs: ['src/archive'],
    searchPattern: 'read_limited|max_entry_bytes',
    serenaFile: 'src/archive/containers.rs',
    required: [
      { id: 'option-field', pattern: /max_entry_bytes/i },
      { id: 'enforcer', pattern: /read_limited/i },
      { id: 'is-an-error', pattern: /\berror\b|\bErr\b|returns? an error|fails?\b/i },
    ],
    bonus: [{ id: 'default-16mib', pattern: /16\s*(MiB|MB)|16\s*\*\s*1024|16777216/i }],
  },
  {
    id: 'T2-multiline-blocks',
    difficulty: 'medium',
    question: 'How does multiline search group matches into a single reported block? State the condition under which a new match joins the current block, what happens when it does not, and what quiet result mode does instead.',
    symbol: 'finish_block',
    naiveGlobs: ['src/multiline'],
    searchPattern: 'finish_block|end_line|quiet_match',
    serenaFile: 'src/multiline/mod.rs',
    required: [
      { id: 'block-type', pattern: /\bBlock\b/ },
      { id: 'join-condition', pattern: /end_line|same block|overlap|start_line/i },
      { id: 'flush', pattern: /finish_block/i },
      { id: 'quiet-path', pattern: /quiet/i },
    ],
    bonus: [{ id: 'quiet-fn', pattern: /quiet_match/i }],
  },
  {
    id: 'T3-silent-archive-miss',
    difficulty: 'hard',
    question: 'A regex matches a file on disk but returns nothing when the same file sits inside a .tar.gz. List every mechanism in this crate that can silently cause that, and state the path format under which a match inside an archive would have been reported.',
    symbol: 'search_compressed_tar',
    naiveGlobs: ['src/archive', 'src/options'],
    searchPattern: 'search_compressed_tar|safe_virtual_path|max_entries|archives',
    serenaFile: 'src/archive/compression.rs',
    required: [
      { id: 'enabled-flag', pattern: /\benabled\b/i },
      { id: 'size-limit', pattern: /max_entry_bytes|max_expanded_bytes|max_archive_bytes/i },
      { id: 'entry-count', pattern: /max_entries/i },
      { id: 'feature-gate', pattern: /feature\s*=?\s*"?archives|cfg\(feature/i },
      { id: 'unsafe-path-skip', pattern: /safe_virtual_path|\.\.\/|parent ?dir|traversal/i },
    ],
    bonus: [{ id: 'bang-path', pattern: /!\{?inner|outer_path\}!|"!"|`!`|archive!|path!/ }],
  },
];

const SYMBOLS = [
  { name: 'read_limited', definitionFile: 'src/archive/containers.rs', kind: 'fn' },
  { name: 'finish_block', definitionFile: 'src/multiline/mod.rs', kind: 'fn' },
  { name: 'quiet_match', definitionFile: 'src/collector/operations.rs', kind: 'fn' },
  { name: 'safe_virtual_path', definitionFile: 'src/archive/containers.rs', kind: 'fn' },
  { name: 'search_expanded_file', definitionFile: 'src/archive/compression.rs', kind: 'fn' },
  { name: 'ArchiveOptions', definitionFile: 'src/options/types.rs', kind: 'struct' },
];

const IMPLEMENTATION_FIELDS = [
  'enabled',
  'max_archive_bytes',
  'max_entry_bytes',
  'max_expanded_bytes',
  'max_entries',
  'max_decoder_memory_bytes',
];

const IMPLEMENTATION_TASK =
  'In this Rust crate there is a struct `ArchiveOptions` (resource and expansion limits for archive search). ' +
  'Add a constructor `pub fn disabled() -> Self` on `ArchiveOptions` returning a configuration with archive search disabled ' +
  'and EVERY numeric limit field set to 0 (do not reuse Default). ' +
  'Reply with ONLY Rust code that can be appended verbatim at the END of the file that defines the struct: ' +
  'an `impl ArchiveOptions { ... }` block containing `disabled()`. ' +
  'No markdown fences, no explanations, no test module.';

function hiddenImplementationTest() {
  return `
#[cfg(test)]
mod harness_disabled_check {
    use super::ArchiveOptions;

    #[test]
    fn disabled_turns_everything_off_and_zeroes_every_limit() {
        let options = ArchiveOptions::disabled();
        assert!(!options.enabled);
        assert_eq!(options.max_archive_bytes, 0);
        assert_eq!(options.max_entry_bytes, 0);
        assert_eq!(options.max_expanded_bytes, 0);
        assert_eq!(options.max_entries, 0);
        assert_eq!(options.max_decoder_memory_bytes, 0);
    }
}
`;
}

function gradeAnswer(task, answer) {
  const hit = task.required.filter((fact) => fact.pattern.test(answer)).map((fact) => fact.id);
  const missed = task.required.filter((fact) => !fact.pattern.test(answer)).map((fact) => fact.id);
  const bonus = (task.bonus || []).filter((fact) => fact.pattern.test(answer)).map((fact) => fact.id);
  return {
    hit,
    missed,
    bonus,
    qualityEarned: hit.length,
    qualityPossible: task.required.length,
    taskSuccess: missed.length === 0,
  };
}

function gradeContext(task, context) {
  const evidenceOnly = String(context).split(task.question).join('');
  return gradeAnswer(task, evidenceOnly);
}

module.exports = {
  IMPLEMENTATION_FIELDS,
  IMPLEMENTATION_TASK,
  LIVE_TASKS,
  SYMBOLS,
  gradeAnswer,
  gradeContext,
  hiddenImplementationTest,
};
