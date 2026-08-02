//! Deterministic work and risk routing for Cortex Loom.
//!
//! Model-reported confidence is deliberately absent from this API. Routing is
//! based on inspectable task text, evidence state, schema validity, budgets,
//! mutation authority, and local capability availability.

use serde::{Deserialize, Serialize};

pub use cortex_domain::{ExecutionTarget, RiskLevel};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskClass {
    Deterministic,
    RepositoryAnalysis,
    AdvisoryDraft,
    Implementation,
    Security,
    Authentication,
    Concurrency,
    Migration,
    Release,
    Deployment,
    Publication,
    Ambiguous,
}

impl TaskClass {
    #[must_use]
    pub const fn is_high_risk(self) -> bool {
        matches!(
            self,
            Self::Security
                | Self::Authentication
                | Self::Concurrency
                | Self::Migration
                | Self::Release
                | Self::Deployment
                | Self::Publication
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    NotRequired,
    Verified,
    Missing,
    Contradictory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationStatus {
    None,
    Approved,
    ApprovalRequired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenBudget {
    pub estimated_input_tokens: u32,
    pub estimated_output_tokens: u32,
    pub max_input_tokens: u32,
    pub max_output_tokens: u32,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            estimated_input_tokens: 0,
            estimated_output_tokens: 0,
            max_input_tokens: 8_192,
            max_output_tokens: 1_024,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalAvailability {
    pub weavatrix: bool,
    pub ollama: bool,
}

impl Default for LocalAvailability {
    fn default() -> Self {
        Self {
            weavatrix: true,
            ollama: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoutingRequest {
    pub task: String,
    pub evidence: EvidenceStatus,
    pub schema_valid: bool,
    pub budget: TokenBudget,
    pub mutation: MutationStatus,
    pub availability: LocalAvailability,
}

impl RoutingRequest {
    #[must_use]
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            evidence: EvidenceStatus::NotRequired,
            schema_valid: true,
            budget: TokenBudget::default(),
            mutation: MutationStatus::None,
            availability: LocalAvailability::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    pub class: TaskClass,
    pub risk: RiskLevel,
    pub mutation_likely: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "code", content = "detail")]
pub enum RoutingReason {
    HighRiskTask(TaskClass),
    MissingEvidence,
    ContradictoryEvidence,
    SchemaValidationFailed,
    InputBudgetExceeded { estimated: u32, limit: u32 },
    OutputBudgetExceeded { estimated: u32, limit: u32 },
    MutationApprovalRequired,
    MutationReservedForUpstream,
    AmbiguousRequest,
    ImplementationReservedForUpstream,
    DeterministicRule,
    RepositoryGraphRule,
    AdvisoryDraftRule,
    WeavatrixUnavailable,
    OllamaUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoutingDecision {
    pub target: ExecutionTarget,
    pub class: TaskClass,
    pub risk: RiskLevel,
    pub reasons: Vec<RoutingReason>,
    /// True only for a bounded local draft which cannot mutate project state.
    pub advisory_only: bool,
}

impl RoutingDecision {
    #[must_use]
    pub const fn approves_local_model(&self) -> bool {
        matches!(self.target, ExecutionTarget::Ollama) && self.advisory_only
    }
}

/// Classify work using stable lexical rules, without calling a model.
#[must_use]
pub fn classify(task: &str) -> Classification {
    let normalized = normalize(task);
    let mutation_likely = contains_word_from(
        &normalized,
        &[
            "add",
            "apply",
            "change",
            "delete",
            "edit",
            "fix",
            "implement",
            "modify",
            "remove",
            "rename",
            "replace",
            "rewrite",
            "update",
            "write",
        ],
    );

    let class = if has_phrase(
        &normalized,
        &[
            "authentication",
            "authorization",
            "oauth",
            "openid",
            "login",
            "jwt",
            "access token",
            "refresh token",
            "role based access",
            "tenant isolation",
        ],
    ) || contains_word_from(&normalized, &["auth"])
    {
        TaskClass::Authentication
    } else if has_phrase(
        &normalized,
        &[
            "security",
            "vulnerability",
            "sql injection",
            "cross site scripting",
            "csrf",
            "xss",
            "credential",
            "secret rotation",
            "permission boundary",
            "threat model",
        ],
    ) {
        TaskClass::Security
    } else if has_phrase(
        &normalized,
        &[
            "concurrency",
            "concurrent",
            "race condition",
            "deadlock",
            "thread safety",
            "atomic update",
            "cancellation race",
            "parallel mutation",
        ],
    ) {
        TaskClass::Concurrency
    } else if has_phrase(
        &normalized,
        &[
            "database migration",
            "schema migration",
            "data migration",
            "backfill",
            "migrate database",
        ],
    ) || contains_word_from(&normalized, &["migration", "migrations"])
    {
        TaskClass::Migration
    } else if has_phrase(
        &normalized,
        &[
            "release",
            "version bump",
            "release tag",
            "changelog release",
        ],
    ) {
        TaskClass::Release
    } else if has_phrase(
        &normalized,
        &[
            "deployment",
            "deploy",
            "production rollout",
            "kubernetes",
            "terraform",
            "helm chart",
        ],
    ) {
        TaskClass::Deployment
    } else if has_phrase(
        &normalized,
        &[
            "publication",
            "publish",
            "public registry",
            "cargo publish",
            "npm publish",
            "package registry",
        ],
    ) {
        TaskClass::Publication
    } else if is_ambiguous(&normalized) {
        TaskClass::Ambiguous
    } else if has_phrase(
        &normalized,
        &[
            "dependency graph",
            "call graph",
            "repository graph",
            "repo graph",
            "dead code",
            "reachability",
            "impact analysis",
            "weavatrix",
            "analyze repository",
            "inspect dependencies",
        ],
    ) {
        TaskClass::RepositoryAnalysis
    } else if has_phrase(
        &normalized,
        &[
            "format",
            "sort",
            "parse",
            "validate json",
            "validate schema",
            "count",
            "exact match",
            "extract fields",
            "canonicalize",
            "deterministic",
        ],
    ) && !mutation_likely
    {
        TaskClass::Deterministic
    } else if has_phrase(
        &normalized,
        &[
            "summarize",
            "summary",
            "draft",
            "explain",
            "outline",
            "brainstorm",
            "compress evidence",
            "classify text",
            "suggest wording",
        ],
    ) && !mutation_likely
    {
        TaskClass::AdvisoryDraft
    } else if mutation_likely {
        TaskClass::Implementation
    } else {
        TaskClass::Ambiguous
    };

    let risk = if class.is_high_risk() {
        RiskLevel::High
    } else if matches!(
        class,
        TaskClass::Implementation | TaskClass::RepositoryAnalysis
    ) {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };
    Classification {
        class,
        risk,
        mutation_likely,
    }
}

/// Select a target through deterministic, fail-closed routing rules.
#[must_use]
pub fn route(request: &RoutingRequest) -> RoutingDecision {
    let classification = classify(&request.task);
    let mut reasons = guard_reasons(request);
    if classification.class.is_high_risk() {
        reasons.push(RoutingReason::HighRiskTask(classification.class));
    }
    if request.mutation == MutationStatus::ApprovalRequired {
        reasons.push(RoutingReason::MutationApprovalRequired);
    }
    if request.mutation != MutationStatus::None || classification.mutation_likely {
        reasons.push(RoutingReason::MutationReservedForUpstream);
    }

    if !reasons.is_empty() {
        return upstream(classification, reasons);
    }

    match classification.class {
        TaskClass::Deterministic => decision(
            ExecutionTarget::Deterministic,
            classification,
            RoutingReason::DeterministicRule,
            false,
        ),
        TaskClass::RepositoryAnalysis if request.availability.weavatrix => decision(
            ExecutionTarget::Weavatrix,
            classification,
            RoutingReason::RepositoryGraphRule,
            false,
        ),
        TaskClass::RepositoryAnalysis => {
            upstream(classification, vec![RoutingReason::WeavatrixUnavailable])
        }
        TaskClass::AdvisoryDraft if request.availability.ollama => decision(
            ExecutionTarget::Ollama,
            classification,
            RoutingReason::AdvisoryDraftRule,
            true,
        ),
        TaskClass::AdvisoryDraft => {
            upstream(classification, vec![RoutingReason::OllamaUnavailable])
        }
        TaskClass::Implementation => upstream(
            classification,
            vec![RoutingReason::ImplementationReservedForUpstream],
        ),
        TaskClass::Ambiguous => upstream(classification, vec![RoutingReason::AmbiguousRequest]),
        _ => upstream(
            classification,
            vec![RoutingReason::HighRiskTask(classification.class)],
        ),
    }
}

fn guard_reasons(request: &RoutingRequest) -> Vec<RoutingReason> {
    let mut reasons = Vec::new();
    match request.evidence {
        EvidenceStatus::Missing => reasons.push(RoutingReason::MissingEvidence),
        EvidenceStatus::Contradictory => reasons.push(RoutingReason::ContradictoryEvidence),
        EvidenceStatus::NotRequired | EvidenceStatus::Verified => {}
    }
    if !request.schema_valid {
        reasons.push(RoutingReason::SchemaValidationFailed);
    }
    if request.budget.estimated_input_tokens > request.budget.max_input_tokens {
        reasons.push(RoutingReason::InputBudgetExceeded {
            estimated: request.budget.estimated_input_tokens,
            limit: request.budget.max_input_tokens,
        });
    }
    if request.budget.estimated_output_tokens > request.budget.max_output_tokens {
        reasons.push(RoutingReason::OutputBudgetExceeded {
            estimated: request.budget.estimated_output_tokens,
            limit: request.budget.max_output_tokens,
        });
    }
    reasons
}

fn decision(
    target: ExecutionTarget,
    classification: Classification,
    reason: RoutingReason,
    advisory_only: bool,
) -> RoutingDecision {
    RoutingDecision {
        target,
        class: classification.class,
        risk: classification.risk,
        reasons: vec![reason],
        advisory_only,
    }
}

fn upstream(classification: Classification, reasons: Vec<RoutingReason>) -> RoutingDecision {
    RoutingDecision {
        target: ExecutionTarget::Upstream,
        class: classification.class,
        risk: classification.risk.max(RiskLevel::Medium),
        reasons,
        advisory_only: false,
    }
}

fn normalize(task: &str) -> String {
    task.chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_phrase(normalized: &str, phrases: &[&str]) -> bool {
    let padded = format!(" {normalized} ");
    phrases
        .iter()
        .any(|phrase| padded.contains(&format!(" {phrase} ")))
}

fn contains_word_from(normalized: &str, words: &[&str]) -> bool {
    normalized
        .split_whitespace()
        .any(|word| words.contains(&word))
}

fn is_ambiguous(normalized: &str) -> bool {
    normalized.is_empty()
        || matches!(
            normalized,
            "fix it" | "do it" | "handle this" | "make it better" | "update this" | "change this"
        )
}

#[cfg(test)]
mod tests;
