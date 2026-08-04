# Roadmap

## Working MVP

- Canonical typed process graphs with optimistic persistence and history.
- Editable React/SVG workflow UI with multiple graph documents.
- `SKILL.md` import, visual editing, export, and explicit save.
- Bounded MCP tools for graph read/list/write, skill compile/export, routing, one-step Weavatrix evidence compilation, and Refactor preview.
- Deterministic evidence selection with stable IDs, priorities, token budgets, omission reporting, and fail-closed critical evidence.
- Explicit local-small, local-medium, and upstream-strong routing tiers.
- Structured Ollama drafts with exact model tags, schema/evidence validation, CPU/GPU reporting, and no hidden model download or downgrade.
- Native Weavatrix graph evidence and a separate preview-only bridge to Weavatrix Refactor.
- Durable run snapshots with ready/running/succeeded/failed/skipped/cancelled node state, deterministic edge transitions, optimistic commands, event history, HTTP/MCP controls, and SVG overlays.
- Immutable attempt-scoped evidence, audited human/review decisions, graph-configured bounded retries, and read-only deterministic replay verification.
- Offline model-profile benchmark and calibration harness (`cortex-eval`): typed fixtures for classification/extraction/compression, pure comparators, pinned prompt/schema versions, fail-closed verdicts, and explicit `model_absent` skips instead of hidden pulls.

## Next implementation milestones

1. ~~Extend durable runs with evidence invalidation, executor leases, lease expiry, and explicit external-executor identity.~~ Done: typed `ExecutorIdentity`, `claim_lease`/`release_lease` commands with bounded TTL and lazy replay-deterministic expiry, lease enforcement on node commands, retry/completion lease clearing, and `invalidate_evidence` that blocks future citations while keeping the audit record. UI surfaces for leases remain future work.
2. ~~Add graph-to-agent adapters for Codex, Claude Code, and Copilot while keeping the canonical graph vendor-neutral.~~ Done (iteration 1): `cortex-adapters` renders preview-only vendor wiring (skill instructions plus MCP registration) from one canonical graph via the `adapter_export` MCP tool and `GET /api/adapters/{agent}`. Remaining: run-node execution by external agents stays gated on milestone 1 leases and executor identity.
3. ~~Wire evaluated Ollama profiles into the MCP host behind explicit runtime configuration and shadow-mode metrics.~~ Done (iteration 1): `cortex-shadow` observes `route_work` and `weavatrix_context_compile` behind `CORTEX_SHADOW=1`, with append-only samples, `shadow_metrics_read`, and `/api/shadow/*`; see [shadow-mode.md](shadow-mode.md). Remaining: promotion criteria stay gated on calibration verdicts and shadow agreement data.
4. ~~Add embeddings and retrieval evaluation before permitting semantic evidence selection.~~ Done across three iterations: bounded `/api/embed`; repository-specific Recall@k/nDCG fixtures with structural relatedness; three pinned ranking modes where `hybrid_graph` (RRF of embedding + BM25 plus a rank-space graph boost) passes the gate on both installed models (`embeddinggemma` 0.96/0.92, `qwen3-embedding:0.6b` 1.00/0.96); and gated production wiring — `CORTEX_SEMANTIC=1` + model tag enables within-band semantic ordering inside the deterministic compiler, with packet provenance (`semanticRanking`) and deterministic fallback plus a recorded warning on any failure. The ranking module lives in `cortex-context` and is shared by the harness and production. Remaining ideas: adjacency from live Weavatrix neighborhoods instead of split-parent grouping, and semantic scores in the shadow compression comparison.
5. Import a small, licensed Superpowers-derived methodology fixture set and build scenario/pressure tests for round-trip behavior.
6. Stabilize or publish native Rust Refactor planning crates before removing the JavaScript compatibility oracle.
7. Run official MCP conformance and adversarial stdio tests before treating the server as production-ready. Streamable HTTP transport shipped (`cortex-mcp --http 127.0.0.1:43818` or `CORTEX_MCP_HTTP`): one HTTP session bridges to one in-process MCP loop over bounded channel pipes, with `Mcp-Session-Id` issuance on initialize, protocol-version header validation, loopback Origin enforcement, bounded sessions with idle expiry, DELETE termination, and 405 on GET (no server-initiated streams). The official suite passes against the committed baseline (`config/mcp-conformance-baseline.yaml`): server-initialize, ping, tools-list, SSE behavior, and both DNS-rebinding checks pass; the remaining entries are suite fixture tools and capabilities (resources/prompts/logging/completion) this tools-only server does not declare. Remaining: adversarial stdio tests; resources/prompts support if ever needed.
8. ~~Add run-level token, latency, device, rejection, fallback, and quality-equivalence telemetry.~~ Done: the append-only usage ledger records every `route_work` decision and `weavatrix_context_compile` savings figure with optional `runId` attribution; the quality summary joins savings with run outcomes so only clean succeeded runs are credited; and executors close the balance by self-reporting upstream consumption via `usage_report` (MCP) or `POST /api/usage/reports` — the adapter usage contract instructs agents to do both and to default `maxTokens` to the measured 4000. Reports are honest self-reporting, not billing verification.

## Research gates

- Compare `qwen3.5:4b`, `phi4-mini`, and a medium local profile on extraction, classification, and citation-preserving compression. Closed on this device under pinned `eval-prompts-v3` with role-aware verdicts: `qwen3.5:4b` passes the `local_small` role (classification 0.82, zero missed escalations, extraction exact-match 0.80) and `qwen3.5:9b` passes `local_medium` (perfect citation preservation). `phi4-mini` remains below both small gates. Dogfood note: shadow compression of a real 7.5k-token packet timed out on CPU — synthetic-fixture latency does not transfer to production payload sizes.
- Do not claim NPU execution until an OpenVINO GenAI or Foundry Local adapter reports and passes calibration on the actual device.
- Establish repository-specific Recall@k, nDCG, classification F1, unsupported-claim rate, and zero-missed-escalation fixtures. Recall@k/nDCG fixtures exist and are measured (`--suite retrieval`); classification and escalation fixtures were established earlier; unsupported-claim rate remains open.
- Measure end-to-end upstream token savings only on quality-equivalent accepted outcomes.

The “90% less upstream work” target remains a hypothesis until those gates pass.
