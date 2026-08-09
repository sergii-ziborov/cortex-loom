# Rust-only Weavatrix Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the legacy Node.js refactor oracle and provide a bounded, read-only refactor-plan preview using first-party Rust crates.

**Architecture:** `cortex-weavatrix` continues to embed `weavatrix-rust` for repository evidence. A new native preview module parses and validates an upstream-authored `RefactorPlan`, verifies repository paths and hashes, prepares exact modify operations in memory with `weavatrix-edit`, and returns structured preview evidence without applying files.

**Tech Stack:** Rust 2024, `weavatrix-rust` 2.3, `weavatrix-refactor-plan`, `weavatrix-edit`, `blazingly-json`, `sha2`, `mcport`.

## Global Constraints

- Backend startup and refactor preview must not discover or execute Node.js, JavaScript, or `.mjs` files.
- Refactor preview is read-only and must not call `weavatrix-worktree` or create confirmation tokens.
- Local models cannot draft an applyable plan; the input plan comes from the upstream agent.
- Existing Weavatrix context compilation remains protocol-independent and native Rust.
- Every Rust source file remains below 500 lines.
- Preserve the public safety statement that Cortex Loom never applies a refactor.
- Required final gates: `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `npm.cmd --prefix ui run build`.

---

### Task 1: Pin the native Weavatrix contracts

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/cortex-weavatrix/Cargo.toml`
- Modify: `crates/cortex-weavatrix/src/lib.rs`

**Interfaces:**
- Consumes: `weavatrix_refactor_plan::{parse_refactor_plan, validate_consumer_plan, fingerprint_plan, RefactorPlanLimits}`.
- Produces: direct dependencies named by responsibility; no transitive use of the old JS package.

- [ ] **Step 1: Write a dependency-surface test**

Add this temporary compile-time test to the existing `lib.rs` test module:

```rust
#[test]
fn first_party_plan_contract_is_available() {
    let limits = weavatrix_refactor_plan::RefactorPlanLimits::default();
    assert!(limits.max_operations > 0);
    assert_eq!(weavatrix_rust::VERSION, "2.3.0");
}
```

- [ ] **Step 2: Run the focused test and confirm the old dependency set fails it**

Run: `cargo test -p cortex-weavatrix first_party_plan_contract_is_available -- --exact`

Expected: compilation fails because `weavatrix-refactor-plan` is not yet a dependency and the embedded Weavatrix version is still 2.2.1.

- [ ] **Step 3: Add exact first-party dependencies**

Add workspace dependencies:

```toml
weavatrix-rust = "2.3.0"
weavatrix-refactor-plan = "0.1.0"
weavatrix-edit = "0.1.7"
```

Add the three workspace dependencies to `crates/cortex-weavatrix/Cargo.toml`, then run:

```powershell
cargo update -p weavatrix-rust --precise 2.3.0
```

- [ ] **Step 4: Run the focused test**

Run: `cargo test -p cortex-weavatrix first_party_plan_contract_is_available -- --exact`

Expected: PASS.

- [ ] **Step 5: Commit the dependency boundary**

```powershell
git add Cargo.toml Cargo.lock crates/cortex-weavatrix/Cargo.toml crates/cortex-weavatrix/src/lib.rs
git commit -m "Use native Weavatrix refactor contracts"
```

### Task 2: Implement bounded native preview

**Files:**
- Create: `crates/cortex-weavatrix/src/refactor_preview.rs`
- Modify: `crates/cortex-weavatrix/src/lib.rs`
- Delete: `crates/cortex-weavatrix/src/adapter.rs`
- Create: `crates/cortex-weavatrix/src/adapter/mod.rs`
- Create: `crates/cortex-weavatrix/src/adapter/evidence.rs`
- Create: `crates/cortex-weavatrix/src/adapter/gather.rs`
- Create: `crates/cortex-weavatrix/src/adapter/tests.rs`
- Test: `crates/cortex-weavatrix/src/refactor_preview.rs`

**Interfaces:**
- Consumes: repository root, a JSON `RefactorPlan`, exact file hashes, and immutable source text.
- Produces: `preview_refactor_plan(repository: &Path, raw_plan: &[u8]) -> Result<RefactorPreview, WeavatrixError>`.

