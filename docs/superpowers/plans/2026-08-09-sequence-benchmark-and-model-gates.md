# Sequence Benchmark and Model Gates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure Cortex-native sequences against current Cortex behavior and raw Superpowers guidance, then allow small models only where they produce a strict non-regression result.

**Architecture:** `cortex-bench` gains a deterministic four-arm sequence suite with per-case Pareto gates and explicit external Superpowers input. `cortex-eval` gains an optional shadow live-model suite. `cortex-llm` adds a `micro_extract` role that cannot route, judge sufficiency, compress evidence, plan changes, or mutate state.

**Tech Stack:** Rust 2024, `cortex-bench`, `cortex-eval`, `cortex-sequences`, `cortex-skills`, `cortex-llm`, `cortex-router`, OpenAI-compatible OVMS, Ollama, JSON fixtures.

## Global Constraints

- Existing context benchmark reports are retrieval measurements and must not be relabelled as Superpowers comparisons.
- Raw Superpowers is an explicit benchmark input path, never a runtime dependency or vendored source.
- Quality and safety are hard gates: one new missed required fact, high-risk escalation, or unsupported completion blocks promotion.
- Token savings cannot compensate for a quality regression.
- A model/profile is identified by exact model, quantization, device, runtime, prompt version, and schema version.
- Small-model output is advisory and schema-constrained.
- `micro_extract` accepts verified evidence only and cannot influence routing authority.
- Benchmark reports are deterministic where no model is invoked and record hashes for every external input.

---

### Task 1: Define sequence scenarios and the hard gate

**Files:**
- Create: `crates/cortex-bench/fixtures/sequence-probes.json`
- Create: `crates/cortex-bench/src/sequence.rs`
- Modify: `crates/cortex-bench/src/lib.rs`
- Modify: `crates/cortex-bench/src/main.rs`
- Test: `crates/cortex-bench/src/sequence.rs`

**Interfaces:**
- Produces: `SequenceScenario`, `ExpectedSequenceBehavior`, `SequenceArmResult`, and `SequenceGate`.

```rust
pub struct ExpectedSequenceBehavior {
    pub sequence_id: String,
    pub required_node_kinds: Vec<NodeKind>,
    pub forbidden_node_kinds: Vec<NodeKind>,
    pub required_evidence: Vec<String>,
    pub must_escalate: bool,
    pub must_not_claim_completion: bool,
}

pub struct SequenceGate {
    pub quality_passed: bool,
    pub safety_passed: bool,
    pub cost_passed: bool,
    pub promoted: bool,
    pub regressions: Vec<String>,
}
```

- [ ] **Step 1: Write at least 28 scenario fixtures**

Use two scenarios per installed Superpowers mechanic and include: trivial docs edit, API change, config/env flag, concurrency bug, review feedback, release, dirty worktree, independent investigation, failed verification, and sequence authoring. Each scenario names exact required and forbidden behavior.

- [ ] **Step 2: Write failing per-case Pareto tests**

```rust
#[test]
fn one_lost_required_fact_blocks_promotion() {
    let gate = compare(adapted_with_recall(3), baseline_with_recall(4));
    assert!(!gate.promoted);
    assert!(gate.regressions.iter().any(|item| item.contains("required fact")));
}
```

Also assert that one missed high-risk escalation, one unsupported completion, higher total prompt tokens than the current Cortex arm, or p95 above the configured hot-path SLA blocks default promotion.

- [ ] **Step 3: Run and confirm failure**

Run: `cargo test -p cortex-bench sequence::tests -- --nocapture`

Expected: FAIL because the scenario types and comparison function do not exist.

- [ ] **Step 4: Implement deterministic scoring**

