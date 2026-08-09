use std::collections::HashMap;
use std::env;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weavatrix_rust::{Weavatrix, operations};

use crate::{McpChild, McpCommand, McpError};

struct TargetedEvidence {
    bundle: EvidenceBundle,
    search_hits: Vec<crate::source_followup::SearchHit>,
}

const MAX_EVIDENCE_CHARS: usize = 24_000;
/// Fragments above this size are split into stable sub-citations so a token
/// budget can keep part of a large fragment instead of dropping it whole
/// (measured: a 6k-token plan fragment was omitted entirely at a 4k budget).
const MAX_FRAGMENT_CHARS: usize = 4_096;

#[derive(Debug, Clone)]
pub struct WeavatrixConfig {
    pub program: String,
    pub refactor_script: Option<PathBuf>,
    pub timeout: Duration,
    pub max_frame_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceBundle {
    pub repository: String,
    pub evidence: Vec<EvidenceFragment>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceFragment {
    pub id: String,
    pub kind: EvidenceKind,
    pub source: String,
    pub content: String,
    /// True for the first sub-citation of a split tool result.
    ///
    /// Criticality belongs to the head of a fragment, not to every piece of
    /// it. Marking each split part critical made any budget below roughly
    /// 5 000 tokens refuse to compile whenever symbol evidence was present —
    /// measured, see `docs/benchmark.md`. The head can never be dropped; the
    /// tail can be truncated by the budget like any other high-priority
    /// evidence.
    #[serde(default = "default_head")]
    pub head: bool,
}

const fn default_head() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    GraphStats,
    ModuleMap,
    ChangePlan,
    SymbolContext,
    /// Identifier-level matches with surrounding lines: the only evidence
    /// kind that reliably carries the exact names a task mentions.
    SearchHits,
    /// Callers and dependents of a symbol.
    Dependents,
    /// Statically extracted HTTP/API endpoints and transport contracts.
    Endpoints,
    /// Bounded source windows around search hits (`read_source`).
    SourceReads,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefactorOperation {
    RenameSymbol,
    RenameRelatedSymbols,
    MoveFile,
    MoveSymbol,
    ChangeSignature,
    EditSymbol,
}

impl RefactorOperation {
    fn tool_name(self) -> &'static str {
        match self {
            Self::RenameSymbol => "rename_symbol",
            Self::RenameRelatedSymbols => "rename_related_symbols",
            Self::MoveFile => "move_file",
            Self::MoveSymbol => "move_symbol",
            Self::ChangeSignature => "change_signature",
            Self::EditSymbol => "edit_symbol",
        }
    }

    fn uses_preview_envelope(self) -> bool {
        matches!(self, Self::RenameSymbol | Self::RenameRelatedSymbols)
    }
}

#[derive(Debug)]
pub enum WeavatrixError {
    NotFound(String),
    Transport(McpError),
    InvalidArguments(String),
    Engine(String),
    LockPoisoned,
}

impl Display for WeavatrixError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(message) | Self::InvalidArguments(message) | Self::Engine(message) => {
                formatter.write_str(message)
            }
            Self::Transport(error) => Display::fmt(error, formatter),
            Self::LockPoisoned => formatter.write_str("Weavatrix session lock was poisoned"),
        }
    }
}

impl std::error::Error for WeavatrixError {}

impl From<McpError> for WeavatrixError {
    fn from(value: McpError) -> Self {
        Self::Transport(value)
    }
}

#[derive(Clone)]
pub struct WeavatrixAdapter {
    config: WeavatrixConfig,
    engines: Arc<Mutex<HashMap<PathBuf, Weavatrix>>>,
}

impl WeavatrixConfig {
    pub fn discover() -> Result<Self, WeavatrixError> {
        let program =
            env::var("CORTEX_LOOM_WEAVATRIX_COMMAND").unwrap_or_else(|_| "node".to_owned());
        let refactor_script = if let Ok(script) = env::var("CORTEX_LOOM_REFACTOR_SCRIPT") {
            Some(PathBuf::from(script))
        } else {
            discover_refactor_script()
        };
        Ok(Self {
            program,
            refactor_script,
            timeout: Duration::from_secs(90),
            max_frame_bytes: 16 * 1024 * 1024,
        })
    }
}

