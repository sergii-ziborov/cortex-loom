//! Claim-level digest contract. A citation id is not a true statement.
//!
//! A digest may only emit atoms that can be checked against the cited
//! evidence. Prose, if needed, is rendered from verified atoms afterwards.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestClaim {
    pub subject: String,
    pub relation: String,
    pub object: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub source_spans: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DigestDocument {
    #[serde(default)]
    pub claims: Vec<DigestClaim>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimEvidence<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub content: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimCheck {
    pub ok: bool,
    pub errors: Vec<String>,
}

/// Verify every claim against the cited evidence bodies.
///
/// A claim is accepted only when: every evidence id exists; the subject and
/// object literals occur in those bodies; a source span, if given, names a
/// file that the cited evidence source mentions; and the object is not a
/// negation of a literal that the evidence states the other way.
#[must_use]
pub fn verify_claims(claims: &[DigestClaim], evidence: &[ClaimEvidence<'_>]) -> ClaimCheck {
    let mut errors = Vec::new();
    if claims.is_empty() {
        errors.push("digest emitted no claims".to_owned());
        return ClaimCheck { ok: false, errors };
    }
    for (index, claim) in claims.iter().enumerate() {
        check_one(index, claim, evidence, &mut errors);
    }
    ClaimCheck {
        ok: errors.is_empty(),
        errors,
    }
}

fn check_one(
    index: usize,
    claim: &DigestClaim,
    evidence: &[ClaimEvidence<'_>],
    errors: &mut Vec<String>,
) {
    if claim.subject.trim().is_empty() || claim.object.trim().is_empty() {
        errors.push(format!("claim {index} has an empty subject or object"));
        return;
    }
    if claim.evidence_ids.is_empty() {
        errors.push(format!("claim {index} cites no evidence"));
        return;
    }
    let mut cited = Vec::new();
    for id in &claim.evidence_ids {
        match evidence.iter().find(|item| item.id == id) {
            Some(item) => cited.push(*item),
            None => errors.push(format!("claim {index} cites unknown id {id}")),
        }
    }
    if cited.is_empty() {
        return;
    }
    let blob: String = cited
        .iter()
        .map(|item| item.content)
        .collect::<Vec<_>>()
        .join("\n");
    if !blob.contains(claim.subject.trim()) {
        errors.push(format!(
            "claim {index} subject {:?} is not in the cited evidence",
            claim.subject
        ));
    }
    if !blob.contains(claim.object.trim()) {
        errors.push(format!(
            "claim {index} object {:?} is not in the cited evidence",
            claim.object
        ));
    }
    if inverted_boolean(&blob, claim.object.trim()) {
        errors.push(format!(
            "claim {index} inverts a boolean around {:?}",
            claim.object
        ));
    }
    for span in &claim.source_spans {
        let file = span.split(':').next().unwrap_or(span);
        if !file.is_empty()
            && !cited
                .iter()
                .any(|item| item.source.contains(file) || item.content.contains(file))
        {
            errors.push(format!(
                "claim {index} source span {span} is not backed by cited evidence"
            ));
        }
    }
}

fn inverted_boolean(blob: &str, object: &str) -> bool {
    let lower = blob.to_ascii_lowercase();
    match object.trim() {
        "true" => lower.contains("= false") || lower.contains(": false"),
        "false" => lower.contains("= true") || lower.contains(": true"),
        _ => false,
    }
}

/// Required atoms that a digest must emit, compared by subject+relation.
#[must_use]
pub fn missing_required(
    required: &[DigestClaim],
    emitted: &[DigestClaim],
) -> Vec<(String, String)> {
    required
        .iter()
        .filter(|need| {
            !emitted.iter().any(|got| {
                got.subject == need.subject
                    && got.relation == need.relation
                    && got.object == need.object
            })
        })
        .map(|need| (need.subject.clone(), need.relation.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> [ClaimEvidence<'static>; 1] {
        [ClaimEvidence {
            id: "WX-DEF",
            source: "src/options/types.rs:41",
            content: "pub enabled: bool = false; // ArchiveOptions.enabled default",
        }]
    }

    fn claim() -> DigestClaim {
        DigestClaim {
            subject: "ArchiveOptions.enabled".to_owned(),
            relation: "default_value".to_owned(),
            object: "false".to_owned(),
            evidence_ids: vec!["WX-DEF".to_owned()],
            source_spans: vec!["src/options/types.rs:41-41".to_owned()],
        }
    }

    #[test]
    fn a_literal_in_the_cited_span_is_accepted() {
        let check = verify_claims(&[claim()], &evidence());
        assert!(check.ok, "{:?}", check.errors);
    }

    #[test]
    fn an_invented_object_is_rejected() {
        let mut claim = claim();
        claim.object = "true".to_owned();
        let check = verify_claims(&[claim], &evidence());
        assert!(!check.ok);
        assert!(check.errors.iter().any(|error| error.contains("object")));
    }

    #[test]
    fn an_unknown_citation_is_rejected() {
        let mut claim = claim();
        claim.evidence_ids = vec!["WX-MISSING".to_owned()];
        assert!(!verify_claims(&[claim], &evidence()).ok);
    }
}
