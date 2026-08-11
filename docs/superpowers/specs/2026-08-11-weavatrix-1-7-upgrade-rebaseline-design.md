# Weavatrix 1.7 upgrade and Cortex rebaseline

Date: 2026-08-11

## Decision

Cortex Loom will pin its external Weavatrix MCP configuration to
`weavatrix@1.7.0` and remeasure the quality and cost boundaries that changed in
the 1.7 release. The embedded repository-intelligence engine remains
`weavatrix-rust 2.5.0`, which is already current.

This design is P0 only. It upgrades the stale pin, makes the benchmark evidence
self-describing, reruns the complete current scoreboard, and attributes every
failure. It does not add a major retrieval, routing, UI, model, or mutation
feature and does not repair benchmark failures discovered during the run.

The upgrade does not add a refactor apply path. Cortex continues to consume
`weavatrix-refactor-plan 0.1.1` and `weavatrix-edit 0.1.7` only for validation
and preview. The separate `weavatrix-refactor 1.0.4`,
`weavatrix-rust-refactor 0.1.5`, and `weavatrix-worktree 0.2.0` packages are
outside this change because they introduce mutation and confirmation concerns
that need their own design.

## Registry snapshot

The implementation must record a fresh registry check before editing:

| package | Cortex before | current registry version | action |
| --- | ---: | ---: | --- |
| `blazingly-json` | 0.1.4 | 0.1.4 | keep |
| `mcport` | 0.5.0 | 0.5.0 | keep |
| `weavatrix-rust` | 2.5.0 | 2.5.0 | keep |
| `weavatrix-refactor-plan` | 0.1.1 | 0.1.1 | keep |
| `weavatrix-edit` | 0.1.7 | 0.1.7 | keep |
| npm `weavatrix` | 1.6.0 | 1.7.0 | upgrade |

The similarly named `weavatrix-refactor 1.0.4` is an independent MCP server,
not a newer version of `weavatrix-refactor-plan`.

## Considered approaches

### Recommended: targeted upgrade plus layered rebaseline

Change the stale npm pin, retain the preview-only refactor boundary, and rerun
only measurements that can distinguish the embedded Rust engine from the
external npm server. This produces attributable results without silently
granting Cortex mutation authority.

### Dependency-only bump

Change `.mcp.json` and run the workspace gates. This is fast but cannot support
claims about the 1.7 symbol and Git changes, and the in-process Cortex benchmark
would misleadingly appear to measure the npm upgrade even though it does not
launch npm Weavatrix.

### Refactor integration at the same time

Add the current refactor runtime and worktree crates while upgrading Weavatrix.
This is rejected for this change: preview, confirmation, revision binding,
single-use authority, audit, and rollback require a separate threat model and
test plan. Combining them would make benchmark deltas hard to attribute.

## Change scope

1. Update `.mcp.json` from `weavatrix@1.6.0` to `weavatrix@1.7.0`.
2. Add or update a contract test that fails on a stale external MCP pin.
3. Extend every benchmark report with an immutable environment manifest and a
   common scoreboard schema.
4. Rerun the deterministic, live-model, implementation, symbol, Git, MCP
   schema/payload, and current-Serena comparisons on pinned revisions.
5. Update benchmark documentation with versions, commands, report paths, and
   new results. Every older row is labelled `historical`; no old row may appear
   under a `current` heading.
6. Produce an actionable owner-specific defect packet for every missing,
   distorted, or falsely trusted fact. P0 records these defects but does not
   change the retrieval engine or Cortex selection policy to repair them.
7. Do not add a refactor apply tool, confirmation token, JavaScript planner,
   or write-capable profile.

## Losslessness and false confidence

Weavatrix is held to a lossless evidence contract within each language and
operation scope it declares:

- source-to-fact extraction and the internal graph must not invent, distort,
  or silently drop a supported fact;
- an unbudgeted raw query must return all matching facts in its declared scope;
- a budgeted or paginated view may omit facts only when the response reports
  that incompleteness in machine-readable metadata such as `fit: false`,
  `truncated: true`, a non-zero dropped count, or a continuation cursor;
- a response that omits a required fact while claiming or implying complete
  coverage is a Weavatrix correctness bug, not an acceptable recall trade-off.

