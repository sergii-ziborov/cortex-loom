# Cortex Loom coding-token audit — 2026-10-03

## Result

The requested 2–3× saving in **complete coding-task tokens** is not yet
reproduced on this host. In the first matched Sol pair, Cortex saved **26.8%
of input plus output tokens** (1.37×) and **6.5% of uncached input plus output**
(1.07×). The second pair saved **37.7% gross** (1.61×) but no uncached tokens.
Both agents made the requested code changes correctly in both pairs. These
are two tasks, not a population estimate or a Sonnet/Opus result.

| Same code change, Sol medium; installed UI and warm Cargo cache in both lanes | Control | Cortex MCP | Cortex / control |
| --- | ---: | ---: | ---: |
| Input + output, including cached input | 544,741 | 398,755 | 0.732 |
| Input minus cached input + output | 36,325 | 33,955 | 0.935 |
| Shell commands | 18 | 15 | 0.833 |
| MCP calls | 0 | 1 | — |
| Shell result characters | 76,443 | 26,516 | 0.347 |
| MCP result characters | 0 | 27,106 | — |

Both lanes started at `d025afab5b8d85ef7689d05c2df65436e416f991`,
received the same `Elevated` coding task, used `gpt-6-sol` at medium effort,
the same installed UI dependencies and Cargo cache, and the same Codex CLI
settings. The Cortex lane additionally received the MCP call instruction and
an agent-profile MCP server. The Sol agent wrote correct code in both lanes;
focused Rust tests, formatting, and the UI build passed. The broader MCP test
suite encountered the same sandbox HTTP-port failure. Raw provider usage:
`/tmp/cortex-code-ab-control-fair.jsonl` and
`/tmp/cortex-code-ab-packet.jsonl`. The Cortex packet carried 8,902 estimated
tokens, including the owning test suite from line 1 and all five requested
source surfaces.
The machine-readable totals are in
[`cortex-code-edit-paired-2026-10-03.json`](cortex-code-edit-paired-2026-10-03.json),
produced from the CLI's completed-turn usage records by
`benchmarks/coding-agents/collect_codex_usage.py`.

The second task fixed a real `PacketStore` bug: arbitrary `HashMap` eviction
became oldest-first eviction with replacement refreshing recency. Both lanes
started from the same clean revision, used Sol medium and the same shared
Cargo target, and passed `cargo test -p cortex-mcp`, format, and affected
Clippy. Cortex used one `cortex_prepare` call; the verified patch was brought
back to the main checkout.

| PacketStore code edit, Sol medium | Control | Cortex MCP | Cortex / control |
| --- | ---: | ---: | ---: |
| Input + output, including cached input | 358,014 | 222,876 | 0.623 |
| Input minus cached input + output | 29,822 | 29,852 | 1.001 |
| Shell commands | 13 | 9 | 0.692 |
| MCP calls | 0 | 1 | — |
| Shell result characters | 27,726 | 10,488 | 0.378 |
| MCP result characters | 0 | 19,057 | — |

Raw receipts: `/tmp/cortex-packetstore-control.jsonl` and
`/tmp/cortex-packetstore-loom.jsonl`. Both worktrees were removed after the
diff review; only the main checkout remains.

Further one-off Cortex probes on that same task did **not** improve the
matched result. Adding manifest and test-module scaffolding increased gross
usage to 522,861 with 18 shell calls. Compacting search rows cut the packet
estimate to 6,465 tokens but raised the complete task to 725,894 gross and
43,782 uncached-plus-output tokens with 30 shell calls; the agent had to
rediscover omitted snippets. Both experiments were removed from the working
tree. Their traces are `/tmp/cortex-code-ab-scaffold.jsonl` and
`/tmp/cortex-code-ab-compact.jsonl`. A smaller packet is not a token saving
when it causes more coding-agent turns.

