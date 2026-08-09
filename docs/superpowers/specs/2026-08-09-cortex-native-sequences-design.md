# Cortex-native editable sequences

Date: 2026-08-09

## Decision

Cortex Loom will not execute, vendor, or depend on Superpowers. Superpowers is
an evaluation source: useful workflow mechanics may inspire Cortex-native
sequences only after they are adapted to Cortex safety, evidence, routing, and
token-budget rules and measured against the existing Cortex baseline.

The product will ship independently authored, versioned sequence templates.
Users create editable copies of those templates. A copied sequence has no
runtime link to its template and is never overwritten by a template update.

## Goals

- Give users editable methodology workflows instead of injecting a long skill
  prompt into every agent turn.
- Preserve the useful parts of Superpowers without inheriting universal hard
  gates that reduce accuracy on small or evidence-poor tasks.
- Reuse first-party Weavatrix, Blazingly, and Cortex crates where they already
  own a capability.
- Route deterministic work and bounded classification away from the strong
  upstream agent without granting weak models mutation or approval authority.
- Measure sequence quality, escalation safety, token cost, and latency before
  promoting a template or model profile.

## Non-goals

- Importing or synchronizing an installed Superpowers plugin.
- Copying upstream Superpowers prose into Cortex fixtures.
- Requiring brainstorming, TDD, worktrees, subagents, or review for every task.
- Letting a local model apply Weavatrix Refactor plans, merge branches, approve
  releases, or make high-risk decisions.
- Replacing mature MIT or Apache dependencies merely to make them first-party.
- Keeping a Node.js or JavaScript backend compatibility path. TypeScript is a
  UI implementation detail only; repository intelligence, sequence execution,
  routing, storage, and refactor preview stay in Rust.

## Package boundaries

### `cortex-skills`

Remains the portable Markdown-to-graph and graph-to-Markdown compiler. It owns
syntax, provenance, stable round trips, and library import. It does not own the
product methodology catalog or execution policy.

### `cortex-sequences`

A new protocol-independent Rust crate that owns:

- the Cortex-native template catalog;
- template identifiers, semantic versions, and release notes;
- creation of an independent editable graph from a template;
- sequence lint and safety diagnostics;
- active-step packet construction;
- deterministic activation hints and candidate scoring inputs.

It depends on `cortex-domain` and `cortex-skills`. It does not depend on MCP,
HTTP, React, Ollama, Superpowers, or filesystem transports.

### Existing owners

- `cortex-run` executes revisioned run graphs and their transitions.
- `cortex-router` enforces risk and model-lane policy.
- `cortex-weavatrix` embeds `weavatrix-rust` for graph, source, search, Git,
  memory, and impact evidence and uses the native refactor-plan contracts for
  preview validation.
- `cortex-store` persists user graphs and optimistic revisions.
- `cortex-mcp` and `cortex-server` expose transport adapters only.
- The existing UI edits canonical graphs but never becomes their owner.

No additional orchestration crate is introduced in the first iteration.

## First-party dependency policy

Prefer an existing package owned by `sergii-ziborov` when it already provides
the required contract:

- `weavatrix-git` for bounded read-only Git evidence;
- `weavatrix-search` for deterministic repository search;
- `weavatrix-semantic` and `weavatrix-search-vector` for semantic candidates;
- `weavatrix-memory` for temporal evidence and retrieval;
- `weavatrix-edit`, `weavatrix-refactor-plan`, and `weavatrix-worktree` for
  exact previewed edit contracts and recoverable execution boundaries;
- `mcport`, `blazingly-json`, and Blazingly for MCP/API transport contracts.

An external MIT or Apache dependency remains acceptable when no first-party
package owns the capability and rewriting it would not improve the product.
Every new dependency must have one named responsibility and must not bypass
Cortex authority or evidence gates.

## Rust-only Weavatrix boundary

The current evidence path already embeds `weavatrix-rust`, but the optional
`weavatrix_refactor_preview` adapter still discovers a Node.js program and the
legacy `weavatrix-refactor-mcp.mjs`. That compatibility oracle is removed.

The native replacement composes the current first-party Rust contracts:

