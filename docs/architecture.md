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
- `cortex-store`: SQLite persistence, optimistic revisions, and history.
- `cortex-skills`: Markdown skill import and canonical readable export.
- `cortex-router`: deterministic risk and execution policy.
- `cortex-ollama`: bounded Ollama discovery and structured drafting.
- `cortex-weavatrix`: MCP client and safe Weavatrix/Refactor preview workflows.
- `cortex-mcp`: bounded stdio tools for Codex and Claude.
- `cortex-server`: local HTTP API and embedded graph UI host.
- `ui`: controlled React/SVG editor; it never owns canonical persistence.

The protocol-independent crates do not depend on MCP, HTTP, or the UI. `cortex-mcp` uses `mcport`; JSON-heavy protocol paths use `blazingly-json`.

## Safety boundaries

- Stable MCP `2025-11-25` is the compatibility baseline; newer revisions are negotiated, not assumed.
- All frames, queues, tool runtimes, model contexts, graph sizes, and response sizes are bounded.
- Weavatrix Refactor is preview-only in the first milestone. Apply/rollback is intentionally absent.
- Graph writes require the current revision; stale clients receive a conflict instead of overwriting newer state.
- Secrets are read only from runtime configuration and never stored in graph documents.
- Hardware/device placement is measured. Ollama GPU residency is not reported as NPU execution.

## Public extraction candidates

After API and security review, `cortex-domain`, `cortex-skills`, `cortex-router`, and the generic MCP client may become separate public repositories. The application, model policy, run history, user workflows, and UI remain private unless separately approved. Licensing is intentionally undecided for this private milestone.

