# Editable Cortex Sequences Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship seven Cortex-native sequence templates that users can copy, edit, validate, run, reopen, compare, and export without a runtime dependency on Superpowers.

**Architecture:** A new protocol-independent `cortex-sequences` crate owns immutable templates, copy metadata, lint, deterministic activation hints, and active-step packets. Existing store, server, MCP, and React layers expose that core without duplicating sequence policy.

**Tech Stack:** Rust 2024, `cortex-domain`, `cortex-skills`, `cortex-store`, `blazingly-json`, `sha2`, Axum, mcport, React 18, TypeScript, Vitest.

## Global Constraints

- Superpowers is an evaluation source only; no runtime import, discovery, prose copy, or plugin dependency.
- Built-in templates are immutable; a user copy contains its complete graph by value and is independently editable.
- Template updates never overwrite a user graph.
- Only the active step enters methodology context.
- High-risk or mutating nodes cannot target local models.
- Source files remain below 500 lines.
- Preserve existing `cortex-skills::bundled_skills()` behavior for consumers.
- Required final gates: Rust format/test/clippy, UI build, UI tests, and stable Markdown round trips.

---

### Task 1: Create the protocol-independent sequence crate

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/cortex-sequences/Cargo.toml`
- Create: `crates/cortex-sequences/src/lib.rs`
- Create: `crates/cortex-sequences/src/template.rs`
- Create: `crates/cortex-sequences/src/error.rs`
- Create: `crates/cortex-sequences/src/tests.rs`

**Interfaces:**
- Consumes: `cortex_domain::GraphDocument` and `cortex_skills::import_skill_markdown`.
- Produces: `TemplateVersion`, `SequenceTemplate`, `TemplateRef`, `templates()`, and `instantiate_template()`.

Define the public contracts exactly:

```rust
pub struct TemplateVersion { pub major: u16, pub minor: u16, pub patch: u16 }

pub struct SequenceTemplate {
    pub id: &'static str,
    pub version: TemplateVersion,
    pub title: &'static str,
    pub description: &'static str,
    pub markdown: &'static str,
    pub changelog: &'static str,
    pub activation: ActivationHints,
}

pub fn templates() -> &'static [SequenceTemplate];
pub fn instantiate_template(
    template_id: &str,
    graph_id: &str,
    name: &str,
) -> Result<GraphDocument, SequenceError>;
```

- [ ] **Step 1: Write failing catalog and copy tests**

```rust
#[test]
fn a_copy_is_editable_and_detached_from_its_template() {
    let graph = instantiate_template("discover-and-plan", "my-plan", "My plan").unwrap();
    assert_eq!(graph.id, "my-plan");
    assert_eq!(graph.metadata["sequence.templateId"], "discover-and-plan");
    assert_eq!(graph.metadata["sequence.editable"], "true");
    assert_eq!(graph.revision, 0);
}
```

Also assert unique template IDs, ordered versions, and stable fingerprints.

- [ ] **Step 2: Run and confirm failure**

Run: `cargo test -p cortex-sequences`

Expected: FAIL because the crate and contracts do not exist.

- [ ] **Step 3: Add the crate and minimal template machinery**

Register the workspace member. Compile template Markdown through `cortex-skills`, replace graph identity and title, set revision zero, and add:

```text
sequence.templateId
sequence.templateVersion
sequence.templateFingerprint
sequence.editable=true
```

Calculate the fingerprint from stable exported Markdown bytes using SHA-256.

- [ ] **Step 4: Run crate tests and commit**

Run: `cargo test -p cortex-sequences`

```powershell
git add Cargo.toml Cargo.lock crates/cortex-sequences
git commit -m "Add the Cortex sequence template core"
```

### Task 2: Author the seven native templates

**Files:**
- Create: `crates/cortex-sequences/templates/discover-and-plan.md`
- Create: `crates/cortex-sequences/templates/bounded-implementation.md`
- Create: `crates/cortex-sequences/templates/root-cause-debugging.md`
- Create: `crates/cortex-sequences/templates/review-and-correct.md`
- Create: `crates/cortex-sequences/templates/verify-and-integrate.md`
- Create: `crates/cortex-sequences/templates/parallel-investigation.md`
- Create: `crates/cortex-sequences/templates/sequence-authoring.md`
- Create: `crates/cortex-sequences/src/catalog.rs`
- Modify: `crates/cortex-sequences/src/lib.rs`
- Test: `crates/cortex-sequences/src/tests.rs`

**Interfaces:**
- Consumes: typed `[kind: ...]` annotations and explicit graph transitions supported by `cortex-skills`.
- Produces: seven complete, original Cortex workflows with evidence gates, escalation, and terminals.

- [ ] **Step 1: Write structural pressure tests**

For every template assert: at least one evidence/test/review gate, a terminal, an upstream/handoff path, no Superpowers source path, and an export-import fixpoint.

```rust
for template in templates() {
    let graph = instantiate_template(template.id, "copy", template.title).unwrap();
    assert!(graph.nodes.iter().any(|node| node.kind == NodeKind::Terminal));
    assert!(graph.nodes.iter().any(|node| matches!(node.kind, NodeKind::UpstreamAgent | NodeKind::Handoff)));
    assert!(!template.markdown.contains("superpowers/"));
}
```

- [ ] **Step 2: Run and confirm missing-template failure**

Run: `cargo test -p cortex-sequences catalog -- --nocapture`

Expected: FAIL because all seven IDs are not registered.

- [ ] **Step 3: Write original Cortex-native workflows**

Each template must use active, short instructions and Cortex concepts: Weavatrix evidence, one bounded recovery, explicit sufficiency, risk-aware model lane, and upstream fallback. Do not translate upstream paragraphs. Keep each template under 140 lines.

- [ ] **Step 4: Verify round trips and commit**

Run: `cargo test -p cortex-sequences`

```powershell
git add crates/cortex-sequences
git commit -m "Add seven Cortex-native sequences"
```

### Task 3: Add lint and active-step packets

**Files:**
- Create: `crates/cortex-sequences/src/lint.rs`
- Create: `crates/cortex-sequences/src/packet.rs`
- Modify: `crates/cortex-sequences/src/lib.rs`
- Test: `crates/cortex-sequences/src/tests.rs`

**Interfaces:**
- Produces: `lint_sequence(&GraphDocument) -> Vec<SequenceDiagnostic>`.
- Produces: `active_step_packet(&GraphDocument, node_id: &str, evidence_ids: &[String]) -> Result<ActiveStepPacket, SequenceError>`.

```rust
pub struct SequenceDiagnostic {
    pub code: DiagnosticCode,
    pub node_id: Option<String>,
    pub message: String,
    pub severity: DiagnosticSeverity,
}