- `weavatrix-rust` provides read-only repository evidence;
- `weavatrix-refactor-plan` parses, bounds, validates, canonicalizes, and
  fingerprints an evidence-carrying plan;
- `weavatrix-edit` prepares exact in-memory text edits for preview;
- `weavatrix-worktree` remains the separately owned execution boundary but is
  not called by Cortex Loom because apply remains outside the product scope.

There is no native first-party symbol planner equivalent to the old JavaScript
rename/signature/move planner in the inspected Rust packages. Cortex therefore
does not pretend to preserve that generation capability. A strong upstream
agent drafts a `RefactorPlan` from cited Weavatrix evidence; Cortex validates
and previews that plan in Rust. Local models cannot draft an applyable plan.

The old `operation + arguments` MCP preview request is replaced by a bounded
native plan envelope. Preview returns validation status, canonical fingerprint,
evidence/completeness diagnostics, affected paths, and in-memory diffs where
the plan provides exact source edits. It never emits or accepts a confirmation
token and never writes the repository.

## Template and user-copy lifecycle

Built-in templates are immutable release artifacts. Each carries:

- `templateId`;
- `templateVersion`;
- `title` and `description`;
- activation hints;
- changelog text;
- a canonical graph fingerprint.

`Use and edit` creates a normal revision-zero graph with provenance metadata
recording the source template and version. All nodes, labels, gates, model
lanes, budgets, transitions, and escalation paths in the copy are editable.

The copy contains all required nodes and edges by value. It has no subflow or
runtime dependency on the built-in template. Updating Cortex can add a newer
template version but cannot mutate the copy. The UI may compare the copy with
the latest template and apply explicitly selected changes through an ordinary
revision-checked graph save.

## Initial sequence catalog

### `discover-and-plan`

Collects repository evidence before choosing a design. It asks a user question
only when evidence cannot resolve a material choice. A design approval gate is
used for architectural or expansive work, not for trivial edits.

### `bounded-implementation`

Executes a reviewed plan in small batches with checkpoints. TDD is selected
when a behavior has a testable contract; documentation, metadata, and purely
mechanical changes use proportionate verification instead.

### `root-cause-debugging`

Captures a reproduction, gathers source and change-history evidence, tests one
falsifiable hypothesis at a time, and escalates after a bounded number of
failed hypotheses. A passing symptom without a proven cause is not completion.

### `review-and-correct`

Separates requirement compliance from code-quality review. Review feedback is
verified against source and tests before being applied. Unclear or technically
incorrect feedback is returned with evidence instead of being accepted
performatively.

### `verify-and-integrate`

Maps acceptance criteria to fresh checks, records skipped or unavailable
proof, and prevents unsupported completion claims. Merge, pull request, keep,
or discard remains an explicit human choice; destructive actions are never
selected by a local model.

### `parallel-investigation`

Splits only independent evidence-gathering or review tasks. Concurrency is
bounded, each branch has an explicit output contract, and one aggregation gate
checks contradictions and missing coverage before proceeding.

### `sequence-authoring`

Creates or modifies a sequence through scenario-first pressure tests, graph
lint, Markdown round-trip checks, and an execution dry run. It is the native
replacement for treating skill prose as executable policy.

## Activation and context flow

Manual selection is authoritative. Automatic selection starts in shadow and
recommendation mode.

1. Deterministic task features produce a bounded candidate set.
2. Existing semantic retrieval may reorder candidates but cannot add an
   unqualified sequence.
3. An optional calibrated small instruct model may classify among the bounded
   candidates using schema-constrained output.
4. Low confidence, invalid output, or a high-risk task falls back to the
   deterministic result or the upstream agent.
5. Only a manually selected or promoted policy-selected sequence becomes the
   active run graph.

The agent never receives the complete sequence on every turn. The active node
produces an `ActiveStepPacket` containing:

- the current instruction;
- required inputs and evidence classes;
- cited evidence identifiers;
- completion criteria;
- token and attempt budgets;
- allowed executor lane;
- success, recovery, and escalation transitions.

This packet is the only methodology text added to the model context. Stable
graph metadata and inactive guidance stay out of the prompt.

## Model lanes

- Deterministic Rust and Weavatrix handle parsing, search, graph traversal,
  evidence validation, and policy checks.
