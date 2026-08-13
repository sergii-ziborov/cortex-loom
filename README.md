# Cortex Loom

Cortex Loom is a local, graph-native process control plane in front of Codex, Claude, and Copilot. It selects evidence with deterministic tools and Weavatrix, delegates bounded low-risk transformations to explicit local-model profiles, verifies their output, and leaves ambiguous, mutating, or high-risk engineering decisions to the upstream coding agent.

The current release contains:

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

Four crates are dual-licensed under **MIT OR Apache-2.0** — `cortex-domain` (typed process-graph schema), `cortex-context` (budget-bounded evidence selection and retrieval ranking), `cortex-router` (fail-closed routing policy), and `cortex-skills` (`SKILL.md` round trip). Each carries its own README, license texts, and package metadata; everything else is unlicensed.

## What Cortex Loom is — and is not

Cortex Loom is the audit, budget, and authority layer between repository
intelligence and a coding agent. It does not replace the agent, and it does
not try to replace Weavatrix. Weavatrix supplies the native Rust repository
graph, search, dependency, transport, architecture, and preview operations;
Cortex chooses which operations to ask for, packs their evidence under a
budget, runs explicit sufficiency gates, and records what was allowed to act.

### Ecosystem place (AI control plane)

```text
Cortex Loom (this) ──► asks Weavatrix for code facts
                   ──► may propose GraphPatch / work against Weavatrix Loom
                   ──► does not own Loom Registry or semantic compiler
```

| Product | Role vs Cortex |
| --- | --- |
| **Weavatrix** | Code intelligence (facts) |
| **[Weavatrix Loom](https://github.com/sergii-ziborov/weavatrix-loom)** | Semantic composition + **capability Registry** + compile → Rust |
| **Cortex Loom** (this) | Agent workflow, token economy, routing, process graph |
| **[FerroSift](https://github.com/sergii-ziborov/ferrosift)** | Transform recipes/ops (not Cortex, not Loom Registry) |

The thin `wvx-cortex` crate inside weavatrix-loom is only **intent → GraphPatch
ops** — not this full product. Loom [ADR-0012](https://github.com/sergii-ziborov/weavatrix-loom/blob/main/docs/adr/0012-ecosystem-boundaries.md).

It is intentionally not:

- an autonomous code-writing model;
- a second repository indexer;
- a **capability interchange Registry** or semantic compiler (Weavatrix Loom);
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
  routing, skill, usage, Weavatrix context, and preview tools. `--profile
  context` (or `CORTEX_MCP_PROFILE=context`) serves evidence compilation
  alone — two tools and 454 tokens of schema instead of twenty-seven and
  4 021, for callers that never touch runs, graphs, or sequences.
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

Latest deterministic results (stamp `final-quality-2026-08-11`):

| repository arm | selected tokens | delivered over MCP | anchor facts |
| --- | ---: | ---: | ---: |
| naive known directories | 319 564 | — | 40/40 |
| raw Weavatrix | 100 486 | — | 29/40 |
| Cortex targeted | **23 287** | 27 571 | 28/40 |
| Cortex targeted + verified source | **32 623** | 37 773 | **40/40** |

"Selected" is what the compiler chose; "delivered" is what
`weavatrix_context_compile` actually serializes, which is the figure a caller
budgets against.

Measured end to end as a server, on one question with four declared facts,
against every alternative driven for real over JSON-RPC:

| approach | session tokens | calls | facts |
| --- | ---: | ---: | ---: |
| read the candidate files | 79 040 | — | 4/4 |
| `ripgrep` + file reads | 4 904 | 5 | 3/4 |
| Serena MCP 1.28.1 | 10 540 | 3 | 4/4 |
| **Cortex Loom, `--profile context`** | **4 167** | **1** | **4/4** |

**−94.7% against reading the files, at equal recall, in a single round trip.**
Session tokens include the server's tool schemas, which every client loads for
the whole session: 486 for the context profile against ~4 000 for the full
one. Two changes carry most of that: since mcport 0.5.0 every reply is a
single compact text representation rather than a payload mirrored into
`content` and `structuredContent`, and since 2026-08-12 graph answers are
rendered as text instead of passed through as minified JSON, which alone was
37–63% of every packet. Round trips matter more than the totals suggest,
because each one re-reads the entire session.

On three harder questions against an unfamiliar repository, answered by a
local `qwen3.5:9b`, the context profile is the cheapest arm in the field at
6 945 session tokens — 2.5× under naive, 3.0× under Serena, 4.9× under
Superpowers — and out-answers Serena, the bare agent, and Superpowers while
doing it. Full tables, latency, symbol-resolution accuracy, and a live-model
implementation benchmark are in [benchmark](docs/benchmark.md).

| methodology arm | estimated tokens | scenarios passed |
| --- | ---: | ---: |
| current bundled Cortex skills | 10 401 | 3/28 |
| raw Superpowers 6.2.0 | 72 839 | 15/28 |
| Cortex-native active-step packets | **3 812** | **28/28** |

The source arm preserves 40/40 recall at 89.8% fewer selected tokens than the
naive fixture baseline (88.2% fewer by delivered MCP size); targeted uses
92.7% fewer selected tokens but preserves only 28/40 facts. Native
sequences use 94.77% fewer methodology tokens than raw Superpowers with no
declared scenario regression — but that figure compares one active-step packet
against a whole `SKILL.md`. A complete sequence sends five or six packets, so
the amortized saving is about 66–71%; and the native arm is scored from its own
typed graph while prose arms are scored by keyword inference, which makes 28/28
closer to a consistency check than to a comparison. Both caveats are worked out
in [benchmark](docs/benchmark.md). The paired live `qwen3.5:4b` sequence smoke did
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

Design notes: [architecture](docs/architecture.md), [research](docs/research.md), and [evaluation gates](docs/evaluation.md).
