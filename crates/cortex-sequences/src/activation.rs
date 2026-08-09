use std::cmp::Reverse;

use serde::{Deserialize, Serialize};

use crate::templates;

const MAX_CANDIDATES: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCandidate {
    pub template_id: String,
    pub deterministic_score: u16,
    pub matched_hints: Vec<String>,
}

/// Select a small deterministic candidate set from template-owned hints.
///
/// The function recommends methodology only. It never changes node executor,
/// risk, mutation, evidence, or escalation authority inside a template.
#[must_use]
pub fn candidate_templates(task: &str) -> Vec<SequenceCandidate> {
    let normalized = normalize(task);
    let read_only = contains_any(
        &normalized,
        &[
            "read only",
            "read-only",
            "audit",
            "analy",
            "inspect",
            "explain",
        ],
    );
    let mutation = contains_any(
        &normalized,
        &[
            "implement",
            "build",
            "fix",
            "change",
            "edit",
            "refactor",
            "remove",
            "rename",
            "merge",
            "release",
            "publish",
        ],
    );
    let mut candidates: Vec<_> = templates()
        .iter()
        .filter_map(|template| {
            let mut score = 0_u16;
            let mut matched = Vec::new();
            score_hints(
                &normalized,
                template.activation.lexical_cues,
                12,
                "cue",
                &mut score,
                &mut matched,
            );
            score_hints(
                &normalized,
                template.activation.intents,
                7,
                "intent",
                &mut score,
                &mut matched,
            );
            score_hints(
                &normalized,
                template.activation.task_classes,
                4,
                "class",
                &mut score,
                &mut matched,
            );
            score_hints(
                &normalized,
                template.activation.evidence_classes,
                2,
                "evidence",
                &mut score,
                &mut matched,
            );
            if mutation && template.activation.mutation {
                score = score.saturating_add(3);
                matched.push("mutation:allowed".to_owned());
            }
            if read_only && !template.activation.mutation {
                score = score.saturating_add(4);
                matched.push("mutation:read-only".to_owned());
            }
            (score > 0).then(|| SequenceCandidate {
                template_id: template.id.to_owned(),
                deterministic_score: score,
                matched_hints: matched,
            })
        })
        .collect();
    candidates.sort_by_key(|candidate| {
        (
            Reverse(candidate.deterministic_score),
            candidate.template_id.clone(),
        )
    });
    candidates.truncate(MAX_CANDIDATES);
    if candidates.is_empty() {
        candidates.push(SequenceCandidate {
            template_id: "discover-and-plan".to_owned(),
            deterministic_score: 1,
            matched_hints: vec!["fallback:unclear".to_owned()],
        });
    }
    candidates
}

fn score_hints(
    task: &str,
    hints: &[&str],
    weight: u16,
    kind: &str,
    score: &mut u16,
    matched: &mut Vec<String>,
) {
    for hint in hints {
        let hint = normalize(hint);
        if task.contains(&hint) {
            *score = score.saturating_add(weight);
            matched.push(format!("{kind}:{hint}"));
        }
    }
}

fn contains_any(task: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| task.contains(needle))
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character == '-' {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use cortex_domain::NodeKind;

    use super::*;
    use crate::instantiate_template;

    #[test]
    fn expected_template_is_retained_across_twenty_eight_tasks() {
        let cases = [
            ("plan a small API contract change", "discover-and-plan"),
            (
                "design the storage boundary before coding",
                "discover-and-plan",
            ),
            ("understand the impact of this request", "discover-and-plan"),
            (
                "implement the approved parser change",
                "bounded-implementation",
            ),
            ("build the new adapter with tests", "bounded-implementation"),
            (
                "refactor this module in small slices",
                "bounded-implementation",
            ),
            (
                "fix the confirmed off by one defect",
                "bounded-implementation",
            ),
            (
                "debug a reproducible concurrency failure",
                "root-cause-debugging",
            ),
            ("find the root cause of this hang", "root-cause-debugging"),
            (
                "diagnose the failing integration test",
                "root-cause-debugging",
            ),
            (
                "explain this runtime error before fixing",
                "root-cause-debugging",
            ),
            (
                "review this patch for contract regressions",
                "review-and-correct",
            ),
            ("address verified review feedback", "review-and-correct"),
            (
                "correct the actionable review comments",
                "review-and-correct",
            ),
            ("verify every acceptance criterion", "verify-and-integrate"),
            (
                "finish and commit the approved work",
                "verify-and-integrate",
            ),
            ("prepare a guarded release", "verify-and-integrate"),
            ("merge only after verification", "verify-and-integrate"),
            (
                "investigate three independent questions in parallel",
                "parallel-investigation",
            ),
            (
                "compare independent implementations read-only",
                "parallel-investigation",
            ),
            (
                "parallelize the audit without shared state",
                "parallel-investigation",
            ),
            ("author a new editable sequence", "sequence-authoring"),
            (
                "adapt this workflow from benchmark failures",
                "sequence-authoring",
            ),
            ("edit the methodology template", "sequence-authoring"),
            (
                "evaluate whether this skill sequence regressed",
                "sequence-authoring",
            ),
            (
                "audit source and contracts without edits",
                "parallel-investigation",
            ),
            ("plan a configuration env flag rollout", "discover-and-plan"),
            (
                "review a dirty worktree before release",
                "review-and-correct",
            ),
        ];
        for (task, expected) in cases {
            let candidates = candidate_templates(task);
            assert!(
                candidates.iter().any(|item| item.template_id == expected),
                "{task:?} did not retain {expected}: {candidates:?}"
            );
            assert!(candidates.len() <= MAX_CANDIDATES);
        }
    }

    #[test]
    fn high_risk_recommendation_never_removes_upstream_authority() {
        for task in [
            "debug a production concurrency race",
            "plan an authentication migration",
            "prepare a guarded release",
        ] {
            for candidate in candidate_templates(task) {
                let graph = instantiate_template(&candidate.template_id, "copy", "Copy").unwrap();
                assert!(
                    graph
                        .nodes
                        .iter()
                        .any(|node| node.kind == NodeKind::UpstreamAgent),
                    "{} lost upstream authority for {task}",
                    candidate.template_id
                );
            }
        }
    }

    #[test]
    fn unknown_tasks_fail_closed_to_discovery() {
        assert_eq!(
            candidate_templates("frobnicate the wobble")[0].template_id,
            "discover-and-plan"
        );
    }
}