Cortex context compilation is intentionally selective, but selection must not
be confused with truth. `sufficient: true` is valid only when the delivered
packet satisfies every declared evidence obligation and every upstream source
used for that obligation reports complete coverage. Any downstream task
failure under `sufficient: true` is counted as false confidence until the
failure is attributed and disproved.

The benchmark assigns exactly one primary owner to every failed required fact:

| classification | condition | required output |
| --- | --- | --- |
| `WEAVATRIX_BUG` | source truth exists, but raw/unbudgeted Weavatrix omits or distorts it; or a partial response claims completeness | Weavatrix reproduction and fix packet |
| `CORTEX_BUG` | raw Weavatrix contains the fact, but the delivered Cortex packet omits, corrupts, or falsely trusts it | Cortex reproduction and fix packet |
| `MODEL_FAILURE` | the delivered packet contains complete cited evidence, but the model answer or implementation is wrong | model prompt/output record |
| `HARNESS_BUG` | oracle, parser, version capture, ordering, or scoring is invalid | invalidate the affected row and repair the harness before rerun |

An unresolved attribution is reported as `UNCLASSIFIED` and fails the P0 exit
criterion. Token savings never compensate for false confidence.

Each Weavatrix defect packet must contain:

- target repository and exact commit;
- server, engine, parser, and operation versions;
- language, tool name, full arguments, and MCP response format;
- expected fact with authoritative source path and span;
- actual result or result artifact hash;
- completeness, truncation, dropped-item, budget, and cursor metadata;
- the smallest deterministic regression test that should fail before the fix;
- the suspected failing layer and a concrete fix direction, without claiming a
  root cause that the evidence does not prove.

The same structure is used for Cortex defects, replacing the engine layer with
planner, gather, compiler, delivery, or sufficiency as appropriate.

## Report manifest and common schema

Every JSON report, including deterministic-only reports, records the values
actually observed by the harness rather than labels typed into documentation:

- report schema version, benchmark suite version, timestamp, and run ID;
- Cortex commit and dirty-state flag;
- target repository URL/path, exact commit, and dirty-state flag;
- operating system, architecture, and benchmark command;
- `blazingly-json`, `mcport`, `weavatrix-rust`, npm Weavatrix,
  `weavatrix-refactor-plan`, and `weavatrix-edit` versions when present;
- competitor executable/package version and immutable revision when available;
- model name, runtime, runtime version, model digest, context length,
  temperature, seed, thinking mode, and other generation parameters;
- MCP protocol version, transport, server profile, payload representation
  (`text`, `structured`, or `mirrored`), and the representation counted;
- trial index, exact arm order, warm/cold state, timeouts, and retry count.

If a value cannot be detected, the report records `unknown` plus a reason. It
must not substitute a manually written version string.

All suites project their raw results into one scoreboard row shape:

- task and arm identifiers;
- quality numerator, denominator, scoring oracle, compilation, and hidden-test
  result where applicable;
- `sufficient`, actual task success, false-confidence boolean, and failure
  classification;
- selected, delivered, model-prefill, and model-generation tokens;
- MCP/tool calls and model turns;
- end-to-end and tool latency, with raw samples plus median and range;
- artifact paths or hashes for replay.

## Measurement design

### Embedded Cortex retrieval

Run the ten-task, 4,000-token deterministic probe at least three times with
distinct output paths and the same non-time-varying stamp. After normalizing
the per-run identifiers and timestamps, the scored content must be identical.
Report selected and delivered tokens, anchor recall, sufficiency, actual oracle
success, false confidence, and failure attribution for every arm.

This measurement validates the current Cortex adapter over
`weavatrix-rust 2.5.0`; it is a regression check, not evidence that changing
the npm pin altered the in-process engine.

### External Weavatrix MCP

Launch exactly `weavatrix@1.7.0` over stdio and verify initialization,
`tools/list`, and real tool calls. Capture the server and engine versions.
Measure the same fixed workloads used by the earlier comparison:

- direct symbol dependents, including type references and the known
  receiver-method case;
- recent commit history with default compact output;
- `HEAD~10` graph diff with its compact default and an explicit token budget;
- analytics-enabled history separately, so compact history is not charged for
  hotspot and co-change tables it did not request.

