# Roadmap

## Working MVP

- Canonical typed process graphs with optimistic persistence and history.
- Editable React/SVG workflow UI with multiple graph documents.
- `SKILL.md` import, visual editing, export, and explicit save.
- Bounded MCP tools for graph read/list/write, skill compile/export, routing, one-step Weavatrix evidence compilation, and Refactor preview.
- Deterministic evidence selection with stable IDs, priorities, token budgets, omission reporting, and fail-closed critical evidence.
- Explicit local-small, local-medium, and upstream-strong routing tiers.
- Structured Ollama drafts with exact model tags, schema/evidence validation, CPU/GPU reporting, and no hidden model download or downgrade.
- Native Weavatrix graph evidence plus Rust-only validation and read-only rendering of upstream-authored refactor plans.
- Durable run snapshots with ready/running/succeeded/failed/skipped/cancelled node state, deterministic edge transitions, optimistic commands, event history, HTTP/MCP controls, and SVG overlays.
- Immutable attempt-scoped evidence, audited human/review decisions, graph-configured bounded retries, and read-only deterministic replay verification.
- Offline model-profile benchmark and calibration harness (`cortex-eval`): typed fixtures for classification/extraction/compression, pure comparators, pinned prompt/schema versions, fail-closed verdicts, and explicit `model_absent` skips instead of hidden pulls.

## Next implementation milestones

1. ~~Extend durable runs with evidence invalidation, executor leases, lease expiry, and explicit external-executor identity.~~ Done: typed `ExecutorIdentity`, `claim_lease`/`release_lease` commands with bounded TTL and lazy replay-deterministic expiry, lease enforcement on node commands, retry/completion lease clearing, and `invalidate_evidence` that blocks future citations while keeping the audit record. UI surfaces for leases remain future work.
2. ~~Add graph-to-agent adapters for Codex, Claude Code, and Copilot while keeping the canonical graph vendor-neutral.~~ Done (iteration 1): `cortex-adapters` renders preview-only vendor wiring (skill instructions plus MCP registration) from one canonical graph via the `adapter_export` MCP tool and `GET /api/adapters/{agent}`. Remaining: run-node execution by external agents stays gated on milestone 1 leases and executor identity.
3. ~~Wire evaluated Ollama profiles into the MCP host behind explicit runtime configuration and shadow-mode metrics.~~ Done (iteration 1): `cortex-shadow` observes `route_work` and `weavatrix_context_compile` behind `CORTEX_SHADOW=1`, with append-only samples, `shadow_metrics_read`, and `/api/shadow/*`; see [shadow-mode.md](shadow-mode.md). Remaining: promotion criteria stay gated on calibration verdicts and shadow agreement data.
4. ~~Add embeddings and retrieval evaluation before permitting semantic evidence selection.~~ Done across three iterations: bounded `/api/embed`; repository-specific Recall@k/nDCG fixtures with structural relatedness; three pinned ranking modes where `hybrid_graph` (RRF of embedding + BM25 plus a rank-space graph boost) passes the gate on both installed models (`embeddinggemma` 0.96/0.92, `qwen3-embedding:0.6b` 1.00/0.96); and gated production wiring — `CORTEX_SEMANTIC=1` + model tag enables within-band semantic ordering inside the deterministic compiler, with packet provenance (`semanticRanking`) and deterministic fallback plus a recorded warning on any failure. The ranking module lives in `cortex-context` and is shared by the harness and production. Remaining ideas: adjacency from live Weavatrix neighborhoods instead of split-parent grouping, and semantic scores in the shadow compression comparison.
5. ~~Import a small, licensed Superpowers-derived methodology fixture set and build scenario/pressure tests for round-trip behavior.~~ Done: nine original methodology skills in the Superpowers `SKILL.md` format (`crates/cortex-skills/fixtures/` with a provenance NOTICE; no upstream prose copied) — test-driven development, systematic debugging, grounded review, evidence-first change, blast-radius analysis, interface-contract change, dependency upgrade, performance investigation, and incident response — plus pressure tests: semantic round-trip with an export fixpoint check, CRLF and Unicode stability, a 60-step dependency chain, and hostile-frontmatter rejection. The tests immediately caught a real sharp edge — `[depends: N]` is a tail annotation, punctuation after it breaks label stability — now documented by the fixtures themselves.
6. ~~Remove the JavaScript compatibility oracle without claiming a native semantic planner.~~ Done: `weavatrix-rust` 2.3 supplies repository intelligence; the upstream coding agent authors a complete `weavatrix.refactor-plan.v1`; `weavatrix-refactor-plan` validates and fingerprints it; and `weavatrix-edit` renders exact modify/rename results in memory. Create/delete/rename/modify paths are repository-confined and hash-guarded. Node, `.mjs`, apply, rollback, confirmation tokens, and worktree execution are absent.
7. ~~Run official MCP conformance and adversarial stdio tests before treating the server as production-ready.~~ Done. Adversarial stdio tests drive the real loop with hostile input — raw garbage, truncated and non-object JSON, null/duplicate/string ids, premature and unknown calls, a 5 MiB line against the 4 MiB limit, and 2000-deep nesting — asserting the loop never panics and answers valid requests after garbage (tool-level failures arrive as `result.isError`, protocol failures as JSON-RPC errors). Streamable HTTP transport shipped (`cortex-mcp --http 127.0.0.1:43818` or `CORTEX_MCP_HTTP`): one HTTP session bridges to one in-process MCP loop over bounded channel pipes, with `Mcp-Session-Id` issuance on initialize, protocol-version header validation, loopback Origin enforcement, bounded sessions with idle expiry, DELETE termination, and 405 on GET (no server-initiated streams). The official suite passes against the committed baseline (`config/mcp-conformance-baseline.yaml`): server-initialize, ping, tools-list, SSE behavior, and both DNS-rebinding checks pass; the remaining entries are suite fixture tools and capabilities (resources/prompts/logging/completion) this tools-only server does not declare. Remaining: adversarial stdio tests; resources/prompts support if ever needed.
8. ~~Add run-level token, latency, device, rejection, fallback, and quality-equivalence telemetry.~~ Done: the append-only usage ledger records every `route_work` decision and `weavatrix_context_compile` savings figure with optional `runId` attribution; the quality summary joins savings with run outcomes so only clean succeeded runs are credited; and executors close the balance by self-reporting upstream consumption via `usage_report` (MCP) or `POST /api/usage/reports` — the adapter usage contract instructs agents to do both and to default `maxTokens` to the measured 4000. Reports are honest self-reporting, not billing verification.
9. ~~Adapt the useful Superpowers mechanics without adding Superpowers as a dependency.~~ Done: 13 mechanics are represented by seven versioned Cortex-native sequence templates, copied into detached editable graphs, linted before use, recommended by a deterministic bounded candidate set, and compiled into one active-step packet at a time. The global `using-superpowers` bootstrap is intentionally omitted. HTTP, MCP, and Sequence Studio expose the same canonical graphs; the four-arm deterministic benchmark promotes the native representation only when no declared scenario regresses.

