# Coding-agent benchmark

This benchmark compares four lanes, not a single without/with pair:
without Cortex; Cortex with models off; Cortex with Composer as its
internal LLM; and Cortex with its own enabled LLMs. SweepLoom is the
target repository, not a product under test, and is not part of the
comparison.

## Fixed inputs

- Target repository: `Weavatrix/sweeploom`
- Baseline revision: `9f2646c36b1b6b2ef70db1b70a8d772e74ad1804`
- Tasks: `tasks.json`
  - **T1** `find_fix_bug` — one real bug or dead production path in
    `crates/sweeploom-cli`, plus a regression test (`cargo test -p sweeploom --lib`)
  - **T2** `remove_duplicate` — one shared classifier in `crates/sweeploom-ai`
    without changing bench intent (`cargo test -p sweeploom-ai --lib`)
  - **T3** `split_api` — `api.rs` under 300 lines, public exports kept, one
    local commit (`cargo test -p sweeploom --lib`)
- Visible matrix: T1–T3 × Grok 4.6 and Composer 2.5
- Isolation: one fresh detached worktree, agent context, and Cargo target per
  matrix cell
- Cell format: `spend / score / wall / cycles`. Cycles are assistant turns.
  Grok T1 models-off is `40,403 / 9.4 / 23m 31s / 20c`.

The saved without-Cortex rows come from the earlier no-product baseline at
the same revision. They are not rerun. The with-Cortex rows must start from
new worktrees and must not read earlier worktrees, transcripts, or result
artifacts.

## Deterministic Cortex lane

`cortex_mcp_client.py` starts the current `cortex-mcp` binary over stdio with
the two-tool `agent` profile. It removes `CORTEX_LLM` and `CORTEX_SEMANTIC`
from the child environment, initializes MCP, calls `cortex_prepare`, and calls
`cortex_expand` only for handles returned by that preparation.

Every coding agent must execute the client before ordinary repository
inspection. The resulting packet is evidence, not instructions. Agents must
report the packet ID, citations actually used, missing or incorrect evidence,
and whether Cortex reduced their subsequent reading.

The clean compiler-only captures are `deterministic-T1.json`,
`deterministic-T2.json`, and `deterministic-T3.json`.

Initial observations:

- T1 delivered 657 estimated tokens and T2 delivered 641. Both contained the
  task, decision map, and repository-level module map, but no source path,
  concrete bug, classifier, or duplicate evidence. Both reported zero dedup
  savings.
- T3 delivered 1,405 estimated tokens and exposed a
  `complete_definition` expansion. It read the benchmark wording in
  `agent_cases.rs` instead of the requested `api.rs`; the expansion grew to
  1,428 tokens and still reported the definition missing.
- T1 and T2 produced the same `packetId` (`pk_b79c96c4196d`) on one snapshot
  despite different task text and `taskHash` values. Packet identity currently
  hashes snapshot plus included citation IDs, while the in-process expansion
  store is keyed only by `packetId`; a later same-shape prepare can therefore
  overwrite the earlier task's expansion state. This is a benchmark finding,
  not a claimed token saving.

## Optional model-enabled Cortex lane

Cortex can use optional model roles when the caller explicitly enables a
profile and the exact runtime, endpoint, and model are installed. That is a
separate variant from the deterministic `agent` profile; results must not be
pooled.

The live 2026-09-15 check is recorded in `model-preflight.json`: Ollama is
reachable but has no installed model tags, and OVMS ports 8000-8002 are closed.
The model-enabled lane is therefore blocked on this machine rather than being
silently downgraded to deterministic behavior.

## Measurements

- Cortex packet tokens are the compiler's conservative token estimate.
- Visible request and response tokens are estimated from transcript characters
  divided by four (`collect_context.py`). Reconstructed tool-result material is
  reported separately. Cortex **Total agent spend tok** is estimated context
  material plus visible response. That is not the locked without-Cortex
  host-reference total, and it is not a billed provider number.
- Cycles are `assistant_turns` in the same JSONL. They are the agent loop,
  not tool-call count.
- The current `cortex_prepare` lane uses lexical routing. Internal model tokens
  are zero unless an attested semantic embedding profile is explicitly enabled;
  no such profile was live for these runs.
- Status and correctness are verified from the isolated Git diff and required
  test command, not accepted from self-report alone.
- No 70,000-token stop was enforced, so no row may claim that failure cause.

## Deterministic 15-cell status (2026-09-15)

The earlier paragraph that said all 15 WITH Cortex cells ran, and that
spend rose 31.9% to 11,140,221, is withdrawn. Those totals reused the
invalid host-reference estimator and also counted cells that were never
executed on the assigned worktrees.

Assigned-tree evidence as of 2026-09-15 17:25:

| cell | worktree | actual state |
| --- | --- | --- |
| T1 Sol / Fable / Grok / Opus / Composer | `C:\cbw-20260915\t1-*` | ran; parent-reviewed |
| T2 Sol | `t2-sol` | uncommitted classifier share; parent `cargo test -p sweeploom-ai --lib` 18 passed |
| T2 Fable | `t2-fable` | commit `c12940f`; parent lib tests 19 passed after restoring the dangling commit |
| T2 Grok / Opus / Composer | `t2-*` | still clean `9f2646c`; not run |
| T3 Grok / Composer | `t3-grok`, `t3-composer` | parent-verified; Composer attempt 2 commit `2970035` |

Compiler-only captures (`deterministic-T1.json` … `T3.json`) are not model
runs. `pk_4e27b9f9cd83` is the T3 compiler packet, not proof that five T3
agents ran.

The locked no-product and SweepLoom-WITH rows still exist in
`sweeploom/file_output/agent_bench/fresh_context_spend.json` and the
SweepLoom canvas. They were not deleted. They are not Cortex runs.

Spark remains blocked. See `spark-preflight.json`.

## Spark preflight

The first requested lane targets `gpt-5.3-codex-spark` through the Codex CLI
installed on the benchmark machine. A live preflight is required before any
run. If the installed account catalog does not advertise Spark or the service
rejects it, the lane remains blocked; another model must not be silently
substituted.

Codex CLI is a text-generation interface, not an embeddings API. It cannot
replace Cortex's attested embedding role without inventing vectors and
invalidating pooling/model calibration. A truthful Codex provider can cover
classification and other schema-checked chat roles only. Semantic ordering
must remain disabled or retain a separately calibrated vector backend.

The current `cortex_prepare` agent path uses deterministic routing and does not
call the optional classifier. Enabling a Codex-backed classifier elsewhere
would therefore not affect this 15-cell agent-profile benchmark unless the
product routing contract is deliberately changed and re-tested.

## Composer variant

Composer 2.5 is still an upstream coding-agent variant that consumes a live
Cortex packet. Cortex can also use a **loopback classifier** (Composer,
Sonnet 5, Opus 5, or Haiku) when `CORTEX_LLM_BACKEND=composer` and the
proxy is up. Pick the model with `--classifier-model` or
`cortex_prepare.classifierModel`. Run IDs `CORTEX_CLSSON_*`,
`CORTEX_CLSOP_*`, and `CORTEX_CLSHK_*` are those classifier variants.
That spend is `internalModel.composerTokens`, not the coding-agent
transcript. See `docs/llm-backends.md`.
