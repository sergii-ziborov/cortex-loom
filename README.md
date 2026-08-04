# Cortex Loom

Cortex Loom is a local, graph-native process control plane in front of Codex, Claude, and Copilot. It selects evidence with deterministic tools and Weavatrix, delegates bounded low-risk transformations to explicit local-model profiles, verifies their output, and leaves ambiguous, mutating, or high-risk engineering decisions to the upstream coding agent.

The first milestone contains:

- a typed, editable process graph with human, evidence, test, review, retry, handoff, local-model, and upstream-agent nodes;
- round-trip `SKILL.md` import/export with source provenance;
- SQLite persistence with optimistic revisions and graph history;
- executable run snapshots with immutable evidence, audited human decisions, bounded retries, deterministic replay verification, expiring executor leases with explicit identity, and audited evidence invalidation;
- deterministic evidence selection and token-budget reporting, including one-step compilation of typed Weavatrix evidence;
- inspectable model/context routing with fail-closed escalation;
- an offline model-profile calibration harness (`cortex-eval`) with typed fixtures, fail-closed verdicts, and no hidden model pulls;
- opt-in shadow observation (`cortex-shadow`, `CORTEX_SHADOW=1`) that measures local profiles on real MCP traffic with append-only samples and zero workflow influence;
- preview-only vendor adapters (`cortex-adapters`): `adapter_export` renders Claude Code, Codex, or Copilot wiring from the canonical graph without writing files;
- an append-only token-accounting ledger: routing decisions, compilation savings with run attribution, quality-equivalent crediting (only clean succeeded runs), and executor-reported upstream consumption (`usage_read`, `usage_report`, `GET /api/usage/*`);
- a retrieval evaluation gate for embedding profiles (Recall@k/nDCG on repository-specific fixtures, three pinned ranking modes) and, behind it, opt-in semantic evidence ordering (`CORTEX_SEMANTIC=1`): the gated hybrid BM25+embedding+graph ranking reorders fragments only within priority bands, records provenance on the packet, and falls back to deterministic order on any failure;
- native `weavatrix-rust` repository evidence and preview-only Weavatrix Refactor integration;
- bounded MCP tools plus a browser-based React/SVG editor extracted from AI Dev System.

This repository is private. Candidate reusable crates remain in the workspace until their APIs, tests, security boundaries, and licensing are reviewed for separate public release.

## Run locally

```powershell
npm.cmd --prefix ui ci
npm.cmd --prefix ui run build
cargo run -p cortex-server
```

Calibrate local model and embedding profiles with `cargo run -p cortex-eval -- --discover` and `cargo run -p cortex-eval` (reports land in `.cortex-loom/eval/`; absent models are skipped, never pulled). The editor opens at `http://127.0.0.1:43817`. When the UI was built before `cargo build`, its assets are embedded into `cortex-server` and the release binary is a single self-contained file; `--ui-dir` or `CORTEX_LOOM_UI_DIR` still serve from disk for development. Save a graph before creating a run; ready/running/completed node and edge states are rendered directly on the SVG. The run workbench submits provenance-bearing evidence, records approve/reject decisions, triggers graph-configured retries, and verifies replay without repeating external work. Run the stdio MCP server with `cargo run -p cortex-mcp`, or serve Streamable HTTP with `cargo run -p cortex-mcp -- --http 127.0.0.1:43818` (sessions via `Mcp-Session-Id`, loopback-only origins; the official MCP conformance suite passes against `config/mcp-conformance-baseline.yaml`).

Design notes: [architecture](docs/architecture.md), [research](docs/research.md), [evaluation gates](docs/evaluation.md), and [roadmap](docs/roadmap.md).