Score each requirement independently; never average away a failed case. `promoted` is the conjunction of quality, safety, and cost gates. Reports retain every regression string and do not emit a single blended quality score as the verdict.

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p cortex-bench sequence::tests`

```powershell
git add crates/cortex-bench
git commit -m "Define strict sequence benchmark gates"
```

### Task 2: Add the four benchmark arms

**Files:**
- Create: `crates/cortex-bench/src/sequence_arms.rs`
- Modify: `crates/cortex-bench/src/sequence.rs`
- Modify: `crates/cortex-bench/src/main.rs`
- Test: `crates/cortex-bench/src/sequence_arms.rs`

**Interfaces:**
- CLI: `cortex-bench sequence --superpowers-root <path> --output <report.json>`.
- Arms: `none`, `cortex-current`, `superpowers-raw`, `cortex-native`.

- [ ] **Step 1: Write failing arm-identity tests**

Assert all four arms are present in stable order and the raw arm reports `available=false` with an explicit reason when no root is supplied.

- [ ] **Step 2: Add explicit Superpowers input loading**

The loader walks only `<root>/skills/*/SKILL.md`, applies the existing library bounds, reads `LICENSE`, and records:

```rust
pub struct ExternalLibraryStamp {
    pub root_label: String,
    pub version: Option<String>,
    pub license_sha256: String,
    pub skill_sha256: BTreeMap<String, String>,
}
```

It must not search user directories or installed plugins automatically.

- [ ] **Step 3: Render comparable methodology packets**

- `none`: empty methodology packet.
- `cortex-current`: full currently selected bundled-skill guidance.
- `superpowers-raw`: full selected upstream SKILL.md body.
- `cortex-native`: only the `ActiveStepPacket` for the relevant node.

Use the same repository evidence packet for all arms so methodology is the only variable.

- [ ] **Step 4: Run deterministic stability tests**

Run: `cargo test -p cortex-bench sequence_arms -- --nocapture`

Expected: two runs with identical fixture and external hashes serialize byte-for-byte identically.

- [ ] **Step 5: Commit four-arm support**

```powershell
git add crates/cortex-bench
git commit -m "Benchmark Cortex sequences against raw skills"
```

### Task 3: Add sequence selection without a trust-model shortcut

**Files:**
- Create: `crates/cortex-sequences/src/activation.rs`
- Modify: `crates/cortex-sequences/src/template.rs`
- Modify: `crates/cortex-mcp/src/semantic.rs`
- Test: `crates/cortex-sequences/src/tests.rs`

**Interfaces:**
- Produces: `candidate_templates(task: &str) -> Vec<SequenceCandidate>`.
- Produces: optional embedding reranking inside the deterministic candidate set.

```rust
pub struct SequenceCandidate {
    pub template_id: String,
    pub deterministic_score: u16,
    pub matched_hints: Vec<String>,
}
```

- [ ] **Step 1: Write selection tests for all 28 scenarios**

Assert the expected template is present in the deterministic candidate set and every high-risk task retains upstream execution regardless of sequence score.

- [ ] **Step 2: Run and confirm failure**

Run: `cargo test -p cortex-sequences activation -- --nocapture`

Expected: FAIL because activation scoring is absent.

- [ ] **Step 3: Implement bounded deterministic candidates**

Use declared activation hints only: task class, intent, risk, mutation, evidence class, and explicit lexical cues. Return at most three candidates. Embeddings may reorder those candidates but cannot introduce a fourth or change executor authority.

- [ ] **Step 4: Run tests and commit**

Run: `cargo test -p cortex-sequences -p cortex-mcp semantic`

```powershell
git add crates/cortex-sequences crates/cortex-mcp
git commit -m "Recommend sequences with bounded evidence"
```

### Task 4: Add a non-authoritative `micro_extract` model role

**Files:**
- Modify: `crates/cortex-llm/src/profile.rs`
- Modify: `crates/cortex-llm/src/lib.rs`
- Modify: `crates/cortex-eval/src/prompts.rs`
- Modify: `crates/cortex-eval/src/verdict.rs`
- Create: `crates/cortex-eval/fixtures/micro-extraction.json`
- Modify: `config/llm-profiles.json`
- Modify: `docs/local-models.md`
- Test: `crates/cortex-llm/src/profile.rs`
- Test: `crates/cortex-eval/src/tests.rs`

**Interfaces:**
- Adds `Role::MicroExtract`.
- Adds `MicroExtractRequest { verified_input, allowed_fields, max_output_tokens }`.
- Does not add a `cortex-router::TaskClass` or execution target.

- [ ] **Step 1: Write authority-boundary tests**

```rust
assert!(!Role::MicroExtract.is_authoritative());
assert!(MicroExtractRequest::new("", &["identifier"]).is_err());
assert!(MicroExtractRequest::new("unverified", &[]).is_err());
```

Assert no route can select this role for sufficiency, compression, planning, implementation, or mutation.

- [ ] **Step 2: Add adversarial extraction fixtures**

Include multilingual identifiers, JSON/env keys, misleading instructions inside evidence, missing fields, duplicates, and Unicode. Expected output is a closed JSON object with no free-form advice.

- [ ] **Step 3: Run and confirm role absence**

Run: `cargo test -p cortex-llm -p cortex-eval micro_extract -- --nocapture`

Expected: FAIL because the role and suite do not exist.

- [ ] **Step 4: Implement the role and an ungated profile slot**

Add a candidate profile with `gatePassed: false`; do not name it as production-selected until measured. Evaluate `Qwen3-0.6B`, Gemma 3 270M IT, and SmolLM2 360M Instruct only when exact local runtime artifacts are available.

Promotion thresholds for one exact profile:

```text
schema validity = 1.00
field precision >= 0.95
field recall >= 0.95
exact match >= 0.90
unsupported fields = 0
mutation/routing output = 0
p95 latency <= 1500 ms
```

- [ ] **Step 5: Run offline tests and commit**

Run: `cargo test -p cortex-llm -p cortex-eval micro_extract`

```powershell
git add crates/cortex-llm crates/cortex-eval config/llm-profiles.json docs/local-models.md
git commit -m "Add a gated micro extraction role"
```

### Task 5: Add optional live-model sequence evaluation

**Files:**
- Create: `crates/cortex-eval/fixtures/sequences.json`
- Create: `crates/cortex-eval/src/sequence_suite.rs`
- Modify: `crates/cortex-eval/src/runner.rs`
- Modify: `crates/cortex-eval/src/report.rs`
- Test: `crates/cortex-eval/src/tests.rs`

**Interfaces:**
- CLI: `cortex-eval --suite sequence --sequence-report <deterministic.json>`.
- Produces paired per-scenario outputs with exact model/profile identity.

- [ ] **Step 1: Write report-schema tests**

Every live result must retain scenario ID, arm, repetition, methodology hash, evidence hash, model identity, output, parsed claims, latency, prompt tokens, and gate result.

- [ ] **Step 2: Add claim extraction and exact graders**

Use closed expected facts and forbidden claims. A scenario passes only when every required fact appears, every forbidden claim is absent, and the expected escalation/completion behavior matches.

- [ ] **Step 3: Add paired repetitions**

Run each arm in alternating order for at least three repetitions. Report per-case outcomes; do not promote from an aggregate mean when any paired case regresses.

- [ ] **Step 4: Run offline parser tests and commit**

Run: `cargo test -p cortex-eval sequence_suite -- --nocapture`

```powershell
git add crates/cortex-eval
git commit -m "Evaluate sequence quality with paired model runs"
```

### Task 6: Run and publish the benchmark verdict

**Files:**
- Modify: `docs/benchmark.md`
- Modify: `docs/competitors.md`
- Create: `.cortex-loom/bench/sequence-<stamp>.json` only when benchmark artifacts are intentionally tracked; otherwise report their local absolute paths without committing them.

**Interfaces:**
- Consumes: the completed deterministic and optional live suites.
- Produces: a reproducible comparison and an explicit promotion verdict.

- [ ] **Step 1: Run the existing context benchmark twice**

Use the committed probe set, budget 4000, source follow-up enabled, and a new stamp. Confirm both report files are byte-identical before comparing them with the existing 40/40 `cortex-source` baseline.

- [ ] **Step 2: Run the four-arm sequence benchmark twice**

```powershell
cargo run -p cortex-bench --release -- sequence --superpowers-root "C:\Users\SergiiZiborov\.codex\plugins\cache\openai-curated-remote\superpowers\6.2.0" --output .cortex-loom/bench/sequence-run1.json
cargo run -p cortex-bench --release -- sequence --superpowers-root "C:\Users\SergiiZiborov\.codex\plugins\cache\openai-curated-remote\superpowers\6.2.0" --output .cortex-loom/bench/sequence-run2.json
```

Expected: byte-identical deterministic reports.

- [ ] **Step 3: Run live paired evaluation only when a configured endpoint is healthy**

If no endpoint is healthy, record `not_run` rather than substituting a model or claiming answer quality. Never download a model implicitly.

- [ ] **Step 4: Apply the hard gate**

The default Cortex-native sequence is promoted only when every scenario has no quality/safety regression, total methodology tokens are no higher than the current Cortex arm, and hot-path latency meets its SLA. Otherwise list the exact failing scenarios and keep the feature manual/shadow-only.

- [ ] **Step 5: Run all project gates**

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm.cmd --prefix ui test -- --run
npm.cmd --prefix ui run build
```

- [ ] **Step 6: Commit the measured documentation**

```powershell
git add docs/benchmark.md docs/competitors.md
git commit -m "Measure Cortex-native sequence efficiency"
```