- `qwen3-embedding:0.6b`, once its existing retrieval gate is active, may rank
  candidates within an already qualified band. It is not an instruct model.
- A small instruct model may perform bounded classification or extraction only
  after passing the sequence-selection calibration suite.
- The strong upstream coding agent handles ambiguity, contradictory evidence,
  high risk, design decisions, and mutation-authorized work.

Local-model output is advisory. Schema validity, confidence, and calibration
never override the domain rule that high-risk or mutating work requires an
upstream agent or human.

## Lint and failure handling

A user sequence cannot be saved as runnable when it has:

- unreachable executable nodes;
- no terminal path;
- an unbounded retry or executable cycle;
- a gate without a failure or escalation path;
- a local-model mutation or high-risk policy;
- a branch without explicit selectable transitions;
- missing active-step completion criteria;
- references to nodes outside the copied graph.

Missing evidence triggers at most one bounded source-recovery pass when the
active node permits it. A still-thin packet sets `requiresUpstream` and follows
the explicit escalation edge. Invalid or low-confidence small-model output
falls back without changing the run. Stale graph revisions return a conflict
and never overwrite a newer edit.

## User interface

The library distinguishes `Cortex templates` from `My sequences`.

Template actions:

- Preview;
- Use and edit;
- Compare a user copy with the latest template version.

User-sequence actions:

- Edit the graph and Markdown view;
- configure executor lane, evidence requirement, budget, and retry limit;
- run lint and an execution dry run;
- export `SKILL.md`;
- view the source template version without restoring it automatically.

The inspector uses task-oriented labels rather than exposing raw JSON for
ordinary edits. Safety diagnostics identify the affected node and the required
repair. Advanced metadata remains available but is not the primary interface.

## Evaluation

The sequence benchmark has four arms:

1. no methodology sequence;
2. the current Cortex skill behavior;
3. raw Superpowers guidance as a control arm;
4. the Cortex-native editable sequence.

The deterministic suite measures candidate selection, expected evidence
classes, graph coverage, required gates, unnecessary gates, escalation paths,
round-trip stability, and prompt-token overhead. A shadow live-model suite
measures task correctness where deterministic anchor recall is insufficient.

Reported metrics include:

- answer and plan correctness;
- anchor/evidence recall;
- unnecessary-step and missing-step rates;
- high-risk escalation recall and unnecessary-escalation rate;
- unsupported completion claims;
- upstream and total prompt tokens;
- end-to-end latency;
- small-model schema validity and selection accuracy.

A template is not promoted by default when it reduces task correctness or
evidence recall versus the current Cortex arm. High-risk missed escalations and
unsupported successful-completion claims must be zero in the promotion set.
The added active-step methodology context must remain bounded and be reported
separately from repository evidence tokens.

## Rollout

1. Add `cortex-sequences`, its template contract, lint, and tests.
2. Remove the Node.js refactor oracle, upgrade the embedded Weavatrix Rust
   dependency, and expose native plan validation and preview without apply.
3. Move the seven Cortex-native templates into the new catalog while retaining
   compatibility for existing `cortex-skills::bundled_skills()` consumers.
4. Add template-copy and active-step-packet APIs through the existing server
   and MCP composition layers.
5. Upgrade the library/editor UI for template copying, diagnostics, and model
   lanes.
6. Add the four-arm deterministic benchmark and shadow model hooks.
7. Run the existing context probe plus the new sequence benchmark twice and
   compare stable reports before enabling automatic recommendations.

## Acceptance criteria

- Cortex has no build, runtime, or source-discovery dependency on Superpowers.
- Cortex backend startup and refactor preview do not discover or execute Node,
  JavaScript, or `.mjs` files.
- Refactor preview parses and validates the first-party Rust plan contract and
  cannot mutate repository files.
- All seven templates compile into valid typed graphs and survive stable
  Markdown round trips.
- A user can create, edit, save, reopen, and export an independent sequence.
- Updating a template does not change an existing user copy.
- Only the active step is included in the methodology context packet.
- Unsafe model-lane edits are rejected by protocol-independent validation.
- Selection remains recommendation-only until its benchmark promotion gate
  passes.
- The required Rust, UI build, UI tests, and both benchmark reruns pass before
  the implementation is reported complete.