impl WeavatrixAdapter {
    #[must_use]
    pub fn new(config: WeavatrixConfig) -> Self {
        Self {
            config,
            engines: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn prepare_context(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        let root = repository.canonicalize().map_err(|error| {
            WeavatrixError::Engine(format!("cannot open {}: {error}", repository.display()))
        })?;
        let mut sessions = self
            .engines
            .lock()
            .map_err(|_| WeavatrixError::LockPoisoned)?;
        if !sessions.contains_key(&root) {
            let engine = Weavatrix::open(&root).map_err(|error| {
                WeavatrixError::Engine(format!("Weavatrix graph build failed: {error}"))
            })?;
            sessions.insert(root.clone(), engine);
        }
        let engine = sessions.get_mut(&root).ok_or_else(|| {
            WeavatrixError::Engine("native Weavatrix session was not retained".to_owned())
        })?;
        let refreshed = engine.refresh_if_stale().map_err(|error| {
            WeavatrixError::Engine(format!("Weavatrix refresh failed: {error}"))
        })?;
        let graph_status = native_call(engine, "graph_stats", json!({}))?;
        let module_map = native_call(
            engine,
            "module_map",
            json!({"top_n": 24, "include_non_product": false}),
        )?;
        let symbol_context = symbol
            .map(|label| {
                native_call(
                    engine,
                    "context_bundle",
                    json!({
                        "label": label,
                        "max_related": 30,
                        "max_references": 30,
                        "max_source_files": 12
                    }),
                )
            })
            .transpose()?;
        let verification = native_call(
            engine,
            "verified_change",
            json!({
                "task": task,
                "phase": "plan",
                "duplicate_ratchet": true,
                "run_tests": false
            }),
        )?;
        let repository = repository.to_string_lossy().into_owned();
        let mut evidence = Vec::new();
        evidence.extend(fragments(
            "WX-GRAPH",
            EvidenceKind::GraphStats,
            "weavatrix:graph_stats",
            &graph_status,
        ));
        evidence.extend(fragments(
            "WX-MODULES",
            EvidenceKind::ModuleMap,
            "weavatrix:module_map",
            &module_map,
        ));
        evidence.extend(fragments(
            "WX-VERIFY",
            EvidenceKind::ChangePlan,
            "weavatrix:verified_change",
            &verification,
        ));
        if let Some(symbol_context) = &symbol_context {
            evidence.extend(fragments(
                "WX-SYMBOL",
                EvidenceKind::SymbolContext,
                "weavatrix:context_bundle",
                symbol_context,
            ));
        }
        Ok(EvidenceBundle {
            repository,
            evidence,
            warnings: refreshed
                .then(|| "native Weavatrix graph refreshed from changed source evidence".to_owned())
                .into_iter()
                .collect(),
        })
    }

    /// Collect evidence for one task by asking Weavatrix the operations the
    /// task actually implies, each under a share of `budget`.
    ///
    /// The difference from [`WeavatrixAdapter::prepare_context`] is not the
    /// compiler but the questions: that path always asks the same four
    /// structural operations, which describe a repository without containing
    /// the identifiers a task named. This path plans from the task text (see
    /// [`crate::plan`]) and pushes the budget down into each operation, so
    /// Weavatrix trims the array it understands instead of a whole fragment
    /// being dropped afterwards.
    ///
    /// A failing operation is a warning, not a failure: partial evidence with
    /// the omission on the record beats no evidence at all.
    pub fn prepare_targeted_context(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        self.prepare_targeted_context_with(
            repository,
            task,
            symbol,
            budget,
            crate::plan::PlanPolicy::default(),
        )
    }

    /// As [`WeavatrixAdapter::prepare_targeted_context`], with an explicit
    /// cost policy.
    ///
    /// The default policy drops operations whose estimated cost the budget
    /// cannot carry. Raising `overcommit` asks for them anyway, which is what
    /// a benchmark needs in order to establish whether an operation that is
    /// normally trimmed was worth trimming.
    ///
    /// # Errors
    ///
    /// Returns [`WeavatrixError`] when the repository cannot be opened or the
    /// native graph cannot be built or refreshed.
    pub fn prepare_targeted_context_with(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        self.collect_targeted(
            repository,
            task,
            symbol,
            budget,
            policy,
            crate::PlanHints::default(),
            false,
        )
        .map(|gathered| gathered.bundle)
    }

    /// As [`WeavatrixAdapter::prepare_targeted_context_with`], then open
    /// bounded `read_source` windows on the files `search_code` hit.
    ///
    /// This is the control for whether identifier-adjacent facts that live a
    /// few lines past a search match (`compile_skill`, `fn endpoint`) can be
    /// recovered without paying the naive whole-file cost.
    ///
    /// # Errors
    ///
    /// Returns [`WeavatrixError`] when the repository cannot be opened or the
    /// native graph cannot be built or refreshed.
    pub fn prepare_targeted_context_with_source_reads(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        self.collect_targeted(
            repository,
            task,
            symbol,
            budget,
            policy,
            crate::PlanHints::default(),
            true,
        )
        .map(|gathered| gathered.bundle)
    }

    /// Gather with active-skill hints, verify structural sufficiency, and run
    /// at most one deterministic recovery pass before returning.
    ///
    /// The recovery widens an empty identifier search, retries missing
    /// structural operations, and opens source windows when requested. It
    /// never drafts an answer and never applies a refactor.
    ///
    /// # Errors
    ///
    /// Returns [`WeavatrixError`] when the repository cannot be opened or the
    /// native graph cannot be built or refreshed.
    pub fn prepare_verified_targeted_context(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
        hints: crate::PlanHints,
    ) -> Result<(EvidenceBundle, crate::EvidenceSufficiency), WeavatrixError> {
        let source_followup = hints.source_followup_or(true);
        let mut gathered = self.collect_targeted(
            repository,
            task,
            symbol,
            budget,
            policy,
            hints,
            source_followup,
        )?;
        let initial = crate::verify::assess_gathered(
            &gathered.bundle,
            task,
            symbol,
            hints,
            source_followup,
            gathered.search_hits.len(),
            false,
        );
        if initial.sufficient {
            return Ok((gathered.bundle, initial));
        }
        self.retry_targeted(
            repository,
            task,
            symbol,
            budget,
            policy,
            hints,
            source_followup,
            &initial,
            &mut gathered,
        )?;
        let final_report = crate::verify::assess_gathered(
            &gathered.bundle,
            task,
            symbol,
            hints,
            source_followup,
            gathered.search_hits.len(),
            true,
        );
        Ok((gathered.bundle, final_report))
    }

    #[allow(clippy::too_many_arguments)] // explicit gather controls; no transport request object
    fn collect_targeted(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
        hints: crate::PlanHints,
        source_followup: bool,
    ) -> Result<TargetedEvidence, WeavatrixError> {
        let root = self.canonical_root(repository)?;
        let mut sessions = self
            .engines
            .lock()
            .map_err(|_| WeavatrixError::LockPoisoned)?;
        let engine = Self::session(&mut sessions, &root)?;
        let refreshed = engine.refresh_if_stale().map_err(|error| {
            WeavatrixError::Engine(format!("Weavatrix refresh failed: {error}"))
        })?;
        let mut evidence = Vec::new();
        let mut warnings: Vec<String> = refreshed
            .then(|| "native Weavatrix graph refreshed from changed source evidence".to_owned())
            .into_iter()
            .collect();
        let mut search_hits = Vec::new();
        for operation in crate::plan::plan_with_hints(task, symbol, budget, policy, hints) {
            match native_call(engine, operation.tool, operation.arguments.clone()) {
                Ok(value) => {
                    if let Some(overrun) = budget_overrun(operation.tool, &value) {
                        warnings.push(overrun);
                    }
                    if operation.tool == "search_code" {
                        search_hits.extend(crate::source_followup::hits_from_search(&value));
                    }
                    evidence.extend(fragments(
                        operation.id,
                        operation.kind,
                        &format!("weavatrix:{}", operation.tool),
                        &value,
                    ));
                }
                Err(error) => warnings.push(format!("{} unavailable: {error}", operation.tool)),
            }
        }
        if source_followup {
            append_source_reads(
                engine,
                &mut evidence,
                &mut warnings,
                &search_hits,
                budget,
                policy,
                "WX-SOURCE",
            );
        }
        Ok(TargetedEvidence {
            bundle: EvidenceBundle {
                repository: repository.to_string_lossy().into_owned(),
                evidence,
                warnings,
            },
            search_hits,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn retry_targeted(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
        hints: crate::PlanHints,
        source_followup: bool,
        initial: &crate::EvidenceSufficiency,
        gathered: &mut TargetedEvidence,
    ) -> Result<(), WeavatrixError> {
        gathered.bundle.warnings.push(format!(
            "evidence sufficiency retry: missing {}",
            initial.missing_evidence.join(", ")
        ));
        let root = self.canonical_root(repository)?;
        let mut sessions = self
            .engines
            .lock()
            .map_err(|_| WeavatrixError::LockPoisoned)?;
        let engine = Self::session(&mut sessions, &root)?;

        if initial
            .missing_evidence
            .iter()
            .any(|kind| kind == "search_hits")
        {
            retry_wide_search(
                engine,
                &mut gathered.bundle.evidence,
                &mut gathered.bundle.warnings,
                &mut gathered.search_hits,
                task,
                budget,
                policy,
            );
        }
        for operation in crate::plan::plan_with_hints(task, symbol, budget, policy, hints) {
            let kind = crate::verify::kind_name(operation.kind);
            if !initial
                .missing_evidence
                .iter()
                .any(|missing| missing == kind)
                || operation.kind == EvidenceKind::SearchHits
            {
                continue;
            }
            match native_call(engine, operation.tool, operation.arguments) {
                Ok(value) => gathered.bundle.evidence.extend(fragments(
                    &format!("WX-RETRY-{}", operation.id.trim_start_matches("WX-")),
                    operation.kind,
                    &format!("weavatrix:{}", operation.tool),
                    &value,
                )),
                Err(error) => gathered
                    .bundle
                    .warnings
                    .push(format!("{} retry unavailable: {error}", operation.tool)),
            }
        }
        let has_source = gathered
            .bundle
            .evidence
            .iter()
            .any(|item| item.kind == EvidenceKind::SourceReads);
        if source_followup && !has_source {
            append_source_reads(
                engine,
                &mut gathered.bundle.evidence,
                &mut gathered.bundle.warnings,
                &gathered.search_hits,
                budget,
                policy,
                "WX-RETRY-SOURCE",
            );
        }
        Ok(())
    }

    fn canonical_root(&self, repository: &Path) -> Result<PathBuf, WeavatrixError> {
        let _ = self;
        repository.canonicalize().map_err(|error| {
            WeavatrixError::Engine(format!("cannot open {}: {error}", repository.display()))
        })
    }

    fn session<'a>(
        sessions: &'a mut HashMap<PathBuf, Weavatrix>,
        root: &Path,
    ) -> Result<&'a mut Weavatrix, WeavatrixError> {
        if !sessions.contains_key(root) {
            let engine = Weavatrix::open(root).map_err(|error| {
                WeavatrixError::Engine(format!("Weavatrix graph build failed: {error}"))
            })?;
            sessions.insert(root.to_path_buf(), engine);
        }
        sessions.get_mut(root).ok_or_else(|| {
            WeavatrixError::Engine("native Weavatrix session was not retained".to_owned())
        })
    }

    pub fn preview_refactor(
        &self,
        repository: &Path,
        operation: RefactorOperation,
        arguments: &Value,
    ) -> Result<String, WeavatrixError> {
        let arguments = arguments.as_object().cloned().ok_or_else(|| {
            WeavatrixError::InvalidArguments("refactor arguments must be a JSON object".to_owned())
        })?;
        let mut arguments = Value::Object(arguments);
        strip_confirmation_fields(&mut arguments);
        {
            let object = arguments.as_object_mut().ok_or_else(|| {
                WeavatrixError::InvalidArguments(
                    "refactor arguments must be a JSON object".to_owned(),
                )
            })?;
            if operation.uses_preview_envelope() {
                object.insert("mode".to_owned(), Value::String("preview".to_owned()));
            }
            object.insert("output_format".to_owned(), Value::String("json".to_owned()));
        }

        let mut client = self.refactor_client(repository)?;
        client.call_tool(
            "open_repo",
            &json!({
                "path": repository.to_string_lossy(),
                "build": true,
                "mode": "full",
                "precision": "lsp",
                "output_format": "json"
            }),
        )?;
        let result = client.call_tool(operation.tool_name(), &arguments)?;
        let mut result = result;
        strip_confirmation_fields(&mut result);
        Ok(extract_text(&result))
    }

    fn refactor_client(&self, repository: &Path) -> Result<McpChild, WeavatrixError> {
        let script = self.config.refactor_script.as_ref().ok_or_else(|| {
            WeavatrixError::NotFound(
                "Weavatrix Refactor was not found; set CORTEX_LOOM_REFACTOR_SCRIPT".to_owned(),
            )
        })?;
        let mut client = McpChild::spawn(&McpCommand {
            program: self.config.program.clone(),
            args: vec![
                script.to_string_lossy().into_owned(),
                repository.to_string_lossy().into_owned(),
            ],
            cwd: repository.parent().map(Path::to_path_buf),
            timeout: self.config.timeout,
            max_frame_bytes: self.config.max_frame_bytes,
        })?;
        client.initialize()?;
        Ok(client)
    }
}

/// Convert one tool result into one or more stable, individually citable
/// fragments. Small results keep the bare id (`WX-VERIFY`); oversized results
/// split deterministically into `WX-VERIFY-1..n` at paragraph boundaries so a
/// token budget can keep a prefix instead of dropping the whole fragment.
fn fragments(id: &str, kind: EvidenceKind, source: &str, value: &Value) -> Vec<EvidenceFragment> {
    let content = extract_text(value);
    let parts = split_content(&content, MAX_FRAGMENT_CHARS);
    let single = parts.len() == 1;
    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| EvidenceFragment {
            id: if single {
                id.to_owned()
            } else {
                format!("{id}-{}", index + 1)
            },
            kind,
            source: source.to_owned(),
            content: part,
            head: index == 0,
        })
        .collect()
}

/// Deterministic paragraph packing: greedy chunks up to `max_chars`,
/// splitting a single oversized paragraph at a character boundary.
fn split_content(content: &str, max_chars: usize) -> Vec<String> {
    if content.chars().count() <= max_chars {
        return vec![content.to_owned()];
    }
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0_usize;
    for paragraph in content.split("\n\n") {
        let mut remaining = paragraph;
        loop {
            let remaining_chars = remaining.chars().count();
            let separator = usize::from(current_chars > 0) * 2;
            if current_chars + separator + remaining_chars <= max_chars {
                if current_chars > 0 {
                    current.push_str("\n\n");
                    current_chars += 2;
                }
                current.push_str(remaining);
                current_chars += remaining_chars;
                break;
            }
            if current_chars > 0 {
                parts.push(std::mem::take(&mut current));
                current_chars = 0;
                continue;
            }
            // A single paragraph larger than the cap: hard character split.
            let boundary = remaining
                .char_indices()
                .nth(max_chars)
                .map_or(remaining.len(), |(offset, _)| offset);
            parts.push(remaining[..boundary].to_owned());
            remaining = &remaining[boundary..];
            if remaining.is_empty() {
                break;
            }
        }
    }
    if current_chars > 0 {
        parts.push(current);
    }
    parts.retain(|part| !part.trim().is_empty());
    if parts.is_empty() {
        parts.push(content.to_owned());
    }
    parts
}

fn strip_confirmation_fields(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.retain(|key, _| !is_confirmation_key(key));
            for child in object.values_mut() {
                strip_confirmation_fields(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_confirmation_fields(item);
            }
        }
        Value::String(text) => {
            if let Ok(mut nested) = serde_json::from_str::<Value>(text) {
                strip_confirmation_fields(&mut nested);
                *text = nested.to_string();
            } else if text.lines().any(contains_confirmation_label) {
                *text = text
                    .lines()
                    .filter(|line| !contains_confirmation_label(line))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn is_confirmation_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase()
            .replace(['_', '-', ' '], "")
            .as_str(),
        "confirmtoken" | "confirmationtoken" | "applytoken"
    )
}

fn contains_confirmation_label(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase().replace(['_', '-'], " ");
    ["confirm token", "confirmation token", "apply token"]
        .iter()
        .any(|label| normalized.contains(label))
}

fn discover_refactor_script() -> Option<PathBuf> {
    let current = env::current_dir().ok()?;
    let mut roots = vec![current.clone()];
    roots.extend(current.ancestors().map(Path::to_path_buf));
    roots.into_iter().find_map(|root| {
        let direct = root.join("weavatrix-refactor/bin/weavatrix-refactor-mcp.mjs");
        let sibling = root.join("../weavatrix-refactor/bin/weavatrix-refactor-mcp.mjs");
        [direct, sibling]
            .into_iter()
            .find(|candidate| candidate.is_file())
    })
}

/// Read the `token_budget` report a bounded operation attaches to its reply.
///
/// `fit: false` is not a failure and not a Weavatrix defect — the graph
/// relationships it returns are lossless by contract, so it trims source
/// excerpts and then tells the truth rather than dropping evidence to hit a
/// number. Consuming that signal is the point: a packet built from an
/// overrun answer is bigger than the caller asked for, and the caller should
/// be told by whom and by how much instead of discovering it in the total.
fn budget_overrun(tool: &str, value: &Value) -> Option<String> {
    let report = value.get("token_budget")?;
    if report.get("fit").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let requested = report.get("requested").and_then(Value::as_u64)?;
    let estimated = report.get("estimated_tokens").and_then(Value::as_u64)?;
    let dropped = report
        .get("dropped_items")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(format!(
        "{tool} could not fit its budget: asked {requested} tokens, returned \
         {estimated} after dropping {dropped} items; the remainder is lossless \
         evidence it will not discard"
    ))
}

fn native_call(
    engine: &mut Weavatrix,
    name: &str,
    arguments: Value,
) -> Result<Value, WeavatrixError> {
    operations::call(engine, name, arguments).map_err(WeavatrixError::Engine)
}

fn append_source_reads(
    engine: &mut Weavatrix,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    search_hits: &[crate::source_followup::SearchHit],
    budget: u32,
    policy: crate::plan::PlanPolicy,
    id_prefix: &str,
) {
    let paths =
        crate::source_followup::unique_paths(search_hits, crate::source_followup::MAX_SOURCE_FILES);
    if paths.is_empty() {
        warnings.push("source follow-up skipped: search returned no file paths".to_owned());
        return;
    }
    let per_file = crate::source_followup::per_file_budget(budget, paths.len(), policy);
    warnings.push(format!(
        "source follow-up: {} file(s), ~{per_file} tokens each",
        paths.len()
    ));
    for (index, hit) in paths.iter().enumerate() {
        let arguments = crate::source_followup::read_arguments(hit, per_file);
        match native_call(engine, "read_source", arguments) {
            Ok(value) => {
                if let Some(overrun) = budget_overrun("read_source", &value) {
                    warnings.push(overrun);
                }
                evidence.extend(fragments(
                    &format!("{id_prefix}-{}", index + 1),
                    EvidenceKind::SourceReads,
                    "weavatrix:read_source",
                    &value,
                ));
            }
            Err(error) => {
                warnings.push(format!("read_source unavailable for {}: {error}", hit.path));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn retry_wide_search(
    engine: &mut Weavatrix,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    search_hits: &mut Vec<crate::source_followup::SearchHit>,
    task: &str,
    budget: u32,
    policy: crate::plan::PlanPolicy,
) {
    let identifiers = crate::plan::extract_identifiers(task);
    if identifiers.is_empty() {
        warnings.push("wide search retry skipped: task names no identifiers".to_owned());
        return;
    }
    let token_budget = policy
        .search_tokens
        .min(budget.saturating_mul(2) / 5)
        .max(200);
    let arguments = json!({
        "query": crate::plan::search_pattern(&identifiers),
        "is_regex": true,
        "before": 2,
        "after": 2,
        "max_results": 80,
        "glob": "{apps,crates,ui,config}/**/*",
        "token_budget": token_budget,
    });
    match native_call(engine, "search_code", arguments) {
        Ok(value) => {
            search_hits.extend(crate::source_followup::hits_from_search(&value));
            evidence.extend(fragments(
                "WX-RETRY-SEARCH",
                EvidenceKind::SearchHits,
                "weavatrix:search_code",
                &value,
            ));
        }
        Err(error) => warnings.push(format!("wide search retry unavailable: {error}")),
    }
}

fn extract_text(value: &Value) -> String {
    let structured = value
        .get("structuredContent")
        .and_then(|content| content.get("result"))
        .and_then(|result| result.get("text"))
        .and_then(Value::as_str);
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str);
    let text = structured
        .or(content)
        .map_or_else(|| value.to_string(), str::to_owned);
    truncate_chars(text, MAX_EVIDENCE_CHARS)
}

fn truncate_chars(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut result: String = value.chars().take(max_chars).collect();
    result.push_str("\n[truncated by Cortex Loom]");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refactor_allowlist_never_maps_to_apply() {
        for operation in [
            RefactorOperation::RenameSymbol,
            RefactorOperation::RenameRelatedSymbols,
            RefactorOperation::MoveFile,
            RefactorOperation::MoveSymbol,
            RefactorOperation::ChangeSignature,
            RefactorOperation::EditSymbol,
        ] {
            assert_ne!(operation.tool_name(), "apply_edit_plan");
        }
    }

    #[test]
    fn extracts_structured_text_before_fallback_content() {
        let value = json!({
            "content": [{"type": "text", "text": "fallback"}],
            "structuredContent": {"result": {"text": "structured"}}
        });
        assert_eq!(extract_text(&value), "structured");
    }

    #[test]
    fn small_results_keep_the_bare_citation_id() {
        let value = json!({"content": [{"type": "text", "text": "short plan"}]});
        let parts = fragments("WX-VERIFY", EvidenceKind::ChangePlan, "weavatrix:v", &value);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].id, "WX-VERIFY");
    }

    #[test]
    fn oversized_results_split_into_stable_ordered_sub_citations() {
        let paragraphs: Vec<String> = (0..8)
            .map(|i| format!("paragraph {i} {}", "x".repeat(900)))
            .collect();
        let text = paragraphs.join("\n\n");
        let value = json!({"content": [{"type": "text", "text": text}]});
        let parts = fragments("WX-VERIFY", EvidenceKind::ChangePlan, "weavatrix:v", &value);
        assert!(parts.len() > 1, "must split: {}", parts.len());
        for (index, part) in parts.iter().enumerate() {
            assert_eq!(part.id, format!("WX-VERIFY-{}", index + 1));
            assert!(part.content.chars().count() <= MAX_FRAGMENT_CHARS);
            assert_eq!(part.kind, EvidenceKind::ChangePlan);
        }
        let rejoined: String = parts
            .iter()
            .map(|part| part.content.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(rejoined, text, "splitting loses no content");

        let again = fragments("WX-VERIFY", EvidenceKind::ChangePlan, "weavatrix:v", &value);
        assert_eq!(parts, again, "splitting is deterministic");
    }

    #[test]
    fn a_single_oversized_paragraph_is_hard_split_without_loss() {
        let text = "y".repeat(MAX_FRAGMENT_CHARS * 2 + 100);
        let parts = split_content(&text, MAX_FRAGMENT_CHARS);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts.concat(), text);
    }

    #[test]
    fn preview_results_strip_nested_confirmation_tokens() {
        let mut value = json!({
            "confirm_token": "outer-secret",
            "content": [{
                "type": "text",
                "text": "{\"plan\":1,\"confirmationToken\":\"inner-secret\"}"
            }]
        });
        strip_confirmation_fields(&mut value);
        let rendered = value.to_string();
        assert!(!rendered.contains("outer-secret"));
        assert!(!rendered.contains("inner-secret"));
        assert_eq!(value["content"][0]["text"], "{\"plan\":1}");
    }
}