Direct MCP calls with `off`, `local`, and Composer/Haiku on the coding task
returned identical evidence bodies. In the latter two modes,
`internalModel.called=false` and `skipReason=lexical_floor_upstream_strong`:
the optional model is only a routing classifier and cannot change an already
upstream-strong code-edit route. The configured Qwen OVMS and Composer proxy
endpoints refused connections on this host, so these are **not live Qwen or
Haiku evaluations**.

A separate **local Qwen3 8B diagnostic** used the installed Ollama GPU model
with the same Elevated evidence packet. An unrestricted plan (5,766 prompt
tokens, 1,119 completion tokens, 111 seconds) invented a test budget that
would not omit the low-priority item, gave an invalid unit-test command, and
contradicted itself about whether checks ran. A constrained JSON checklist
(5,777 prompt, 615 completion tokens, 70 seconds) still proposed an
unsupported `lib.rs` edit and fictitious search-result paths. Neither draft
was fed to the coding agent or used as a savings claim. This GPU runtime is
not the configured OVMS/NPU Qwen lane.

## Earlier diagnostic

The earlier paired run below had unprepared UI dependencies in its control
environment and older Cortex retrieval. It found a regression but is not the
matched current result.

| Same code change, Sol medium | Control | Cortex MCP | Cortex / control |
| --- | ---: | ---: | ---: |
| Input + output, including cached input | 796,807 | 1,048,522 | 1.316 |
| Input minus cached input + output | 37,639 | 50,250 | 1.335 |
| Shell commands | 19 | 31 | 1.63 |
| MCP calls | 0 | 1 | — |
| Shell result characters | 77,952 | 75,955 | 0.974 |
| MCP result characters | 0 | 9,189 | — |

The task added `Elevated` **between High and Normal** in the evidence enum,
rank, snake_case serialization, MCP `context_compile` schema, tests, and UI
help, while preserving Critical's fail-closed behavior. Both lanes started
from `d025afab5b8d85ef7689d05c2df65436e416f991` in fresh clones, used
`gpt-6-sol` at medium effort, and completed the same task. Both passed
`cargo fmt --all -- --check`, the UI build, and the focused context/MCP tests
when an unrelated HTTP socket test was excluded. The full MCP suite failed on
that socket test in both isolated lanes. Raw usage and edits:
`/tmp/cortex-code-ab-fixed/{control,cortex_mcp}.jsonl` and the sibling clones.

The Cortex agent received a usable 2,400-token packet with exact source
windows for the enum, UI help, MCP schema, and Critical error. It still
reopened those files, searched again, and ran 12 more shell commands than
control. The packet reduced shell result text by only 1,997 characters, too
little to offset the MCP payload and extra turns. That earlier packet was
`coverage.sufficient: true`, with no missing facets; this rules out the
earlier false-definition retry as the sole cause of the current loss.

A follow-up **wide-packet diagnostic** on the same base revision and task
requested `maxTokens: 10000` and used the final schema-key ranking and usage
guidance. It completed correctly and passed focused Rust tests (excluding the
same socket test), format, and UI build. Gross usage was 605,712, **24.0%
below** the earlier control, but uncached input plus output was 40,336,
**7.2% above** control. It still made 30 shell calls and reopened all four
cited files. The packet itself delivered 9,927 estimated tokens. This was a
later single run with both prompt and implementation changes, so it cannot
isolate the effect of packet size. It also remains far short of 2–3×. Raw
trace: `/tmp/cortex-code-ab-wide/cortex_mcp.jsonl`.

## Confirmed causes and changes

1. **The old 809,468 versus 40,403 figure mixed metrics.** The collector
   labelled peak context plus final response as full-task spend while the
   other lane used cumulative input. The collector now sums complete turns
   and preserves the old field under a legacy name. The archived provider
   receipts are absent, so that historical ratio cannot be recalculated.
