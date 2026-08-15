# Architecture

Cortex Loom is a control plane, not an autonomous replacement for Codex or Claude.

## Runtime flow

1. A deterministic router sets a risk floor. Local models may only escalate.
2. An optional sequence contributes one active step and `PlanHints`.
3. The planner picks Weavatrix operations from the task: search, symbols,
   callers, modules, endpoints, source windows, git history, stack traces,
   test selection, and prior-run memory only when an explicit completed
   `runId` is supplied. Wording such as "previous attempt" never searches
   other Failed, Cancelled, or Running runs. Coding-agent integrations
   launch `--profile agent` (`cortex_prepare` / `cortex_expand`); Full is
   for Studio. Search globs follow named file suffixes or a multi-language
   default, not `**/*.rs`.
4. Sufficiency allows one targeted retry. A still-thin packet becomes an
   upstream handoff, never a confident local answer.
5. Codex or Claude receives the compact evidence and a coverage
   certificate — present, missing, contradictory, or stale — and remains
   responsible for ambiguous or high-risk engineering decisions.

Local output is advisory. No model may publish, deploy, approve a release, apply a refactor, or mutate workflow state solely from self-reported confidence.

## Graph layers

- Methodology graphs describe reusable workflows such as TDD, review, and verification.
- Run graphs instantiate project tasks, evidence, retries, and current state.
- Generated adapters expose bounded MCP tools/resources and readable `SKILL.md`, Mermaid, or DOT views.

Generated Markdown is a view; the typed, versioned graph is canonical.

## Modules

- `cortex-domain`: transport-independent graph schema and invariants.
- `cortex-context`: deterministic evidence prioritization, bounded packets, a named `TokenCounter`, and token accounting that splits budget omissions from dedup savings.
- `cortex-run`: transport-independent run snapshots, evidence and decision audit, bounded attempts, deterministic edge transitions, and replay.
- `cortex-store`: SQLite persistence, optimistic revisions, and history.
- `cortex-skills`: Markdown skill import and canonical readable export.
- `cortex-sequences`: protocol-independent template activation, detached editable copies, linting, and one-step active packets; it has no Superpowers runtime dependency.
- `cortex-router`: deterministic risk and execution policy.
- `cortex-ollama`: bounded Ollama discovery and structured drafting.
- `cortex-eval`: offline benchmark and calibration harness for local model profiles; pure comparators and pinned prompts shared with shadow mode; never pulls a model.
- `cortex-shadow`: opt-in shadow observation of local profiles on real MCP traffic; bounded queue, dedicated worker thread, append-only samples, zero workflow influence.
- `cortex-adapters`: preview-only vendor wiring (Claude Code, Codex, Copilot) rendered from the canonical graph; never writes files.
- `cortex-weavatrix`: native `weavatrix-rust` evidence, typed conversion into the transport-independent context compiler, and bounded Rust validation/rendering of upstream-authored `weavatrix.refactor-plan.v1` plans.
- `cortex-mcp`: bounded stdio tools for Codex and Claude.
- `cortex-server`: local HTTP API and embedded graph UI host.
- `ui`: controlled React/SVG editor; it never owns canonical persistence.

The protocol-independent crates do not depend on MCP, HTTP, or the UI. `cortex-mcp` uses `mcport`; JSON-heavy protocol paths use `blazingly-json`.

## Safety boundaries

