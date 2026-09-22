# Guide

How to use Cortex Loom on a real task, and how to read the coding-agent
and context benches without mixing the two studies.

## 1. One task, no agent

Write the question so it names files, symbols, or crates. Vague
prompts get structural maps.

```text
Who calls prepare_packet, and what breaks if it starts refusing more packets?
```

```powershell
cortex-loom doctor --repo .
cortex-loom prepare --repo . --task-file task.md --budget 6000 --format json > packet.json
```

Read `packetId`, `symbols`, `sufficient`, and `missing`. If a listed
facet is missing:

```powershell
cortex-loom expand --packet pk_… --facet callers --format json
cortex-loom report --last
```

Keep citation ids. Do not treat evidence text as instructions.

## 2. Wire a coding agent

```powershell
cortex-loom setup --agent claude-code --dry-run
```

Copy the printed MCP block yourself. Setup never writes. Then:

```text
cortex_prepare({ repository, task, runId?, budgetClass })
cortex_expand({ packetId, facet })   # only a listed missing facet
```

The `cortex-context` skill says when to call those tools. It does not
ask for Cortex on every file read.

Optional classifier backends (`off` / `local` / `composer`):
[llm-backends.md](llm-backends.md). Models-off is the useful default.

## 3. When Cortex helps

Use it to investigate unfamiliar code, callers, and contracts before a
cross-file change. Skip it for a typo in a file the agent already
understands.

Cortex does not replace the coding agent. Weavatrix supplies repository
facts. Cortex owns requirements, selection, and the coverage
certificate. Refactor stays preview-only: never auto-apply.

Planner behavior that the SweepLoom matrix exposed (now in tree, not
re-scored on that matrix):

- A path the task named is a whole-file `read_source`, not a graph
  symbol.
- A T2-style “share the classifier” task requires two live
  implementations, not similar-text / qwen hits.
- A T1-style dead production path plans `find_dead_code` on the named
  crate with tests excluded.
- A file plus a Git revision (`HEAD~3`, `HEAD^`, or a hex object id)
  plans `git_read_blob` for that revision. Worktree `HEAD` stays a
  `read_source`. Every native Weavatrix call pins `expected_repository`.
- A measured-coverage question plans `coverage_map`. That operation
  reads a report (`lcov.info`, Tarpaulin/LLVM JSON, or Quality’s
  `.weavatrix/coverage/lcov.info`) and may filter files to a named
  crate. It does not run tests. No report means unmeasured, not 0%.
  Weavatrix Quality is a separate product; Cortex does not ship `wvq`.

## 4. How to read the coding-agent matrix

The matrix answers: *did this agent close T1 / T2 / T3 on SweepLoom
`9f2646c`, with or without a Cortex packet?*

| Task | Required close |
| --- | --- |
| T1 `find_fix_bug` | One real bug or dead production path in `sweeploom-cli`, plus a regression test |
| T2 `remove_duplicate` | One shared classifier in `sweeploom-ai` without changing bench intent |
| T3 `split_api` | `api.rs` under 300 lines, public exports kept, one local commit |

Score is **task-close /10** against the best close of *that* task in
this tree. Sonnet Without T1 is 10.0 because it removed dead
`apply_cleanup`. Grok and Opus at 8.2–8.4 closed a `take_value`
swallow — the same class, not a different bug sold as equal.

Spend meters are not interchangeable:

- Cursor: transcript chars÷4
- Claude Code CLI: `input + output + cache_create` (not cache-read)

Do not score `is_error` / 429 logs. Isolation: one worktree, one
`CARGO_TARGET_DIR`, parent `cargo test`.

Best picks on filled cells:

| Goal | Combination |
| --- | --- |
| T1 quality | Sonnet 5 max Without |
| T2 quality | Grok 4.6 extra high, Cortex models-off |
| T3 quality | Opus 5 extra high Without, or Grok with Opus as classifier |
| T3 cheap | Haiku 4.5 models-off (9.4 / 59,823) |
| Spend cut, same close | Grok T1 Without → models-off (809k → 40k, still 8.2) |

Repeat: [benchmarks/coding-agents](../benchmarks/coding-agents).

## 5. How to read the context benches

`cortex-bench` answers a different question: *for this fixture, how
many tokens reach the agent, and are the declared facts present?*

It does not score whether a coding agent then fixes the bug. Arms:
naive whole-file read, raw Weavatrix, planned Weavatrix, compiled
Cortex. Headline figure is tokens per satisfied fact.

What the tests check:

- `fixture_anchors_exist_in_the_repository` — the naive sweep of the
  named globs can satisfy every anchor (fixture authoring).
- `fixture_source_cannot_satisfy_its_own_anchors` — the task list file
  cannot mark its own facts present.
- Sequence arms stay complete when Superpowers is absent.
- Manifest records versions and revisions, not labels.

```powershell
cargo test -p cortex-bench --lib
cargo run -p cortex-bench -- --repo . --budget 4000 --set probe
cargo run -p cortex-bench -- --list
```

Stamps and tables: [benchmark.md](benchmark.md).