2. **The tested local-model path did not compress evidence.** These runs set
   `CORTEX_LLM_BACKEND=off`; the configured OVMS endpoint refused a connection.
   In current code, Qwen's `internalModel.role` is `routing_classifier` and
   `affectsEvidence` is false. Even a live Qwen classifier only changes the
   routing tier; it does not draft a patch or reduce the source packet. The
   separate `cortex-ollama::draft` API is not called by the MCP agent profile;
   its current route gate also refuses upstream coding tasks. An advisory
   coding draft needs a separate contract that keeps execution upstream and
   validates claim content, not merely citation IDs.
3. **The Superpowers-derived scripts were not active.** The `agent` MCP
   profile exposes `cortex_prepare` and `cortex_expand`. Its `workflowStep`
   is a recommendation with `active:false`; no sequence step was executed in
   this A/B. Savings from an active workflow cannot be attributed to this
   lane.
4. **Weavatrix search could erase useful results.** A long first matching
   source line exhausted a bounded `search_code` response, and the tail trim
   removed all short hits. `weavatrix-rust` 2.17.5 clips each line around its
   first match and rebases spans; the release passed its feature, lint,
   architecture, and publish checks. Cortex now depends on 2.17.5. Cortex
   also stopped using broad positive globs that scanned ignored UI dependencies
   before product files. Release:
   https://github.com/Weavatrix/weavatrix-rust/releases/tag/v2.17.5
5. **Creation-task planning spent evidence on the wrong targets.** A request
   to add a new feature could be treated as test selection because it said
   “run tests”; `snake_case` was mistaken for a source identifier; the MCP
   tool label `context_compile` triggered a nonexistent definition lookup.
   The planner now uses task-domain searches for new features, ignores an
   unquoted naming-convention label, reads distinct owning files, and keeps
   requested MCP/UI source windows. It does not demand an existing definition
   for a new feature. A generic MCP schema property no longer outranks the
   specific field named by the task.
6. **The Refactor packages did not cause this A/B cost.** The agent profile
   has no `weavatrix_refactor_preview` tool, and neither lane invoked
   `weavatrix-refactor-plan` or `weavatrix-edit`. Full-profile previews can
   still carry full before/after bodies up to 64 KiB each; a compact diff
   option is a separate improvement. The preview/confirmation boundary must
   remain intact.
7. **A named type's short definition could hide the implementation.** In the
   PacketStore pre-fix probe, Cortex returned just the three-line struct,
   tried a nonexistent sibling `tests.rs`, spent follow-up windows in
   unrelated `cortex-context` code, and falsely reported sufficient test
   coverage from a file that merely mentioned `PacketStore`. The gatherer
   now opens the named owner's source from line 1 for code edits, probes a
   sibling test file only if it exists, and requires an actual test body for
   the test certificate. The post-fix probe included `insert`, the stale
   check, and embedded unit tests; it made no missing-file call. A separate
   locator fix uses the actual read range, so the long definition is cited
   as lines 31–106 instead of the misleading three-line graph span.

## Remaining product gap

Cortex currently saves some *retrieval* work but does not replace enough
coding-agent turns to meet a 2–3× full-task target. The next capability must
produce a concise, cited implementation or test plan, or a locally drafted
patch preview, that the upstream agent can verify and apply. It should be
gated on source completeness and measured against full-task provider usage.
For high-risk or unverified work, retain the upstream agent and its review;
never auto-apply Refactor changes. A calibrated Qwen draft path, plus repeated
Sonnet, Opus, and Sol code-edit pairs with identical quality gates, is needed
before claiming the target.

The current packet starts at the owning test-file head, includes all five
requested code surfaces, and reports `coverage.sufficient: true` only when
the relevant test source is present. The agent's plain MCP response carries
unescaped exact source with path and line locators. A classifier is skipped
when the lexical route is already `upstream_strong`, eliminating a model call
that could not change the decision. The final Cortex checkout passed
`cargo fmt --all -- --check`, `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, the UI build, and
the Python benchmark tests. All Rust source files remain below 500 lines.
