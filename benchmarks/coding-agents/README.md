# Coding-agent benchmark

This benchmark compares the saved SweepLoom coding-agent baseline with a live
Cortex Loom evidence lane.

## Fixed inputs

- Target repository: `Weavatrix/sweeploom`
- Baseline revision: `9f2646c36b1b6b2ef70db1b70a8d772e74ad1804`
- Tasks: `tasks.json`
- Matrix: three tasks by five upstream coding models
- Isolation: one fresh detached worktree, agent context, and Cargo target per
  matrix cell

The saved `WITHOUT` rows come from the earlier baseline at the same revision.
They are not rerun. The `WITH` rows must start from new worktrees and must not
read earlier worktrees, transcripts, or result artifacts.

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
  divided by four. Reconstructed tool-result material is reported separately.
  No cumulative provider-spend number is claimed: Cursor JSONL omits billing
  telemetry, and repeated context rereads are not summed with a fabricated
  fixed host cost.
- The current `cortex_prepare` lane uses lexical routing. Internal model tokens
  are zero unless an attested semantic embedding profile is explicitly enabled;
  no such profile was live for these runs.
- Status and correctness are verified from the isolated Git diff and required
  test command, not accepted from self-report alone.
- No 70,000-token stop was enforced, so no row may claim that failure cause.

## Deterministic 15-cell result (2026-09-15)

All 15 WITH cells ran. Review used the worktree diff and the required
`cargo test` command. Self-report was not accepted. Canvas comparison
reuses the saved WITHOUT host-reference method so the A/B stays on one
scale.

| lane | passed | failed | estimated spend | peak sum |
| --- | ---: | ---: | ---: | ---: |
| saved WITHOUT | 15 | 0 | 8,443,701 | 606,381 |
| deterministic Cortex | 11 | 4 | 11,140,221 | 663,105 |

Spend rose 31.9%. Peak rose 9.4%. Failures were T1 Composer (test-only
helper) and T2 Fable / Grok / Composer (WITHOUT classifier merged into
the fuller legacy snapshot). Green tests did not save those rows.

Compiler findings from the live packets:

- T1/T2 often certified `sufficient: true` while source search returned
  no file paths, so `--expand-missing` expanded nothing.
- T3 correctly reported insufficient, then treated
  `crates/sweeploom-cli/src/api.rs` as a symbol. Expansion cited
  `agent_cases.rs` and never delivered `api.rs`.
- All five T3 models received the same packet id `pk_4e27b9f9cd83`.

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

Composer 2.5 is an additional requested coding-agent variant that consumes a
live Cortex packet. It is not a replacement for the blocked Spark variant and
is not represented as an internal Cortex model role: Cursor exposes Composer as
an upstream coding agent, not as the embedding or schema-chat API expected by a
model profile.
