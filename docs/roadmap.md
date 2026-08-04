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
4. Add embeddings and retrieval evaluation before permitting semantic evidence selection.
5. Import a small, licensed Superpowers-derived methodology fixture set and build scenario/pressure tests for round-trip behavior.
6. Stabilize or publish native Rust Refactor planning crates before removing the JavaScript compatibility oracle.
7. Run official MCP conformance and adversarial stdio tests before treating the server as production-ready. Verified prerequisite (2026-08-04): the official suite (`@modelcontextprotocol/conformance` 0.1.16) drives servers over Streamable HTTP only, so cortex-mcp first needs an HTTP transport (or a stdio bridge) before the suite can run.
8. Add run-level token, latency, device, rejection, fallback, and quality-equivalence telemetry. Partially done: an append-only usage ledger (`usage_samples`) records every `route_work` decision and `weavatrix_context_compile` savings figure, readable via the `usage_read` MCP tool and `GET /api/usage/*`. Remaining: run-scoped attribution, upstream-side token counts, and quality-equivalence verdicts.

## Research gates

- Compare `qwen3.5:4b`, `phi4-mini`, and a medium local profile on extraction, classification, and citation-preserving compression. Closed on this device under pinned `eval-prompts-v3` with role-aware verdicts: `qwen3.5:4b` passes the `local_small` role (classification 0.82, zero missed escalations, extraction exact-match 0.80) and `qwen3.5:9b` passes `local_medium` (perfect citation preservation). `phi4-mini` remains below both small gates. Dogfood note: shadow compression of a real 7.5k-token packet timed out on CPU — synthetic-fixture latency does not transfer to production payload sizes.
- Do not claim NPU execution until an OpenVINO GenAI or Foundry Local adapter reports and passes calibration on the actual device.
- Establish repository-specific Recall@k, nDCG, classification F1, unsupported-claim rate, and zero-missed-escalation fixtures.
- Measure end-to-end upstream token savings only on quality-equivalent accepted outcomes.

The “90% less upstream work” target remains a hypothesis until those gates pass.