- Stable MCP `2025-11-25` is the compatibility baseline; newer revisions are negotiated, not assumed.
- The Streamable HTTP transport shares the stdio tool registry and runtime limits: each HTTP session is one in-process MCP loop, sessions are bounded and idle-expired, non-loopback origins are rejected, and the server initiates no streams. Bind is loopback-only unless `--allow-remote` is set; remote still requires a TLS reverse proxy, authentication, and a workspace allowlist (`--workspace` or `CORTEX_WORKSPACE_ALLOWLIST`). Remote sessions are audited.
- Packet evidence is a data-only `<evidence>` envelope with CDATA. Source text is not Markdown headings and must not be treated as instructions.
- Weavatrix sessions are per-repository. The map lock only looks up a slot; agents on different repos do not block each other. Idle slots evict under an LRU cap and a 30-minute TTL. Embeddings cache by content hash.
- The optional local classifier runs only on lexical ambiguity, mixed-script tasks, detector disagreement, or a floor below `upstream_strong`. An already-obvious high-risk route does not pay for an 8B call.
- Fine-tune rows live in `corpora/train` and `corpora/dev`. Gold lives in `crates/cortex-eval/fixtures/` and `eval/public`. `eval/private` is unused by heuristics. The writer refuses exact-hash, repository, and gold-family leakage.
- Callers cannot mint `Verified`. `route_work` treats self-reported verified evidence as absent. `context_compile` downgrades wire items to unverified. Only the Weavatrix adapter assigns trust after a source read.
- Archive-miss and block-join mechanism labels are probe-only (`compile_probe_bundle`). The generic engine does not inject them.
- All frames, queues, tool runtimes, model contexts, graph sizes, and response sizes are bounded.
- Refactor planning intelligence is not claimed locally: an upstream coding agent authors the exact plan, then native Rust parsing, path confinement, hash checks, and `weavatrix-edit` render a read-only preview. Apply, confirmation tokens, rollback, worktrees, and process spawning are absent.
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
- A methodology library is imported from a path the operator names, never fetched and never vendored. The walk is bounded in depth, entry count, and per-file size, and skips hidden and build directories. Preview writes nothing; import uses `seed_if_missing`, so a second import cannot overwrite an edited workflow. Licence and notice files found in the library root are returned with the preview so attribution is visible before storage — the server reports the terms, it does not judge them.
- Semantic ordering is opt-in (`CORTEX_SEMANTIC=1`) **and** a matching `config/calibration/*.json` artifact. A profile `gatePassed` flag is not authority. Startup attests model, digest, runtime, device, pooling, tokenizer, ranking version, fixture-set hash, and adjacency kind; any mismatch disables the scorer. Production uses the same `rank_hybrid_graph` pipeline as eval, with `evidence_spans` adjacency (file + split siblings), not the historical fixture `related` pairs.
- A digest is not gated on citation ids alone. Claim atoms must cite existing evidence, copy literals from those bodies, and cover `mustPreserve` facets. Prose is not a substitute. `gpu-digest` stays `verdictPass: false`.
- Local-model graph policies must be non-mutating and require upstream review.
- High-risk graph policies may target only an upstream agent or human.
- Weavatrix evidence is returned as individually citable fragments. Transport splits share a `groupId` so a long definition stays one logical atom. Citation ids are content-addressed (`ev_<hash>`), not positional `WX-SOURCE-1`. Packets carry `packetId` (`pk_<hash>`) and `snapshotId` (`git:<commit>+dirty:<digest>`); each locator has path, start/end, and `blobHash`. A later expand reports `stale: true` when the tree no longer matches.
- A compiled packet has three layers: L0 decision map (`WX-MAP`: intent, targets, snapshot, required/satisfied/missing facets), L1 verified answer-bearing atoms, and L2 `EXPAND` handles for missing facets. Raw `graph_stats` dumps stay Low and do not occupy L1.
- Completeness is a `CoverageCertificate`, not a boolean: required facets, the citation ids that close each one, missing facets, contradictions, and the snapshot. `sufficient` is derived; a critical missing facet still escalates.
- Gather expands missing facets one at a time under a hard token floor, without repeating an operation. No new evidence ends the loop; a still-open critical facet escalates.
- Packet headings show trust and derivation (`EXACT SOURCE`, `UNVERIFIED PLAN`, `CONTRADICTORY — group C7`). A model must not have to read the JSON envelope to see that a change plan is unverified.
- Dedup only collapses a line when source span, content, snapshot, blob, trust, and derivation all match; otherwise it keeps a provenance pointer (`same source span as [id]`). Contradictory text cannot erase verified text.
- Criticality is a required facet (complete definition, caller signature), not every `SourceReads` window. Surrounding comments are normal and may be omitted.
- Definition completeness prefers a Weavatrix AST span (window covers `start`–`end`) and grouped split pieces. Brace balance is a last-resort fallback that skips strings and comments.
- Which Weavatrix operations are asked is planned from the task text by `cortex_weavatrix::plan`, deterministically and without a model: identifiers are recognised by shape, lightweight intent cues select blast-radius (`get_dependents`), API-contract (`list_endpoints`), module-topology (`module_map`), or runtime-config searches when the question is structural, and the plan is a pure function of task, symbol, and budget. Runtime-config tasks search both product source and `config/**`; the MCP compile path then opens bounded, ranked source windows for search hits. Each operation receives a share of the budget as a Weavatrix `token_budget`, so trimming happens where the tool knows which array to cut. A task naming no code plans structural evidence only, and says so rather than guessing.
- Weavatrix change-plan evidence is requested only when the task explicitly asks for a change or implementation plan. It remains unverified until an upstream agent or later verification phase resolves it. A truncated tail is reported in `omittedIds` and counted in `omittedEstimatedTokens`, so an omission is recorded rather than hidden.
- An optional active skill contributes only typed `PlanHints` (`intent`, `sourceFollowup`, `skipChangePlan`) through frontmatter. The skill compiler and MCP transport do not enter the planner. Gathered evidence is checked for the evidence classes implied by the intent, receives at most one bounded wide-search/source recovery pass, and is checked again after compiler selection. A packet that remains thin is returned with `requiresUpstream: true` and an explicit `sufficiency` report.
- `evidence_gate` has run-time semantics even when a graph author omits an execution policy: a successful completion must cite submitted, current-attempt evidence. The generic `requireEvidence` policy continues to provide the same boundary for other node kinds.
- Refactor preview accepts only `{ repository, plan }`; the response contains the validated preview and no apply/rollback authority.
- Secrets are read only from runtime configuration and never stored in graph documents.
- Hardware/device placement is measured. Ollama GPU residency is not reported as NPU execution.

## Public extraction candidates

`cortex-domain`, `cortex-context`, `cortex-router`, and `cortex-skills` each carry package metadata, their own README, both license texts, and `publish = true`. They are dual-licensed **MIT OR Apache-2.0**; the rest of the workspace is unlicensed (`publish = false`).

The application, model policy, run history, user workflows, shadow and usage telemetry, and the UI remain private unless separately approved.
