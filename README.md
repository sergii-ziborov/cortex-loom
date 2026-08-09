# Cortex Loom

Cortex Loom is a local, graph-native process control plane in front of Codex, Claude, and Copilot. It selects evidence with deterministic tools and Weavatrix, delegates bounded low-risk transformations to explicit local-model profiles, verifies their output, and leaves ambiguous, mutating, or high-risk engineering decisions to the upstream coding agent.

The first milestone contains:

- a typed, editable process graph with human, evidence, test, review, retry, handoff, local-model, and upstream-agent nodes;
- round-trip `SKILL.md` import/export with source provenance;
- seven Cortex-native editable sequences that adapt 13 useful Superpowers mechanics without a plugin or runtime dependency; only the active typed step enters model context;
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
- bounded MCP tools plus a browser-based React/SVG editor, including a read-only **Model interaction** panel showing routing, evidence budgets, and deterministic-versus-shadow decisions;
- a bundled methodology library of 31 original Cortex workflows, plus seven detached-copy sequence templates, seeded on first run so the editor opens with working material rather than empty;
- **methodology library import** from a local checkout: every `SKILL.md` under a path the operator names is compiled into graphs, licence and notice files beside them are surfaced before anything is stored, title collisions are disambiguated instead of dropped, and importing twice never overwrites an edited workflow. No third-party text is vendored into this repository;
- in-app **Help** and **Docs** panels: keyboard and node/edge reference plus the full design documentation served from the binary at `/api/docs`, so a running instance explains itself offline;
- deterministic context and methodology benchmarks (`cortex-bench`) covering naive, Weavatrix, Cortex, raw Superpowers 6.2.0, and Cortex-native sequence arms — see [benchmark](docs/benchmark.md) and [competitors](docs/competitors.md).

This repository is private. Four crates inside it are prepared for public release under **MIT OR Apache-2.0** — `cortex-domain` (typed process-graph schema), `cortex-context` (budget-bounded evidence selection and retrieval ranking), `cortex-router` (fail-closed routing policy), and `cortex-skills` (`SKILL.md` round trip). Each carries its own README, license texts, and publishing metadata; everything else stays private and unlicensed. See [publishing](docs/publishing.md).

## What Cortex Loom is — and is not

Cortex Loom is the audit, budget, and authority layer between repository
intelligence and a coding agent. It does not replace the agent, and it does
not try to replace Weavatrix. Weavatrix supplies the native Rust repository
graph, search, dependency, transport, architecture, and preview operations;
Cortex chooses which operations to ask for, packs their evidence under a
budget, runs explicit sufficiency gates, and records what was allowed to act.

It is intentionally not:

- an autonomous code-writing model;
- a second repository indexer;
- a hosted tracing dependency;
- a JavaScript Weavatrix compatibility layer;
- an auto-apply path for Weavatrix Refactor.

The backend, sequence compiler, router, run engine, model gates, and Weavatrix
adapter are Rust. The only TypeScript is the browser UI. Superpowers is an
evaluation and design source, not a runtime or package dependency; a local
checkout is read only by the optional benchmark when the operator supplies its
path.

## Request flow

1. A deterministic router classifies the request and establishes a risk floor.
2. Manual choice or a bounded recommendation selects an editable sequence.
3. Only the current typed sequence step is compiled into an `ActiveStepPacket`;
   the rest of the workflow remains graph state outside model context.
4. The sequence contributes `PlanHints`: intent, source follow-up policy, and
   whether a change plan is inappropriate for the task.
5. Native Weavatrix operations gather search, symbol, caller, module, endpoint,
   and bounded source evidence. Direct source windows outrank search metadata.
6. A deterministic sufficiency check permits at most one targeted retry. A
   still-thin or contradictory packet becomes an upstream handoff, never a
   confident local answer.
7. The run engine records immutable evidence, attempts, decisions, leases,
   retries, usage, and replay state. Mutation and integration still require the
   authority declared by the graph.

