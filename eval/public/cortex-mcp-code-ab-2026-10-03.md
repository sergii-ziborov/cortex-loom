# Cortex Loom MCP: paired coding-task pilot (2026-10-03)

This replaces the inference from `cortex-mcp-agent-ab-2026-10-03.json`, which is disqualified for the requested comparison because half its tasks were read-only and its only coding task was a narrow local edit. Cortex Loom is the MCP product under test. The historical `benchmarks/coding-agents` experiment uses SweepLoom only as the codebase being changed; its rows are a separate experiment.

**Scope correction:** this pilot explicitly set `CORTEX_LLM_BACKEND=off`, so it did not exercise the optional Qwen classifier or embedding profile. It must not be used to evaluate a Qwen-enabled Cortex configuration. A local-backend preflight on this host failed with `Connection refused` at the configured OVMS endpoint; no local Qwen result is available here. The classifier's current product role is routing only, so even a live Qwen classifier does not itself select or compress the evidence packet.

The repository's older Grok T1 figures, 809,468 Without and 40,403 models-off, are sometimes presented as a large saving. The Without host-reference total and Cortex transcript estimate are different metrics. More specifically, the old collector computed cumulative input over turns but then discarded it and labeled peak context plus response as `total_agent_spend_tok`; 40,403 is therefore **not** full-task spend. The archived field is now named `legacy_peak_context_plus_response_tok`, and the collector sums estimated input across turns for future runs. The raw historical transcripts and baseline receipt are not in this checkout, so the full-task ratio cannot be recovered here. This says nothing about a separate provider-reported 800k/80k pair if its logs exist.

For scale, a context that reaches 80k tokens by the last of 20 turns can account for roughly 800k cumulative input tokens if it grows steadily and is reread on every turn. Cache, truncation, and tool-result delivery change the real provider total; only its usage receipt can settle that total.

## Setup and measurement

- Base revision: `d025afab5b8d85ef7689d05c2df65436e416f991` in four fresh local clones of Cortex Loom.
- Upstream agent: `gpt-6-sol`, medium reasoning, Codex CLI `0.158.0-alpha.2.1`. One run per task and lane. User configuration was ignored in both lanes. The Cortex lane had the repository's `cortex-context` usage guidance in its prompt, an `agent`-profile MCP server, a fresh database, and internal models off. The control lane had the same coding task without Cortex. Both lanes could edit and test.
- Complete task usage is the CLI's final `turn.completed` record. `gross = input_tokens + output_tokens` includes cached input. `fresh+output = input_tokens - cached_input_tokens + output_tokens` is an additional view, not a billed cost. Both include the whole coding session, including tool calls, edits, tests, and final response. Wall time is omitted because concurrent runs shared a Cargo cache.
- The MCP prompt includes the Cortex usage guidance; that overhead is part of this integrated lane. It does not mandate an MCP call for a local edit. Agents chose `budgetClass: normal`; direct baseline probes with `auto` produced the same selected evidence and certificate for these exact requests.

| Code change | Control gross | Cortex gross | Control fresh+output | Cortex fresh+output | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| Correct evidence completeness for caller and MCP/HTTP impact | 1,522,278 | 2,597,184 (+70.6%) | 64,742 | 89,664 (+38.5%) | Both produced fixes that passed focused tests and an end-to-end certificate probe. |
| Add `Elevated` priority across compiler, MCP schema, tests, and UI help | 622,052 | 812,600 (+30.6%) | 39,652 | 46,904 (+18.3%) | Both passed context/MCP tests, format check, and UI build. |

These are two deliberately chosen changes in Cortex's own evidence path. They do **not** estimate savings across general coding work, Sonnet, Opus, or the user's previous tasks. There are no repeats or confidence intervals. The result does not disprove the user's reported 2–3× saving.

## What happened

**Caller and transport impact.** The baseline `cortex_prepare` classified the task as `runtime_config`, returned only `runtime_config` and `target.complete_definition` as required facets, and declared `sufficient: true` without caller or endpoint evidence. The Cortex agent had to inspect the source anyway. It made 49 shell calls versus 43 in control; shell results grew from 168,079 to 175,705 characters, plus 14,297 characters of MCP result. A clean rebuild of each solution followed by the same MCP request showed both fixes now require `direct_callers` and `public_api_effect` and correctly report them missing when the packet lacks that evidence. This validates the specific fix, not general impact coverage for every phrasing.

**New priority.** The Cortex agent called `cortex_prepare` and then `cortex_expand` for `target.complete_definition`. `Elevated` was a new symbol, so neither packet could provide its definition. The two MCP responses added 21,135 JSON characters. Shell result characters fell only from 79,029 to 74,932; both agents made 17 shell calls. The packet therefore did not replace enough code reading to pay for itself. The fixes in both lanes include the enum rank, snake_case wire value, schema entry, budget tests, and UI help. That synthetic priority change was not applied to the main checkout.

The shared Cargo `target` in the agent runs caused an independent binary check to reuse a stale artifact. Token measurements are unaffected, but that first check was invalid. I then cleaned the changed packages separately for each clone, reran the relevant Rust tests, and rebuilt distinct binaries for the end-to-end certificate probes. All four final code variants passed those isolated tests and `cargo fmt --all -- --check`; the two priority variants also passed `npm --prefix ui run build` using the main checkout's installed UI dependencies. Changed source files remained below 500 lines. These are focused checks, not a full workspace release gate.

## Product implications

1. Coverage requirements must represent every requested surface before `sufficient` can be true. In particular, a code-change impact question that names callers and MCP/HTTP entry points needs caller and public API evidence even if the primary intent is misclassified or hinted as runtime configuration.
2. Creation tasks need a different retrieval route from existing-symbol questions. For a new variant such as `Elevated`, gather the containing enum, rank logic, serialization, schemas, and tests. `cortex_expand` should not repeat a failed definition lookup for a symbol that does not exist yet.
3. Benchmark by completed code change, with the same base revision, exact prompt, model, effort, quality gate, and provider usage metric in each paired lane. Record both cached and fresh tokens, MCP payload size, subsequent source reads, and whether the packet actually replaced them. Use multiple tasks and repeated runs before claiming a general saving.

After this pilot, the caller and transport coverage fix was adapted into the main working tree. The original call-chain prompt now requires both `direct_callers` and `public_api_effect` and reports an incomplete certificate when those facts are missing. This source change is uncommitted; the full workspace test, Clippy, format check, and UI build passed on the working tree.
