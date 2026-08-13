# Competitive landscape

Surveyed 2026-08-05. This document exists to stop us from claiming novelty we
do not have. Where a competitor already does something better, that is written
down as such.

## Refresh — 2026-08-09: the nearest products now

There is still no useful single-winner comparison. The closest systems are
different specialists:

| closest on | product | what it has that matters | Cortex boundary |
| --- | --- | --- | --- |
| cross-repo context | Augment Context Engine MCP | live semantic indexing, relationship awareness across repos/services, commit history plus docs/tickets, and curated/compressed retrieval; local and hosted MCP modes | stronger external knowledge and scale; it does not expose Cortex's attempt-scoped evidence gates or editable run semantics |
| local symbol work | Serena | LSP-backed symbol retrieval, references, editing, refactoring and debugger integration through MCP; configurable modes/contexts and a lazy instruction bootstrap | stronger IDE operations; Cortex uses Weavatrix for broader typed transport/architecture evidence and adds budget/authority gates |
| workflow product UX | LangSmith Studio | graph visualization, step/state inspection, threads, time travel, prompt iteration, datasets and experiments | much more mature studio; Cortex graphs are local typed editable documents and do not require a hosted tracing account |
| engineering methodology | Superpowers | 14 mandatory prose skills covering TDD, debugging, planning, worktrees, review and parallel execution | Cortex adapted 13 useful mechanics into seven editable typed sequences and deliberately skipped the global `using-superpowers` bootstrap |
| model-per-task delegation | GitHub Copilot custom agents | per-agent prompt/tools/MCP/skills plus a `model` and `reasoningEffort` override; runtime intent-based delegation | the best idea to borrow, but Cortex must keep deterministic candidate sets, calibrated profile gates and upstream fallback |

