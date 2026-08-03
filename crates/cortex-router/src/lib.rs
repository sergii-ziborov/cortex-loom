//! Deterministic work and risk routing for Cortex Loom.
//!
//! Model-reported confidence is deliberately absent from this API. Routing is
//! based on inspectable task text, evidence state, schema validity, budgets,
//! mutation authority, and local capability availability.

mod classifier;

use serde::{Deserialize, Serialize};

pub use classifier::classify;
pub use cortex_domain::{ExecutionTarget, RiskLevel};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskClass {
    Deterministic,
    RepositoryAnalysis,
    StructuredExtraction,
    ContextCompression,
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
    pub const fn requires_verified_evidence(self) -> bool {
        matches!(self, Self::ContextCompression)
    }
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
    pub model_tier: ModelTier,
    pub context: ContextPlan,
}

impl RoutingDecision {
    #[must_use]
    pub const fn approves_local_model(&self) -> bool {
        matches!(self.target, ExecutionTarget::Ollama) && self.advisory_only
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    None,
    LocalSmall,
    LocalMedium,
    UpstreamStrong,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextStrategy {
    Direct,
    DeterministicExtraction,
    WeavatrixEvidence,
    CitationCompression,
    UpstreamEvidence,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextPlan {
    pub strategy: ContextStrategy,
    pub max_input_tokens: u32,
    pub require_evidence_ids: bool,
}

/// Select a target through deterministic, fail-closed routing rules.
#[must_use]
pub fn route(request: &RoutingRequest) -> RoutingDecision {
    let classification = classify(&request.task);
    let mut reasons = guard_reasons(request);
    if classification.class.requires_verified_evidence()
        && request.evidence == EvidenceStatus::NotRequired
    {
        reasons.push(RoutingReason::MissingEvidence);
    }
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
            ModelTier::None,
            ContextStrategy::DeterministicExtraction,
            request,
        ),
        TaskClass::RepositoryAnalysis if request.availability.weavatrix => decision(
            ExecutionTarget::Weavatrix,
            classification,
            RoutingReason::RepositoryGraphRule,
            false,
            ModelTier::None,
            ContextStrategy::WeavatrixEvidence,
            request,
        ),
        TaskClass::RepositoryAnalysis => {
            upstream(classification, vec![RoutingReason::WeavatrixUnavailable])
        }
        TaskClass::StructuredExtraction if request.availability.ollama => decision(
            ExecutionTarget::Ollama,
            classification,
            RoutingReason::AdvisoryDraftRule,
            true,
            ModelTier::LocalSmall,
            ContextStrategy::DeterministicExtraction,
            request,
        ),
        TaskClass::ContextCompression | TaskClass::AdvisoryDraft if request.availability.ollama => {
            decision(
                ExecutionTarget::Ollama,
                classification,
                RoutingReason::AdvisoryDraftRule,
                true,
                ModelTier::LocalMedium,
                ContextStrategy::CitationCompression,
                request,
            )
        }
        TaskClass::StructuredExtraction
        | TaskClass::ContextCompression
        | TaskClass::AdvisoryDraft => {
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
    model_tier: ModelTier,
    strategy: ContextStrategy,
    request: &RoutingRequest,
) -> RoutingDecision {
    RoutingDecision {
        target,
        class: classification.class,
        risk: classification.risk,
        reasons: vec![reason],
        advisory_only,
        model_tier,
        context: ContextPlan {
            strategy,
            max_input_tokens: request.budget.max_input_tokens,
            require_evidence_ids: matches!(
                strategy,
                ContextStrategy::WeavatrixEvidence | ContextStrategy::CitationCompression
            ),
        },
    }
}

fn upstream(classification: Classification, reasons: Vec<RoutingReason>) -> RoutingDecision {
    RoutingDecision {
        target: ExecutionTarget::Upstream,
        class: classification.class,
        risk: classification.risk.max(RiskLevel::Medium),
        reasons,
        advisory_only: false,
        model_tier: ModelTier::UpstreamStrong,
        context: ContextPlan {
            strategy: ContextStrategy::UpstreamEvidence,
            max_input_tokens: 8_192,
            require_evidence_ids: true,
        },
    }
}

#[cfg(test)]
mod tests;
