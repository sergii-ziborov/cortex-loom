# Architecture

Cortex Loom is a control plane, not an autonomous replacement for Codex or Claude.

## Runtime flow

1. Deterministic parsers and repository tools reduce the search space.
2. Weavatrix supplies revision-bound graph, impact, architecture, and source evidence.
3. A local inference adapter may classify, extract, or compress only bounded evidence.
4. A quality gate validates structure, provenance, risk, and budgets.
5. Codex or Claude receives the compact evidence and remains responsible for ambiguous or high-risk engineering decisions.

Local output is advisory. No model may publish, deploy, approve a release, apply a refactor, or mutate workflow state solely from self-reported confidence.

## Graph layers

- Methodology graphs describe reusable workflows such as TDD, review, and verification.
- Run graphs instantiate project tasks, evidence, retries, and current state.
- Generated adapters expose bounded MCP tools/resources and readable `SKILL.md`, Mermaid, or DOT views.

Generated Markdown is a view; the typed, versioned graph is canonical.

## Modules

- `cortex-domain`: transport-independent graph schema and invariants.
- `cortex-context`: deterministic evidence prioritization, bounded packets, and token-savings accounting.
- `cortex-run`: transport-independent run snapshots, evidence and decision audit, bounded attempts, deterministic edge transitions, and replay.
- `cortex-store`: SQLite persistence, optimistic revisions, and history.
- `cortex-skills`: Markdown skill import and canonical readable export.
- `cortex-router`: deterministic risk and execution policy.
- `cortex-ollama`: bounded Ollama discovery and structured drafting.
- `cortex-weavatrix`: native `weavatrix-rust` evidence, typed conversion into the transport-independent context compiler, plus a compatibility client for safe Weavatrix Refactor previews.
- `cortex-mcp`: bounded stdio tools for Codex and Claude.
- `cortex-server`: local HTTP API and embedded graph UI host.
- `ui`: controlled React/SVG editor; it never owns canonical persistence.

The protocol-independent crates do not depend on MCP, HTTP, or the UI. `cortex-mcp` uses `mcport`; JSON-heavy protocol paths use `blazingly-json`.

## Safety boundaries

- Stable MCP `2025-11-25` is the compatibility baseline; newer revisions are negotiated, not assumed.
- All frames, queues, tool runtimes, model contexts, graph sizes, and response sizes are bounded.
- Weavatrix Refactor is preview-only in the first milestone. Apply/rollback is intentionally absent.
- Graph writes require the current revision; stale clients receive a conflict instead of overwriting newer state.
- Runs retain the exact graph revision and snapshot from which they were created; graph edits never rewrite an active run.
- Every run command requires the current run revision and appends one durable event in the same SQLite transaction.
- Evidence is an immutable submission scoped to one node attempt. A later retry preserves it for audit but cannot cite it as evidence for the new attempt.
- Human and review gates reject generic completion. They require an explicit `approved` or `rejected` decision with actor, reason, and same-attempt evidence references.
- Successful completion traverses sequence/context/tool/success/approval/requires edges; failure traverses failure/fallback/reject/escalation edges.
- Conditional edges are never inferred from free-form expressions. A branch transition requires an explicit edge ID.
- Fan-in nodes wait until every incoming executable edge is resolved. Unselected branches become `skipped` and propagate `not_taken`.
- Run schema v1 rejects arbitrary executable cycles. Retry is an explicit controller node with `targetNodeId` and `maxAttempts` configuration plus a failure transition from the target.
- A retry command reopens only the target's forward closure. When the target reaches `maxAttempts`, its retry edge becomes `not_taken` and the run resolves normally instead of remaining retryable.
- Events contain the exact bounded command payload and run/graph identity. Replay starts from the immutable graph snapshot, requires contiguous sequences, reapplies commands deterministically, and only reports mismatch; it never repairs state or repeats external work.
- Imported skill graphs start at revision zero and become canonical only after an explicit save.
- Local-model graph policies must be non-mutating and require upstream review.
- High-risk graph policies may target only an upstream agent or human.
- Weavatrix evidence is returned as stable, individually citable evidence fragments.
- Weavatrix change-plan evidence remains unverified until an upstream agent or later verification phase resolves it; symbol evidence is critical and never silently omitted by the context budget.
- Refactor confirmation/apply tokens are recursively removed from requests and preview responses.
- Secrets are read only from runtime configuration and never stored in graph documents.
- Hardware/device placement is measured. Ollama GPU residency is not reported as NPU execution.

## Public extraction candidates

After API and security review, `cortex-domain`, `cortex-context`, `cortex-skills`, `cortex-router`, and the generic MCP client may become separate public repositories. The application, model policy, run history, user workflows, and UI remain private unless separately approved. Licensing is intentionally undecided for this private milestone.
