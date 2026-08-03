use std::collections::HashSet;

use cortex_domain::GraphDocument;

use crate::{
    EvidenceInvalidation, EvidenceSubmission, MAX_EVIDENCE_FIELD_BYTES, MAX_EVIDENCE_ID_BYTES,
    MAX_EVIDENCE_IDS, MAX_EVIDENCE_SUBMISSIONS, MAX_EVIDENCE_SUMMARY_BYTES, MAX_RUN_DETAIL_BYTES,
    NodeRunStatus, RunDocument, RunError,
};

pub(super) struct EvidenceInput<'a> {
    pub node_id: &'a str,
    pub evidence_id: &'a str,
    pub submitted_by: &'a str,
    pub source: &'a str,
    pub locator: &'a str,
    pub digest: Option<&'a String>,
    pub summary: &'a str,
}

pub(super) fn submit_evidence(
    run: &mut RunDocument,
    graph: &GraphDocument,
    input: &EvidenceInput<'_>,
    now: i64,
) -> Result<(), RunError> {
    if !graph.nodes.iter().any(|node| node.id == input.node_id) {
        return Err(RunError::NodeNotFound(input.node_id.to_owned()));
    }
    let state = run
        .nodes
        .iter()
        .find(|node| node.node_id == input.node_id)
        .ok_or_else(|| RunError::NodeNotFound(input.node_id.to_owned()))?;
    if state.status != NodeRunStatus::Running {
        return Err(RunError::InvalidNodeState {
            node: input.node_id.to_owned(),
            expected: NodeRunStatus::Running,
            current: state.status,
        });
    }
    validate_submission(run, input)?;
    run.evidence.push(EvidenceSubmission {
        id: input.evidence_id.to_owned(),
        node_id: input.node_id.to_owned(),
        attempt: state.attempt,
        submitted_by: input.submitted_by.to_owned(),
        source: input.source.to_owned(),
        locator: input.locator.to_owned(),
        digest: input.digest.cloned(),
        summary: input.summary.to_owned(),
        submitted_at: now,
        invalidated: None,
    });
    Ok(())
}

/// Record that a submission may no longer be cited. The submission stays in
/// the audit record; already-completed decisions that cited it are historical
/// facts and are never rewritten — reacting to a late invalidation is review
/// or retry work.
pub(super) fn invalidate_evidence(
    run: &mut RunDocument,
    evidence_id: &str,
    actor: &str,
    reason: &str,
    now: i64,
) -> Result<(), RunError> {
    if actor.trim().is_empty() {
        return Err(RunError::EmptyEvidenceField("actor"));
    }
    if actor.len() > MAX_EVIDENCE_FIELD_BYTES {
        return Err(RunError::EvidenceFieldTooLarge {
            field: "actor",
            size: actor.len(),
        });
    }
    if reason.trim().is_empty() {
        return Err(RunError::EmptyEvidenceField("reason"));
    }
    if reason.len() > MAX_RUN_DETAIL_BYTES {
        return Err(RunError::DetailTooLarge(reason.len()));
    }
    let submission = run
        .evidence
        .iter_mut()
        .find(|submission| submission.id == evidence_id)
        .ok_or_else(|| RunError::UnknownEvidence(evidence_id.to_owned()))?;
    if submission.invalidated.is_some() {
        return Err(RunError::EvidenceAlreadyInvalidated(evidence_id.to_owned()));
    }
    submission.invalidated = Some(EvidenceInvalidation {
        actor: actor.to_owned(),
        reason: reason.to_owned(),
        invalidated_at: now,
    });
    Ok(())
}

pub(super) fn validate_evidence_references(
    run: &RunDocument,
    node_id: &str,
    evidence_ids: &[String],
) -> Result<(), RunError> {
    if evidence_ids.len() > MAX_EVIDENCE_IDS {
        return Err(RunError::TooManyEvidenceIds(evidence_ids.len()));
    }
    let mut unique = HashSet::with_capacity(evidence_ids.len());
    for id in evidence_ids {
        validate_id(id)?;
        if !unique.insert(id.as_str()) {
            return Err(RunError::DuplicateEvidence(id.clone()));
        }
        let submission = run
            .evidence
            .iter()
            .find(|submission| submission.id == *id)
            .ok_or_else(|| RunError::UnknownEvidence(id.clone()))?;
        if submission.invalidated.is_some() {
            return Err(RunError::EvidenceInvalidatedError(id.clone()));
        }
        if submission.node_id != node_id {
            return Err(RunError::EvidenceNodeMismatch {
                id: id.clone(),
                node: node_id.to_owned(),
            });
        }
        let attempt = run
            .nodes
            .iter()
            .find(|state| state.node_id == node_id)
            .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))?
            .attempt;
        if submission.attempt != attempt {
            return Err(RunError::EvidenceAttemptMismatch {
                id: id.clone(),
                attempt,
            });
        }
    }
    Ok(())
}

fn validate_submission(run: &RunDocument, input: &EvidenceInput<'_>) -> Result<(), RunError> {
    if run.evidence.len() >= MAX_EVIDENCE_SUBMISSIONS {
        return Err(RunError::TooManyEvidenceSubmissions(run.evidence.len() + 1));
    }
    validate_id(input.evidence_id)?;
    for (field, value) in [
        ("submittedBy", input.submitted_by),
        ("source", input.source),
        ("locator", input.locator),
        ("summary", input.summary),
    ] {
        if value.trim().is_empty() {
            return Err(RunError::EmptyEvidenceField(field));
        }
        let limit = if field == "summary" {
            MAX_EVIDENCE_SUMMARY_BYTES
        } else {
            MAX_EVIDENCE_FIELD_BYTES
        };
        if value.len() > limit {
            return Err(RunError::EvidenceFieldTooLarge {
                field,
                size: value.len(),
            });
        }
    }
    if input.digest.is_some_and(|digest| digest.trim().is_empty()) {
        return Err(RunError::EmptyEvidenceField("digest"));
    }
    if input
        .digest
        .is_some_and(|digest| digest.len() > MAX_EVIDENCE_FIELD_BYTES)
    {
        return Err(RunError::EvidenceFieldTooLarge {
            field: "digest",
            size: input.digest.map_or(0, String::len),
        });
    }
    if run
        .evidence
        .iter()
        .any(|submission| submission.id == input.evidence_id)
    {
        return Err(RunError::DuplicateEvidence(input.evidence_id.to_owned()));
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), RunError> {
    if id.trim().is_empty() {
        return Err(RunError::EmptyEvidenceField("id"));
    }
    if id.len() > MAX_EVIDENCE_ID_BYTES {
        return Err(RunError::EvidenceIdTooLarge(id.len()));
    }
    Ok(())
}