The protocol-independent graph, router, context compiler, sequence compiler,
and run engine do not depend on MCP or the React UI. MCP, HTTP, and the desktop
editor are transports over the same typed contracts.

## Seven editable Cortex sequences

| template | purpose |
| --- | --- |
| `discover-and-plan` | turn ambiguity into a cited, approved implementation plan |
| `bounded-implementation` | execute approved work in small test-led slices |
| `root-cause-debugging` | reproduce, isolate, test one hypothesis, then correct |
| `review-and-correct` | verify review findings before changing code |
| `verify-and-integrate` | map claims to fresh proof and integrate only with authority |
| `parallel-investigation` | split independent read-only work and reconcile contradictions |
| `sequence-authoring` | create or revise sequences through pressure tests and lint |

Together they encode 13 useful mechanics adapted from Superpowers:
brainstorming, planning, plan execution, TDD, systematic debugging, worktree
isolation, parallel and subagent work, receiving and requesting review,
verification, branch finishing, and skill authoring. The global
`using-superpowers` bootstrap is deliberately omitted because eager mandatory
prose reduced precision in this product.

Built-ins are immutable versioned templates. **Use and edit** creates a
detached normal graph: users may change steps, gates, budgets, evidence needs,
model profiles, escalation, and mutation policy without changing the shipped
template. Sequence lint blocks execution on missing authority or safety edges,
while drafts remain saveable. Template upgrades never rewrite a user copy.

## Models and authority

Model names in a graph are capability profiles, not permission to pull or use
an arbitrary checkpoint. The exact model, quantization, device, runtime,
prompt/schema version, latency ceiling, and evaluation verdict belong to the
deployment profile. A local model may only narrow work inside the authority of
its role; any invalid output, unavailable endpoint, failed gate, or higher risk
falls back to deterministic logic or the upstream agent.

| profile | role today | authority |
| --- | --- | --- |
| `gpu-embedding` | Qwen3 Embedding 0.6B ordering within an existing priority band | cannot add evidence or change the risk floor; deterministic fallback |
| `npu-classifier` | Qwen3 8B INT4 route classification | may escalate above the lexical route, never downgrade it |
| `gpu-digest` | future off-path per-revision digest cache | disabled; no hot-path or mutation authority |
| `npu-micro-extract-qwen3-0.6b` | future literal extraction from already verified input | disabled until its exact deployment passes schema, precision/recall, unsupported-output, and latency gates |

The 0.6B lane is intentionally not a miniature planner or router. Its proposed
contract is closed-schema extraction with mechanical validation and zero
completion, sufficiency, routing, or mutation authority. See
[local models](docs/local-models.md) and [evaluation gates](docs/evaluation.md).

## Evidence and safety boundaries

- Critical evidence fails closed when it cannot fit; it is never silently
  truncated into a plausible packet.
- Source follow-up is bounded and requirement-aware. Search metadata cannot
  evict the direct source facts that caused a retry to pass.
- Local classifiers can only escalate; local drafts are schema- and
  citation-validated; shadow observations have zero workflow influence.
- Refactor accepts an upstream-authored, hash-guarded plan and renders an
  in-memory preview. It cannot apply, confirm, roll back, or write files.
- Human decisions cite same-attempt evidence and record actor plus reason.
- Run evidence is immutable. Invalidation is an audited event, not deletion.
- External executors use expiring identity-bearing leases; replay reports
  divergence without repairing history.

## Interfaces

- **React/SVG editor** on `:43817`: graphs, runs, evidence, Sequence Studio,
  model interaction, library import, help, and embedded docs.
- **MCP stdio or Streamable HTTP** on `:43818`: bounded graph, run, sequence,
  routing, skill, usage, Weavatrix context, and preview tools.
- **HTTP API**: the same graphs, sequences, runs, telemetry, adapters, and docs
  used by the UI.
- **`SKILL.md` round trip**: compile Markdown into a typed graph and export a
  stable fixpoint without making prose the runtime authority.
- **Preview-only agent adapters**: render Codex, Claude Code, and Copilot
  wiring; never write vendor configuration automatically.

