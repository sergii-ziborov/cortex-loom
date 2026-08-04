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
- `cortex-eval`: offline benchmark and calibration harness for local model profiles; pure comparators and pinned prompts shared with shadow mode; never pulls a model.
- `cortex-shadow`: opt-in shadow observation of local profiles on real MCP traffic; bounded queue, dedicated worker thread, append-only samples, zero workflow influence.
- `cortex-adapters`: preview-only vendor wiring (Claude Code, Codex, Copilot) rendered from the canonical graph; never writes files.
- `cortex-weavatrix`: native `weavatrix-rust` evidence, typed conversion into the transport-independent context compiler, plus a compatibility client for safe Weavatrix Refactor previews.
- `cortex-mcp`: bounded stdio tools for Codex and Claude.
- `cortex-server`: local HTTP API and embedded graph UI host.
- `ui`: controlled React/SVG editor; it never owns canonical persistence.

The protocol-independent crates do not depend on MCP, HTTP, or the UI. `cortex-mcp` uses `mcport`; JSON-heavy protocol paths use `blazingly-json`.

## Safety boundaries

- Stable MCP `2025-11-25` is the compatibility baseline; newer revisions are negotiated, not assumed.
- The Streamable HTTP transport shares the stdio tool registry and runtime limits: each HTTP session is one in-process MCP loop, sessions are bounded and idle-expired, non-loopback origins are rejected, and the server initiates no streams. Bind beyond loopback only behind an authenticating proxy.
- All frames, queues, tool runtimes, model contexts, graph sizes, and response sizes are bounded.
- Weavatrix Refactor is preview-only in the first milestone. Apply/rollback is intentionally absent.
- Graph writes require the current revision; stale clients receive a conflict instead of overwriting newer state.
- Runs retain the exact graph revision and snapshot from which they were created; graph edits never rewrite an active run.
- Every run command requires the current run revision and appends one durable event in the same SQLite transaction.
- Evidence is an immutable submission scoped to one node attempt. A later retry preserves it for audit but cannot cite it as evidence for the new attempt.
- Evidence can be invalidated with an audited actor and reason; the submission record is never deleted, but an invalidated id can no longer be cited by later commands. Historical decisions that cited it before invalidation are never rewritten.
- Executor leases give one explicit, typed identity (human, upstream agent, local model, or service) exclusive execution of a node. Expiry is evaluated lazily against each command's recorded timestamp, so it is deterministic under replay; an expired lease is claimable and a reopened retry attempt is never pinned to the previous executor. A node without a lease stays open: leases add exclusivity, never authority.
- Human and review gates reject generic completion. They require an explicit `approved` or `rejected` decision with actor, reason, and same-attempt evidence references.
- Successful completion traverses sequence/context/tool/success/approval/requires edges; failure traverses failure/fallback/reject/escalation edges.
- Conditional edges are never inferred from free-form expressions. A branch transition requires an explicit edge ID.
- Fan-in nodes wait until every incoming executable edge is resolved. Unselected branches become `skipped` and propagate `not_taken`.
- Run schema v1 rejects arbitrary executable cycles. Retry is an explicit controller node with `targetNodeId` and `maxAttempts` configuration plus a failure transition from the target.
- A retry command reopens only the target's forward closure. When the target reaches `maxAttempts`, its retry edge becomes `not_taken` and the run resolves normally instead of remaining retryable.
- Events contain the exact bounded command payload and run/graph identity. Replay starts from the immutable graph snapshot, requires contiguous sequences, reapplies commands deterministically, and only reports mismatch; it never repairs state or repeats external work.
- Imported skill graphs start at revision zero and become canonical only after an explicit save.
- Semantic ordering is opt-in (`CORTEX_SEMANTIC=1` plus a model tag holding a passing `hybrid_graph` retrieval verdict), reorders evidence only within a priority band, records its provenance on the packet, and falls back to deterministic order with a recorded warning on any failure. It can never omit critical evidence, alter trust states, or change fail-closed escalation.
- Local-model graph policies must be non-mutating and require upstream review.
- High-risk graph policies may target only an upstream agent or human.
- Weavatrix evidence is returned as stable, individually citable evidence fragments. Oversized tool results split deterministically into ordered sub-citations (`WX-VERIFY-1..n`) at paragraph boundaries, so a token budget keeps a prefix of a large fragment instead of dropping it whole.
- Weavatrix change-plan evidence remains unverified until an upstream agent or later verification phase resolves it; symbol evidence is critical and never silently omitted by the context budget.
- Refactor confirmation/apply tokens are recursively removed from requests and preview responses.
- Secrets are read only from runtime configuration and never stored in graph documents.
- Hardware/device placement is measured. Ollama GPU residency is not reported as NPU execution.

## Public extraction candidates

After API and security review, `cortex-domain`, `cortex-context`, `cortex-skills`, `cortex-router`, and the generic MCP client may become separate public repositories. The application, model policy, run history, user workflows, and UI remain private unless separately approved. Licensing is intentionally undecided for this private milestone.
