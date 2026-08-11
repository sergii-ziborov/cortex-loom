# Weavatrix 1.7 upgrade and Cortex rebaseline

Date: 2026-08-11

## Decision

Cortex Loom will pin its external Weavatrix MCP configuration to
`weavatrix@1.7.0` and remeasure the quality and cost boundaries that changed in
the 1.7 release. The embedded repository-intelligence engine remains
`weavatrix-rust 2.5.0`, which is already current.

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
3. Audit Cortex adapter workarounds against the 2.5 engine contracts. Remove a
   workaround only when a failing regression test proves the new engine owns
   the same behavior; otherwise leave it in place.
4. Update benchmark documentation with versions, commands, report paths, and
   new results. Historical rows remain labelled historical rather than being
   overwritten.
5. Do not add a refactor apply tool, confirmation token, JavaScript planner,
   or write-capable profile.

## Measurement design

### Embedded Cortex retrieval

Run the ten-task, 4,000-token deterministic probe twice with distinct output
paths and the same stamp. The two JSON reports must be byte-identical. Report
both selected and delivered tokens, anchor recall, and false-sufficiency
behavior for every arm.

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
and elapsed time. Keep CLI comparisons on the same repository revision.

### Implementation replay

Repeat the `ArchiveOptions::disabled()` task against a clean pinned revision of
`weavatrix-search` with `qwen3.5:9b`, temperature zero, and a hidden test that
asserts all six fields. Run at least the external Weavatrix and Cortex arms.
Report context tokens, model prefill and generation tokens, compilation, and
hidden-test outcome. A syntactically plausible answer is not a pass.

### Full live comparison

Rerun the existing three-task live suite only if its exact harness and pinned
fixture revision can be recovered. If they cannot be recovered, do not
reconstruct a look-alike and call it comparable; report that the old live table
remains historical and use the deterministic, symbol, Git, and implementation
results as the current rebaseline.

## Failure handling

- A registry mismatch stops the upgrade until the package identity is
  resolved.
- An MCP timeout or malformed response is a benchmark failure, not a skipped
  sample.
- Any packet marked sufficient while missing a declared required fact is a
  false-sufficiency failure.
- If repeated benchmark reports differ, retain both and investigate before
  publishing a deterministic claim.
- Temporary benchmark worktrees must be removed after their exact paths are
  verified; no benchmark may modify the source fixture checkout.

## Verification and commit boundary

Before the final commit, run:

- the focused stale-pin contract test;
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
- The deterministic probe is reproducible and has no quality regression from
  the `final-quality-2026-08-11` baseline.
- Current symbol, Git, and implementation results are reported without mixing
  npm-server effects with the already-upgraded embedded engine.
- All repository quality gates pass and the work is committed without
  co-authorship metadata.
