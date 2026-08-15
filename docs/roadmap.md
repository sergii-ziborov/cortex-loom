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
| Embeddings and digests cached by revision | partial (content-hash cache; not revision-keyed) |

## Stage 3 — beyond the Rust benches

Unicode/mixed-language intent is started. Dynamic language inventory,
AST definition spans, TS/JS/Python/Go/Java suites, no global `ui/`
downrank, 20–50 external repositories, and hidden issue/PR tasks are
not built.

## Stage 4 — prove the work is better

Release evaluation must compare native, native + files, raw Weavatrix,
Cortex, and Serena/LSP on identical models, prompts, commits, oracles,
and retry budgets. See [evaluation.md](evaluation.md). The harness does
not yet run that comparison.