For each workload report recall or required facts, payload tokens, tool calls,
and elapsed time. Keep CLI comparisons on the same repository revision. Score
raw/unbudgeted output against the source truth before scoring any Cortex view;
this is what distinguishes a Weavatrix losslessness defect from a Cortex
selection defect.

### Implementation replay

Repeat the `ArchiveOptions::disabled()` task against a clean pinned revision of
`weavatrix-search` with `qwen3.5:9b`, temperature zero, and a hidden test that
asserts all six fields. Run at least the external Weavatrix and Cortex arms.
Report context tokens, model prefill and generation tokens, compilation, and
hidden-test outcome. A syntactically plausible answer is not a pass.

### Full live comparison

Rerun all three existing live tasks with `qwen3.5:9b`. If the old harness cannot
be recovered exactly, reconstruct it from the documented task/oracle contract,
declare a new suite version, and never compare its aggregate as if it were the
same sample. The old live table remains historical in either case.

### MCP schema and payload

Launch real subprocess servers and measure initialization plus `tools/list`
schema cost, per-call wire bytes/tokens, and full session cost. Parse both MCP
`content` and `structuredContent`; record whether they are identical, distinct,
or single-representation, and count the representation a real client actually
delivers to the model. A missing response, malformed frame, or early stdin
close is a failed trial.

### Current Serena

Resolve Serena from its current package source at benchmark time, pin the exact
resolved version or commit into the manifest, and exercise it through its real
MCP subprocess. Documentation may display only the version captured by the
report. A manually typed competitor version is not admissible current evidence.

### Trial scheduling

Every competitive arm runs at least three times. The harness stores a
deterministic alternating schedule that changes which arm runs first and last;
at minimum it uses the natural order, reverse order, and a recorded cyclic
rotation. Setup and warm-up rules are identical per arm and explicitly recorded.
The scoreboard retains every raw sample and reports median plus range; it does
not select the best run.

## Failure handling

- A registry mismatch stops the upgrade until the package identity is
  resolved.
- An MCP timeout or malformed response is a benchmark failure, not a skipped
  sample.
- Any packet marked sufficient while missing a declared required fact is a
  false-confidence failure and must receive an owner classification.
- If repeated deterministic reports differ after metadata normalization,
  retain every report and investigate before publishing a deterministic claim.
- A failure without a reproducible owner-specific defect packet remains
  `UNCLASSIFIED` and blocks the P0 scoreboard.
- Temporary benchmark worktrees must be removed after their exact paths are
  verified; no benchmark may modify the source fixture checkout.

## Verification and commit boundary

Before the final commit, run:

- the focused stale-pin contract test;
- report-manifest, alternating-order, failure-attribution, and scoreboard
  contract tests through a red-green cycle;
- real stdio MCP smoke tests for `weavatrix@1.7.0`;
- `cargo fmt --all -- --check`;
- `cargo test --workspace`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `npm.cmd --prefix ui run build`;
- `git diff --check`;
- a source-file length check enforcing the 500-line limit.

Commit only Cortex Loom files owned by this repository, under the configured
user identity, with no co-author trailer. Do not push unless explicitly asked.

## Success criteria

- The external MCP pin is `weavatrix@1.7.0` and a regression test guards it.
- All unchanged Rust dependencies are confirmed current rather than guessed.
- Cortex retains a preview-only refactor boundary.
- Every report contains observed engine versions, target commit, model
  parameters, and MCP format.
- The ten-task deterministic probe, three live tasks, implementation hidden
  test, symbol truth set, Git benchmark, schema/payload benchmark, and current
  Serena comparison have all run on the current stack.
- Every competitive arm has at least three samples in alternating order.
- Every older result is visibly historical.
- Every failed fact is classified as a Weavatrix, Cortex, model, or harness
  defect and has the required replay artifact; `UNCLASSIFIED` is zero.
- The final output is one reproducible scoreboard combining task quality,
  false confidence, tokens, calls, and latency without mixing npm-server
  effects with the already-upgraded embedded engine.
- All repository quality gates pass and the work is committed without
  co-authorship metadata.