Primary evidence: [Augment Context Engine MCP](https://docs.augmentcode.com/context-services/mcp/overview),
[Serena](https://github.com/oraios/serena),
[Serena 2026 changelog](https://github.com/oraios/serena/blob/main/CHANGELOG.md),
[LangSmith Studio](https://docs.langchain.com/langsmith/studio),
[Superpowers](https://github.com/obra/superpowers), and
[GitHub Copilot custom-agent orchestration](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/custom-agents).

### What to take next, without copying a dependency

1. Extend the existing editable per-node `ExecutionPolicy` (target, risk,
   token budgets, evidence/upstream gates, mutation boundary and
   `modelProfile`) with capability-level `role`, `maxTier`, `requiresGate`,
   `maxLatencyMs`, and `fallback=upstream`. The node should name a capability,
   while the calibrated deployment maps it to an exact vendor model tuple.
2. Let the deterministic router produce at most three eligible profiles. An
   embedding may reorder them, but cannot introduce a profile or lower the
   lexical risk floor.
3. Keep `micro_extract` as the only sub-1B candidate: verified input, closed
   schema, literal-output validation, no routing/sufficiency/completion or
   mutation authority. It stays disabled until one exact
   model/quantization/device/runtime tuple passes its gate.
4. Copy Studio's useful debugging affordances — inspect active packet,
   evidence, transition and retry history — while keeping storage and runs
   local. Do not copy hosted tracing as a runtime requirement.
5. Copy Serena's lazy guidance principle: active-step instructions are loaded
   on demand. The measured sequence benchmark shows why full eager skill
   injection is too expensive.

Cortex Loom sits at the intersection of four markets that are otherwise
separate. Nothing here occupies the same intersection, which is the honest
version of "no direct competitor": every individual capability has a stronger
specialist.

## The four adjacent markets

### 1. Code intelligence / retrieval for agents

> **Correction, 2026-08-05.** An earlier revision of this document claimed the
> named tools do retrieval "better" than Weavatrix. That was written from
> search-result summaries without inspecting the dependency, and it is wrong.
> The paragraphs below are written against `weavatrix-rust` 2.1.1's actual
> source and a live graph build of this repository.

Local, MIT-ish, graph-or-index based: **Serena**, **CodeGraph**, **GitNexus**,
**Repomix**, **grepai**, **Codanna**. Cloud and enterprise: **Sourcegraph
Cody**, **Greptile**, **Augment Context Engine**, **Bito AI Architect**.

**What Weavatrix actually is**, measured rather than assumed:

- ~25 000 lines of Rust exposing **42 native operations**, against which
  Cortex Loom's fixed evidence path used **four**.
- **25 language contracts** with, in the crate's own words, complete lossless
  structural extraction and deterministic cross-file resolution — including
  GraphQL SDL, Protobuf (proto2/proto3/Editions, streaming kinds), Terraform,
  Solidity, Kubernetes manifests.
- **Typed transport contracts**: HTTP, Kafka, AMQP, NATS, JMS, AWS — matching
  producers to consumers across services, not just call graphs within one.
- Architecture contracts and budgets, violation explanation and exception
  proposal; health auditing with clone families, cycles/SCC, dead code,
  coverage and test-evidence verdicts; git history, graph diff and cross-repo
  git; stacktrace→symbol mapping and change-driven test selection; semantic
  and vector search; community detection and shortest paths.
- **Its own deterministic `token_budget`**, with the same `bytes / 4`
  estimator this workspace uses and an honest `dropped_items` / `fit` account
  on every response.
- Measured on this repository, 2026-08-10: **3 539 nodes, 10 263 edges, 98 ms**
  build, incremental refresh on change. As an MCP server (`weavatrix@1.4.0`,
  `--profile=code`) it exposes **41 tools** whose schemas cost **5 276 tokens**
  of session context before any call is made.

Against that, the comparison set is not in the same category. Serena is
symbol-precision LSP-backed navigation; Repomix packs a repository into a
prompt; grepai is semantic grep; CodeGraph and GitNexus build a call/import
graph. **None of them ships typed cross-service transport contracts,
architecture budgets, or a token-budgeted tool protocol.** The cloud tools buy
breadth — tickets, commits, observability — and multi-repo scale, which is a
different axis, not a deeper one.

- What none of them do, Weavatrix included: arbitrate *whether a model should
  be trusted with a decision*. They answer questions; the agent decides.
- Our relationship: **complementary, and asymmetric.** Weavatrix is a
  dependency far larger than the thing depending on it. If we ever ship our
  own indexer we lose. The honest risk is the opposite of what this document
  first implied — not that a competitor out-retrieves Weavatrix, but that
  Weavatrix already covers so much that the layer above it has to justify
  itself on something other than retrieval.

### 2. Agent workflow orchestration

**LangGraph** (+ LangGraph Studio), **Temporal** as the durable-execution
layer underneath it, **OpenAI Agents SDK**, **CrewAI**, **AutoGen**.

- What they do better: production maturity, checkpointing, distributed
  execution, ecosystem, and a visual graph IDE that already exists.
- Where we differ, concretely:
  - LangGraph graphs are **Python code**; ours are a **typed persisted
    document** with optimistic revisions, history, and a Markdown round trip.
    A LangGraph graph is authored by an engineer; ours is authored by whoever
    can edit a `SKILL.md`.
  - Temporal is explicitly *token-blind*. Our whole point is that the control
    plane understands the token budget.
  - Their human-in-the-loop is an interrupt/signal. Ours is a **typed gate
    that refuses generic completion**: an approve/reject decision needs an
    actor, a reason, and same-attempt evidence references, or the command
    fails.
- Honest risk: LangGraph could add evidence budgeting in a quarter. Our moat
  is not the graph; it is the fail-closed evidence and decision semantics.

### 3. Skills / methodology libraries

**obra/superpowers**, Anthropic's official skills repo, gstack, GSD, and the
~15k-repo `SKILL.md` ecosystem.

- What they do better: distribution and mindshare, by orders of magnitude.
- Where we differ: for them a skill is prose the model reads. For us a skill
  **compiles to a validated graph** with provenance per node, and exports back
  to byte-identical Markdown. That makes a skill inspectable, diffable, and
  executable as a run with real state — not a prompt fragment.
- Honest risk: the market may simply not want compiled skills. Prose is
  cheaper to write and the agent is good at reading it. Our answer has to be
  *audit* — a prose skill cannot tell you which step produced which evidence.

### 4. Token / context cost control

**Repomix** (pack a repo under a budget), agent-side context managers such as
SmallCode's eviction and semantic compression, MCP token-efficiency work, and
every "context engineering" playbook.

- What they do better: they are inside the agent loop, so they see the real
  conversation and can evict mid-turn. We only see the evidence we assemble.
- Where we differ: they compress *after* the fact. We select *before*, by
  declared priority, and we refuse to silently drop critical evidence — a
  budget too small for a critical fragment is an error, not a truncation.
- **Where we overlap with our own dependency:** Weavatrix already implements
  deterministic token budgeting with the identical estimator and the same
  omission accounting. Budgeting alone is therefore not a differentiator; the
  differentiator is *which operations get asked*, and the trust semantics on
  what comes back. See [benchmark.md](benchmark.md).
- The measured claim: published 2026 comparisons put MCP-mediated runs at
  roughly 114k tokens against ~27k for equivalent CLI/skill workflows. That is
  the number we have to beat, and it is a warning: an MCP server that dumps
  raw tool output is a cost *centre*. See [benchmark.md](benchmark.md) for our
  own three-arm measurement, which reproduces exactly that failure mode in the
  `weavatrix-raw` arm.

## Where Cortex Loom is actually differentiated

Ranked by how hard each would be for a competitor to copy.

1. **Fail-closed evidence semantics.** Critical evidence cannot be dropped by
   a budget; contradictory evidence sorts first and forces upstream review;
   unverified evidence marks the whole packet `requiresUpstream`. Copying this
   requires adopting the trust model, not just the sort.
2. **Audited run state.** Immutable attempt-scoped evidence, audited
   invalidation that never deletes the record, typed executor leases with lazy
   replay-deterministic expiry, and deterministic replay that only *reports*
   mismatch and never repairs. This is a fair amount of hard-won detail.
3. **Round-trip `SKILL.md` ↔ typed graph** with an export fixpoint. Novel as
   far as this survey found.
4. **Honest accounting.** `omittedEstimatedTokens` is documented as an
   omission volume and explicitly *not* a saving; savings are credited only on
   clean succeeded runs. Most of the market quotes gross reduction. This is a
   marketing disadvantage and a credibility advantage.
5. **Local-first, single binary, no cloud egress.** Shared with the local
   code-graph tools, not unique.

## Where we are behind

- **Distribution.** Superpowers has six figures of stars; we have a private
  repo and four unpublished crates.
- **Language and repo coverage.** Inherited entirely from Weavatrix.
- **Editor maturity.** LangGraph Studio is a mature product; our React/SVG
  editor is less mature.
- **No agent of our own.** OpenCode, Cline, Goose, Aider and Codex CLI all own
  the loop. We are a server they have to be told to call. Every integration is
  a config file someone must write.
- **The core hypothesis is unproven.** "90% less upstream work" is still a
  hypothesis, and the dogfood run showed shadow compression of a real 7.5k
  packet timing out on CPU. Synthetic-fixture latency did not transfer.

## Positioning that survives scrutiny

> Cortex Loom is not a code search tool, an agent, or an orchestration
> framework. It is the **audit and budget layer between them**: it decides what
> evidence an upstream agent is allowed to see, what a local model is allowed
> to decide, and it keeps a record that can be replayed.

Anything stronger than that is not currently supported by measurement.

## Sources

- [Context Engineering: A Practical Guide for AI Agents (2026) — Sourcegraph](https://sourcegraph.com/blog/context-engineering)
- [MCP Token Cost 2026: A Line-Item Autopsy of the Context Tax](https://getunblocked.com/blog/mcp-token-budget-autopsy/)
- [11 Context Engineering Tips to Cut Coding Agent Tokens](https://www.decodingai.com/p/11-context-engineering-tips-cut-coding-agent-tokens)
- [Codanna alternative? Local code-graph MCP servers compared](https://zzet.org/gortex/local-code-graph-mcp-servers-compared/)
- [Code Intelligence Tools for AI Agents Compared — Ry Walker Research](https://rywalker.com/research/code-intelligence-tools)
- [10 Best Graphify Alternatives for AI Codebase Context in 2026](https://www.knolli.ai/post/graphify-alternatives)
- [Graph-Based Agent Workflow Orchestration in Production: The 2026 Landscape](https://zylos.ai/research/2026-04-14-graph-based-agent-workflow-orchestration-production/)
- [Building Durable AI Agents with Temporal](https://niteagent.com/blog/2026-06-29-durable-ai-agents-temporal-guide/)
- [Best Claude Code Skills in 2026 — Firecrawl](https://www.firecrawl.dev/blog/best-claude-code-skills)
- [Locally Hosted Coding Agents: The 2026 Landscape](https://mehmetozgenozdogan.medium.com/locally-hosted-coding-agents-the-2026-landscape-ed652def5989)
- [SmallCode: Fast, Free, Local AI Coding Agent for Small LLMs](https://www.scriptbyai.com/local-small-ai-coding-agent/)
