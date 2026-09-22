//! Cortex-owned vocabulary the `micro_extract` train split is composed from.
//!
//! Every entry is something this repository actually contains — an identifier,
//! a path, an environment key, a profile key — or a multilingual literal chosen
//! to exercise exact copying. Nothing here is drawn from upstream skill text.

pub const IDENTIFIERS: [&str; 24] = [
    "prepare_context",
    "route_work",
    "GraphStore",
    "MicroExtractRequest",
    "validate_output",
    "LoopbackUrl",
    "DevicePolicy",
    "Placement",
    "judge_micro_extract",
    "active_step_packet",
    "instantiate_template",
    "rank_by_similarity",
    "Bm25Index",
    "graph_boost",
    "rrf_fuse",
    "estimate_tokens",
    "safe_virtual_path",
    "finish_block",
    "quiet_match",
    "CorpusRecord",
    "SuiteSelection",
    "EvalBackend",
    "TimedContent",
    "ModelTier",
];

pub const ENV_KEYS: [&str; 12] = [
    "CORTEX_SEMANTIC",
    "CORTEX_LLM",
    "CORTEX_LLM_PROFILES",
    "SAFE_MODE",
    "CORTEX_EVAL_LIMIT",
    "CORTEX_SHADOW",
    "CORTEX_MCP_PORT",
    "CORTEX_REPORT_DIR",
    "RUST_LOG",
    "NO_COLOR",
    "CORTEX_PROMPT_VERSION",
    "CORTEX_SEQUENCE_ROOT",
];

pub const FILES: [&str; 20] = [
    "config/llm-profiles.json",
    "config/model-inventory.json",
    "config/eval-profiles.json",
    "crates/cortex-llm/src/profile.rs",
    "crates/cortex-llm/src/micro_extract.rs",
    "crates/cortex-eval/src/corpus.rs",
    "crates/cortex-eval/src/verdict.rs",
    "crates/cortex-eval/src/metrics.rs",
    "crates/cortex-eval/src/runner.rs",
    "crates/cortex-router/src/lib.rs",
    "crates/cortex-context/src/ranking.rs",
    "crates/cortex-sequences/src/template.rs",
    "crates/cortex-weavatrix/src/context.rs",
    "crates/cortex-store/src/lib.rs",
    "crates/cortex-domain/src/lib.rs",
    "docs/local-models.md",
    "docs/benchmark.md",
    "docs/fine-tune.md",
    "corpora/sft.jsonl",
    "AGENTS.md",
];

pub const JSON_KEYS: [&str; 16] = [
    "gatePassed",
    "baseUrl",
    "timeoutSeconds",
    "modelProfile",
    "targetRole",
    "trainingSource",
    "requiredEvidence",
    "escalationEdges",
    "maxOutputTokens",
    "contextTokens",
    "probedAt",
    "schemaVersion",
    "promptVersion",
    "rejectedOutputs",
    "evidenceIds",
    "maxInputTokens",
];

pub const LABELS: [&str; 14] = [
    "café",
    "naïve",
    "Διαδρομή",
    "Grüße",
    "résumé",
    "Ştefan",
    "façade",
    "ñandú",
    "Ævintýri",
    "Zürich",
    "Ω-band",
    "日本語ラベル",
    // Right-to-left scripts are where a small model quietly drops a value.
    "ראשי",
    "طريق",
];

/// Bare declared constants. The holdout counts `const PORT = 43817` as an
/// identifier, so the train split must too: an earlier revision left the
/// constant out of gold and taught the model the opposite rule.
pub const CONSTANTS: [&str; 8] = [
    "LIMIT",
    "BUDGET",
    "MAX_ENTRIES",
    "TIMEOUT_MS",
    "WINDOW",
    "THRESHOLD",
    "RETRIES",
    "CAPACITY",
];

/// Nouns that sit next to a value in prose and are not themselves values.
pub const PROSE_NOUNS: [&str; 6] = ["key", "file", "variable", "path", "symbol", "value"];

/// Non-Latin tokens that appear in verified evidence as symbol names.
pub const UNICODE_IDENTS: [&str; 6] = [
    "маршрутизатор",
    "בודק_שער",
    "検証済み",
    "구성요소",
    "переменная",
    "مسار",
];

/// A literal and its diacritic-folded form. The fold is never a substring of
/// the literal, so a folded reply is an invention the validator must refuse.
pub const FOLD_PAIRS: [(&str, &str); 9] = [
    ("café", "cafe"),
    ("naïve", "naive"),
    ("Grüße", "Grusse"),
    ("résumé", "resume"),
    ("Ştefan", "Stefan"),
    ("façade", "facade"),
    ("ñandú", "nandu"),
    ("Ævintýri", "Aevintyri"),
    ("Zürich", "Zurich"),
];

pub const CRATES: [&str; 8] = [
    "cortex-llm",
    "cortex-eval",
    "cortex-router",
    "cortex-context",
    "cortex-sequences",
    "cortex-weavatrix",
    "cortex-store",
    "cortex-domain",
];

/// Plausible values absent from generated evidence. A reject row picks the
/// first one that really is absent, so the lesson is "not in the evidence",
/// never "this one token means reject".
const DECOYS: [&str; 8] = [
    "invented_handler",
    "shadow_router",
    "MISSING_KEY",
    "config/absent.json",
    "src/phantom.rs",
    "PhantomStore",
    "auto_apply",
    "selectFn",
];

/// Routing vocabulary used as bait: it appears in evidence, never in gold.
pub const ROUTING_BAIT: [&str; 4] = [
    "upstream_strong",
    "local_medium",
    "deterministic",
    "local_small",
];

/// Deterministic vocabulary draw. Callers vary the stride so two draws from
/// the same table in one row rarely collide.
pub fn pick<const N: usize>(table: &[&'static str; N], step: usize) -> &'static str {
    table[step % N]
}

/// The first decoy that genuinely does not occur in this evidence.
pub fn decoy(input: &str, step: usize) -> &'static str {
    (0..DECOYS.len())
        .map(|offset| DECOYS[(step + offset) % DECOYS.len()])
        .find(|candidate| !input.contains(candidate))
        .unwrap_or("zz-absent-from-evidence")
}
