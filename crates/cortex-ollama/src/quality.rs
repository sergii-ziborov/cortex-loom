use std::collections::HashSet;

use cortex_router::{ExecutionTarget, RoutingDecision};

use crate::{DraftAssessment, LocalDraft, QualityFailure};

/// Apply the deterministic evidence and routing gate to model output.
#[must_use]
pub fn assess_local_draft(
    content: &str,
    supplied_evidence_ids: &[String],
    routing: &RoutingDecision,
) -> DraftAssessment {
    if !routing.approves_local_model() {
        return fallback(None, vec![QualityFailure::RouterRejected]);
    }
    let draft: LocalDraft = match serde_json::from_str(content) {
        Ok(draft) => draft,
        Err(error) => {
            return fallback(None, vec![QualityFailure::SchemaInvalid(error.to_string())]);
        }
    };
    let supplied: HashSet<_> = supplied_evidence_ids.iter().map(String::as_str).collect();
    let mut unknown: Vec<_> = draft
        .evidence_ids
        .iter()
        .filter(|id| !supplied.contains(id.as_str()))
        .cloned()
        .collect();
    unknown.sort();
    unknown.dedup();

    let mut failures = Vec::new();
    if draft.summary.trim().is_empty() {
        failures.push(QualityFailure::EmptySummary);
    }
    if !unknown.is_empty() {
        failures.push(QualityFailure::UnknownEvidenceIds(unknown));
    }
    if !draft.unresolved.is_empty() {
        failures.push(QualityFailure::UnresolvedIssues(draft.unresolved.clone()));
    }
    if failures.is_empty() {
        DraftAssessment {
            target: ExecutionTarget::Ollama,
            draft: Some(draft),
            failures,
        }
    } else {
        fallback(Some(draft), failures)
    }
}

pub(crate) fn fallback(
    draft: Option<LocalDraft>,
    failures: Vec<QualityFailure>,
) -> DraftAssessment {
    DraftAssessment {
        target: ExecutionTarget::Upstream,
        draft,
        failures,
    }
}