pub struct ActiveStepPacket {
    pub graph_id: String,
    pub graph_revision: u64,
    pub node_id: String,
    pub instruction: String,
    pub required_evidence: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub completion_criteria: Vec<String>,
    pub max_input_tokens: u32,
    pub max_attempts: u32,
    pub executor: String,
    pub success_edges: Vec<String>,
    pub recovery_edges: Vec<String>,
    pub escalation_edges: Vec<String>,
}
```

- [ ] **Step 1: Write one failing test per lint invariant**

Cover unreachable executable node, missing terminal, executable cycle, unbounded retry, gate without failure/escalation, local-model mutation/high risk, branch without choices, missing completion criteria, and external node reference.

- [ ] **Step 2: Write the active-step disclosure test**

```rust
let packet = active_step_packet(&graph, "step-2", &["WX-1".into()]).unwrap();
assert!(packet.instruction.contains("Gather"));
assert!(!packet.instruction.contains("Finish the branch"));
assert_eq!(packet.evidence_ids, ["WX-1"]);
```

- [ ] **Step 3: Run and confirm failures**

Run: `cargo test -p cortex-sequences lint packet -- --nocapture`

Expected: FAIL because diagnostics and packet construction are absent.

- [ ] **Step 4: Implement deterministic traversal and policy checks**

Use graph IDs and typed node/edge kinds only. Never infer safety from prose. Read `completionCriteria`, `requiredEvidence`, `maxInputTokens`, and `maxAttempts` from node config with bounded defaults; invalid types produce diagnostics instead of coercion.

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p cortex-sequences`

```powershell
git add crates/cortex-sequences/src
git commit -m "Validate and compile active sequence steps"
```

### Task 4: Expose templates through store, HTTP, and MCP

**Files:**
- Modify: `apps/cortex-server/src/library.rs`
- Create: `apps/cortex-server/src/sequences.rs`
- Modify: `apps/cortex-server/src/main.rs`
- Modify: `crates/cortex-mcp/src/lib.rs`
- Modify: `crates/cortex-store/src/lib.rs`
- Test: inline test modules in `apps/cortex-server/src/sequences.rs`
- Test: inline test module in `crates/cortex-mcp/src/lib.rs`

