# Roadmap

Product claim: Cortex Loom compiles a task-complete, revision-bound
evidence packet and proves which required facts are present, missing,
contradictory, or stale. Compression is a consequence, not the product.

## Stage 1 — remove false confidence

| Item | Status |
|---|---|
| Temporal memory scoped to an explicit completed `runId` | done |
| Callers cannot mint `Verified` on `route_work` or `context_compile` | done |
| Benchmark mechanism labels stay out of the generic engine | done |
| Snapshot ID, source spans, content-addressed evidence IDs | done |
| Model-specific `TokenCounter` | done |
| Semantic ranking requires a matching calibration artifact | done |
| `cleanRun` vs oracle-backed `qualityEquivalent` | done |
| Non-loopback bind forbidden unless `--allow-remote` | done |

## Stage 2 — cheap agent path

| Item | Status |
|---|---|
| `ServerProfile::Agent` default | done |
| Lean generated adapters (`cortex_prepare` / `cortex_expand`) | done |
| `usage_report` out of the agent workflow | done |
| Routing + sequence hint inside `prepare` | done |
| Adaptive budget (not a fixed 4 000) | done (`auto` default; pins remain) |
| Embeddings and digests cached by revision | done (`snapshot:content-hash`) |

## Stage 3 — beyond the Rust benches

Unicode/mixed-language intent, language inventory, graph-span
definitions, TS/JS/Python/Go/Java fixtures, and task-aware ui/test
ranking are in. External repos are catalogued; hidden issue/PR oracles
are reserved.

## Stage 4 — prove the work is better

Release evaluation must compare native, native + files, raw Weavatrix,
Cortex, and Serena/LSP on identical models, prompts, commits, oracles,
and retry budgets. See [evaluation.md](evaluation.md). The contract
lives in `cortex-bench` `release.rs`. Serena is unavailable unless
`CORTEX_SERENA_ROOT` is set.