The editor's diagram engine remains the established React/SVG canvas with
keyboard navigation, zoom, auto-layout, editable nodes/edges, and live run
overlays. Sequence Studio is a separate dialog over the same graph documents;
it does not fork or replace the canvas layout engine.

## Benchmarks

`cortex-bench` keeps repository retrieval and methodology-context evaluation
separate:

- the **probe benchmark** compares naive whole-directory context,
  `weavatrix-raw`, the previous four-operation Cortex path, planned Weavatrix,
  targeted Cortex, an untrimmed plan, and targeted Cortex with verified source
  follow-up;
- the **sequence benchmark** compares no methodology, current Cortex skills,
  raw Superpowers 6.2.0, and the seven Cortex-native sequences across 28
  declared quality/safety scenarios.

Latest deterministic results (2026-08-09):

| repository arm | estimated tokens | anchor facts |
| --- | ---: | ---: |
| naive known directories | 310 625 | 40/40 |
| raw Weavatrix | 87 462 | 26/40 |
| Cortex targeted | **20 349** | 28/40 |
| Cortex targeted + verified source | **34 564** | **40/40** |

| methodology arm | estimated tokens | scenarios passed |
| --- | ---: | ---: |
| current bundled Cortex skills | 10 401 | 3/28 |
| raw Superpowers 6.2.0 | 72 839 | 15/28 |
| Cortex-native active-step packets | **3 812** | **28/28** |

The source arm preserves the previous best 40/40 while using 1 541 fewer
estimated tokens; targeted preserves 28/40 while using 2 258 fewer. Native
sequences use 94.77% fewer methodology tokens than raw Superpowers with no
declared scenario regression. The paired live `qwen3.5:4b` sequence smoke did
not pass its exact gate (0/12, p95 86.8 s), so that model is not promoted.

Promotion is fail-closed: both current and raw baselines must be available,
native must have zero scenario regression against either, token and latency
ceilings must pass, and repeated static reports must be byte-identical. The
probe fixture also contains no materialized scored literal, so retrieval
cannot earn recall by finding its own answer list. Recall means declared
literals were present in evidence; it is not proof that a model answered
correctly. The naive arm receives the correct globs and token counts use a
four-characters estimate, so every report carries those caveats. Exact latest
measurements and stamps live in [benchmark](docs/benchmark.md).

Run the deterministic suites with:

```powershell
cargo run -p cortex-bench -- --repo . --budget 4000 --set probe `
  --out .cortex-loom/bench/probe.json --stamp local-probe

cargo run -p cortex-bench -- sequence `
  --superpowers-root C:\path\to\superpowers `
  --out .cortex-loom/bench/sequences.json
```

## Run locally

```powershell
npm.cmd --prefix ui ci
npm.cmd --prefix ui run build
cargo run -p cortex-server
```

Calibrate local model and embedding profiles with `cargo run -p cortex-eval -- --discover` and `cargo run -p cortex-eval` (reports land in `.cortex-loom/eval/`; absent models are skipped, never pulled). The editor opens at `http://127.0.0.1:43817`. When the UI was built before `cargo build`, its assets are embedded into `cortex-server` and the release binary is a single self-contained file; `--ui-dir` or `CORTEX_LOOM_UI_DIR` still serve from disk for development. Save a graph before creating a run; ready/running/completed node and edge states are rendered directly on the SVG. The run workbench submits provenance-bearing evidence, records approve/reject decisions, triggers graph-configured retries, and verifies replay without repeating external work. Run the stdio MCP server with `cargo run -p cortex-mcp`, or serve Streamable HTTP with `cargo run -p cortex-mcp -- --http 127.0.0.1:43818` (sessions via `Mcp-Session-Id`, loopback-only origins; the official MCP conformance suite passes against `config/mcp-conformance-baseline.yaml`).

Design notes: [architecture](docs/architecture.md), [research](docs/research.md), [evaluation gates](docs/evaluation.md), and [roadmap](docs/roadmap.md).