Define these public response types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefactorPreview {
    pub schema_version: String,
    pub fingerprint: String,
    pub operation: String,
    pub completeness: String,
    pub affected_paths: Vec<String>,
    pub changes: Vec<PreviewChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChange {
    pub kind: String,
    pub path: String,
    pub destination: Option<String>,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
}
```

- [ ] **Step 1: Write failing read-only preview tests**

Cover exact modify, stale hash, path escape, create-at-existing-path, delete, rename, and no-write behavior. The core assertion is:

```rust
let before = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
let preview = preview_refactor_plan(root, &encoded_plan).unwrap();
assert_eq!(preview.changes[0].before.as_deref(), Some(before.as_str()));
assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), before);
```

- [ ] **Step 2: Run the focused tests and confirm failure**

Run: `cargo test -p cortex-weavatrix refactor_preview::tests -- --nocapture`

Expected: FAIL because `preview_refactor_plan` and its response types do not exist.

- [ ] **Step 3: Implement parse, validation, containment, and in-memory preparation**

Implementation order:

```rust
let plan = parse_refactor_plan(raw_plan, limits)?;
let validated = validate_consumer_plan(&plan, limits)?;
let fingerprint = fingerprint_plan(validated.plan())?;
let root = repository.canonicalize()?;
// Validate each portable path, confine it to root, verify current hashes,
// and call weavatrix_edit::prepare_edits_with_limits for Modify operations.
```

Existing paths must canonicalize under `root`. Missing create destinations must have an existing canonical parent under `root`. Symlink escapes, stale hashes, unsupported encodings, and oversized previews return a typed `WeavatrixError::InvalidArguments` or `Engine` error. Limit each retained before/after body to 64 KiB and report truncation in `warnings`.

- [ ] **Step 4: Export only the native preview API**

Add to `lib.rs`:

```rust
mod refactor_preview;
pub use refactor_preview::{PreviewChange, RefactorPreview, preview_refactor_plan};
```

Change `WeavatrixAdapter::preview_refactor` to accept `plan: &Value`, serialize it with `blazingly-json`, and call `preview_refactor_plan`. Remove `RefactorOperation` from the public API.

- [ ] **Step 5: Split the existing oversized adapter by responsibility**

Move evidence response types and fragment helpers to `adapter/evidence.rs`, native session and gather methods to `adapter/gather.rs`, the public adapter/config/error surface to `adapter/mod.rs`, and the existing adapter tests to `adapter/tests.rs`. Do not change context-gathering behavior during the move. Each resulting Rust file must be below 500 lines.

- [ ] **Step 6: Run crate tests**

Run: `cargo test -p cortex-weavatrix`

Expected: all tests pass, including stale-hash and no-write assertions.

- [ ] **Step 7: Commit native preview**

```powershell
git add crates/cortex-weavatrix/src
git commit -m "Preview Weavatrix refactor plans in Rust"
```

### Task 3: Remove the Node transport and migrate MCP

**Files:**
- Delete: `crates/cortex-weavatrix/src/transport.rs`
- Modify: `crates/cortex-weavatrix/src/adapter/mod.rs`
- Modify: `crates/cortex-weavatrix/src/lib.rs`
- Modify: `crates/cortex-mcp/src/lib.rs`
- Test: inline test module in `crates/cortex-mcp/src/lib.rs`

**Interfaces:**
- Consumes: MCP request `{ repository, plan }`.
- Produces: `weavatrix_refactor_preview` result `{ mode: "preview", preview: RefactorPreview }`.

- [ ] **Step 1: Write a failing MCP contract test**

```rust
assert_eq!(schema["properties"]["plan"]["type"], "object");
assert!(schema["properties"].get("operation").is_none());
assert!(schema["properties"].get("arguments").is_none());
```

Add an integration call with a valid minimal plan and assert the response has no `confirmationToken`, `apply`, or `rollback` field.

- [ ] **Step 2: Run the MCP tests and confirm the old schema fails**

Run: `cargo test -p cortex-mcp weavatrix_refactor_preview -- --nocapture`

Expected: FAIL because the tool still advertises `operation` and `arguments`.

- [ ] **Step 3: Replace the tool schema and handler**

Use this input shape:

```json
{
  "type": "object",
  "required": ["repository", "plan"],
  "properties": {
    "repository": { "type": "string" },
    "plan": { "type": "object" }
  },
  "additionalProperties": false
}
```

Keep the tool name stable and describe the breaking input migration in the tool description. Return validation failures as `isError: true` without process spawning.

- [ ] **Step 4: Delete legacy configuration and transport**

Remove `McpCommand`, `McpChild`, `McpError`, `program`, `refactor_script`, `CORTEX_LOOM_WEAVATRIX_COMMAND`, `CORTEX_LOOM_REFACTOR_SCRIPT`, and `discover_refactor_script`. Delete `transport.rs` only after `rg` proves no remaining caller.

- [ ] **Step 5: Verify absence of executable JS discovery**

Run:

```powershell
rg -n -i "weavatrix-refactor-mcp|CORTEX_LOOM_REFACTOR_SCRIPT|CORTEX_LOOM_WEAVATRIX_COMMAND|Command::new\(.*node|\.mjs" crates apps
```

Expected: no matches in Rust backend sources. The server MIME mapping for UI JavaScript is allowed.

- [ ] **Step 6: Run focused crates and commit**

Run: `cargo test -p cortex-weavatrix -p cortex-mcp`

```powershell
git add crates/cortex-weavatrix crates/cortex-mcp
git commit -m "Remove the JavaScript refactor oracle"
```

### Task 4: Reconcile architecture docs and full gates

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/research.md`
- Modify: `docs/benchmark.md`

**Interfaces:**
- Consumes: the verified native preview implementation.
- Produces: documentation that describes the current Rust boundary without claiming native symbol planning.

- [ ] **Step 1: Update the documented boundary**

State all three facts together: repository intelligence is native Rust; preview validates upstream-authored plans; no native symbol planner or apply path is claimed.

- [ ] **Step 2: Check stale legacy claims**

Run:

```powershell
rg -n -i "JavaScript compatibility oracle|weavatrix-refactor-mcp|preview-only JavaScript" docs crates apps
```

Expected: matches exist only in historical benchmark/design discussion explicitly labelled historical.

- [ ] **Step 3: Run all required gates**

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm.cmd --prefix ui run build
npm.cmd --prefix ui test -- --run
```

Expected: every command exits 0.

- [ ] **Step 4: Commit documentation and gate evidence**

```powershell
git add docs
git commit -m "Document the Rust-only Weavatrix boundary"
```