**Interfaces:**
- HTTP: `GET /api/sequences/templates`.
- HTTP: `GET /api/sequences/templates/{id}`.
- HTTP: `POST /api/sequences/templates/{id}/copy` with `{ graphId, name }`.
- HTTP: `POST /api/sequences/lint` with a graph document.
- HTTP: `POST /api/sequences/active-step` with `{ graphId, nodeId, evidenceIds }`.
- MCP: `sequence_list`, `sequence_copy`, `sequence_lint`, `sequence_step_read`.

- [ ] **Step 1: Write failing API and MCP tests**

Assert the template list returns seven immutable summaries, copy stores only when missing, a second copy cannot overwrite an edited graph, and active-step output excludes inactive node prose.

- [ ] **Step 2: Run focused tests and confirm 404/tool absence**

Run: `cargo test -p cortex-server -p cortex-mcp sequence_ -- --nocapture`

Expected: FAIL because routes and tools are absent.

- [ ] **Step 3: Compose the protocol-independent crate**

Handlers call `cortex-sequences` and `GraphStore::seed_if_missing`; they do not recreate lint or packet logic. Return revision conflict/store errors without retrying writes.

- [ ] **Step 4: Run focused tests and commit**

Run: `cargo test -p cortex-store -p cortex-server -p cortex-mcp`

```powershell
git add apps/cortex-server crates/cortex-mcp crates/cortex-store Cargo.toml Cargo.lock
git commit -m "Expose editable sequence templates"
```

### Task 5: Upgrade the user-facing sequence editor

**Files:**
- Modify: `ui/src/types.ts`
- Modify: `ui/src/api/client.ts`
- Modify: `ui/src/components/LibraryPanel.tsx`
- Modify: `ui/src/components/LibraryDialog.tsx`
- Create: `ui/src/components/SequenceTemplatePanel.tsx`
- Create: `ui/src/components/SequenceDiagnostics.tsx`
- Modify: `ui/src/components/NodeInspector.tsx`
- Modify: `ui/src/styles.css`
- Test: `ui/src/model/sequence.test.ts`

**Interfaces:**
- Consumes: template summaries, user graphs, lint diagnostics, and active-step previews.
- Produces: `Cortex templates` and `My sequences` tabs plus `Use and edit`, `Test sequence`, and `Compare` actions.

- [ ] **Step 1: Write failing UI model tests**

Test that templates and user graphs remain separate, `Use and edit` sends a new `graphId`, diagnostics group by node, and safety errors disable `Run` but not `Save draft`.

- [ ] **Step 2: Run tests and confirm component/model absence**

Run: `npm.cmd --prefix ui test -- --run`

Expected: FAIL because the sequence model and panels do not exist.

- [ ] **Step 3: Implement non-technical editing controls**

Expose task-oriented fields: instruction, proof required, success criteria, executor, input budget, attempts, and failure route. Keep raw JSON under an `Advanced` disclosure. Built-in cards use `Preview` and `Use and edit`; no direct edit action is shown for templates.

- [ ] **Step 4: Add compare without automatic merge**

Compare canonical node IDs, labels, kinds, config, and edges. Present `added`, `changed`, and `removed` counts and let the user copy selected values into the ordinary editor; the API never applies a template diff automatically.

- [ ] **Step 5: Run UI gates and commit**

```powershell
npm.cmd --prefix ui test -- --run
npm.cmd --prefix ui run build
```

```powershell
git add ui
git commit -m "Add editable sequence workflows to the UI"
```

### Task 6: Full compatibility and acceptance gates

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/roadmap.md`
- Modify: `crates/cortex-skills/README.md`
- Modify: `ui/README.md`

**Interfaces:**
- Consumes: completed sequence core and UI.
- Produces: documented user flow and compatibility boundary.

- [ ] **Step 1: Prove the existing 31-skill API remains intact**

Run: `cargo test -p cortex-skills every_bundled_skill_compiles_and_has_a_unique_graph_id -- --exact`

Expected: PASS with 31 unique bundled skills.

- [ ] **Step 2: Run all required gates**

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm.cmd --prefix ui test -- --run
npm.cmd --prefix ui run build
```

Expected: every command exits 0.

- [ ] **Step 3: Verify file-size and dependency boundaries**

```powershell
Get-ChildItem crates,apps -Recurse -File -Filter '*.rs' | Where-Object { (Get-Content $_.FullName).Count -ge 500 } | Select-Object FullName
rg -n -i "superpowers|weavatrix-refactor-mcp|\.mjs" crates/cortex-sequences crates/cortex-weavatrix apps/cortex-server
```

Expected: no Rust source at or above 500 lines in modified/new modules; no product dependency or JS backend reference.

- [ ] **Step 4: Commit documentation**

```powershell
git add docs crates/cortex-skills/README.md ui/README.md
git commit -m "Document Cortex-native editable sequences"
```