## Visibility

The editor has a **Model interaction** panel (header button) that reads the
recorded telemetry: where routing sent work and how much of it stayed away
from the upstream agent, evidence-budget figures with self-reported upstream
consumption beside them, per-model shadow aggregates with missed escalations
highlighted, deterministic-versus-shadow decisions side by side, and the most
recent routing and compilation samples. It is strictly read-only — the UI
never writes telemetry and cannot influence a workflow.

## Deployment note — aarch64 (Raspberry Pi)

The pure-Rust crates (`cortex-context`, `cortex-domain`, `cortex-run`, `cortex-router`, `cortex-skills`, `cortex-adapters`) cross-check cleanly for `aarch64-unknown-linux-gnu` (verified 2026-08-04). Full binaries additionally need a C cross-compiler for bundled SQLite: use `cargo-zigbuild` (lightest), `cross` (Docker), or simply build on the device — all dependencies compile from source on aarch64 Linux.

## Research gates

- Compare `qwen3.5:4b`, `phi4-mini`, and a medium local profile on extraction, classification, and citation-preserving compression. Closed on this device under pinned `eval-prompts-v3` with role-aware verdicts: `qwen3.5:4b` passes the `local_small` role (classification 0.82, zero missed escalations, extraction exact-match 0.80) and `qwen3.5:9b` passes `local_medium` (perfect citation preservation). `phi4-mini` remains below both small gates. Dogfood note: shadow compression of a real 7.5k-token packet timed out on CPU — synthetic-fixture latency does not transfer to production payload sizes.
- Do not claim NPU execution until an OpenVINO GenAI or Foundry Local adapter reports and passes calibration on the actual device.

### Which model could actually go on this NPU

Measured on the development machine 2026-08-05: **Intel Core Ultra 7 255U**
(Arrow Lake-U), Intel AI Boost NPU, Arc iGPU, 47.5 GB RAM. Intel rates the
whole platform at **24 peak TOPS INT8** — CPU, GPU and NPU combined — so this
is not a 40-TOPS Copilot+ class part, and the NPU alone is a fraction of that
figure.

OpenVINO 2026 GenAI names **Qwen2.5-1.5B-Instruct** and
**Qwen3-Embedding-0.6B** as NPU-supported. Qwen3 4B and 8B INT4 IRs exist but
are documented for CPU and GPU. NPU driver 32.0.100.4023 or newer is required.

Ranked by value per unit of risk:

1. **`Qwen3-Embedding-0.6B` on the NPU.** The retrieval gate already passed
   this model (Recall@k 1.00, nDCG 0.96) and gated semantic ordering is
   already wired behind `CORTEX_SEMANTIC=1`, where it reorders only within a
   priority band and falls back deterministically. Moving it off the CPU makes
   an already-validated feature cheap enough to run on every compile. Nothing
   about the trust model changes.
2. **`Qwen2.5-1.5B-Instruct` on the NPU for `route_work` classification.**
   Bounded input, schema-validated output, fail-closed on mismatch. It must
   re-pass the same `local_small` gate the CPU profiles passed (classification
   ≥ 0.80, **zero** missed escalations) before it is allowed to decide
   anything.
3. **Nothing larger, here.** A 3B or 7B is not on the NPU generative path on
   this part, and the thing such a model would be for — compressing evidence —
   is already measured as a dead end twice over: shadow compression of a real
   7.5k-token packet timed out on CPU, and [benchmark.md](benchmark.md) shows
   the 71 % token reduction comes from priority-ordered budgeting across
   operations, not from a model. A larger local model would add latency and a
   trust problem without touching the lever that moves tokens.

This work is blocked on the pluggable-provider abstraction: OpenVINO is not an
Ollama endpoint, so it needs `LlmProvider` first. It stays loopback-only by
construction — the runtime is in-process, not a network service.
- Establish repository-specific Recall@k, nDCG, classification F1, unsupported-claim rate, and zero-missed-escalation fixtures. Recall@k/nDCG fixtures exist and are measured (`--suite retrieval`); classification and escalation fixtures were established earlier; unsupported-claim rate remains open.
- Measure end-to-end upstream token savings only on quality-equivalent accepted outcomes.

The “90% less upstream work” target remains a hypothesis until those gates pass.
