/// Most distinct source windows to open after a search. Beyond this the
/// follow-up starts to resemble a directory sweep.
pub const MAX_SOURCE_FILES: usize = 6;

/// Lines kept above and below each hit.
///
/// `serve_http` sits ~20 lines above the `/mcp` route registration; keep a
/// generous above-window so the entry point lands in the same read.
pub const SOURCE_BEFORE: u32 = 24;
pub const SOURCE_AFTER: u32 = 48;

/// A schema hit should name the field being changed. A generic `"operation"`
/// property in a different MCP tool is not the requested `"priority"` field.
pub(super) fn schema_key_mentioned(task: &str, line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    task.split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|word| word.len() >= 5 && !matches!(*word, "schema" | "context_compile"))
        .any(|word| line.contains(&format!("\"{}\":", word.to_ascii_lowercase())))
}

/// Window shape for one gather pass.
///
/// A broad, enumerating question needs more and larger windows than an
/// identifier question: the measured cross-cutting probe compiled barely half
/// its budget and lost every fact that lived one file away from the hits.
/// Breadth widens the follow-up deterministically; the compiler budget is
/// still the ceiling, so a widened gather can never overrun the packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceWindow {
    /// Distinct files to open.
    pub max_files: usize,
    /// Lines above each hit.
    pub before: u32,
    /// Lines below each hit.
    pub after: u32,
    /// Numerator of the budget share the source pool may use (denominator 5).
    pub pool_fifths: u32,
    /// Extra per-file ceiling. `0` means the pool split is the only cap.
    pub max_per_file: u32,
}

impl SourceWindow {
    /// Window for one task: enumerating questions get the wide shape.
    ///
    /// The pool grew to four fifths once graph answers rendered as text:
    /// a broad question was compiling 2 518 of 4 000 and still missing
    /// one-file-away facts.
    #[must_use]
    pub fn for_task(task: &str) -> Self {
        if crate::plan_intent::is_broad(task) {
            Self {
                max_files: 9,
                before: SOURCE_BEFORE,
                after: 120,
                pool_fifths: 4,
                max_per_file: 0,
            }
        } else if crate::plan_intent::detect(task) == crate::plan_intent::TaskIntent::TestSelection
        {
            // Head + one later site: JSON token_budget from line 1 misses line 50.
            Self {
                max_files: 2,
                before: SOURCE_BEFORE,
                after: 32,
                pool_fifths: 1,
                max_per_file: 280,
            }
        } else if crate::plan::is_new_feature_without_owner(task) {
            // A code change needs enough of each owning file to edit it.
            // Six 200-token slices often repeat one file and omit the body.
            Self {
                max_files: if crate::plan_intent::asks_for_ui(task)
                    && task.to_ascii_lowercase().contains("mcp")
                {
                    if crate::plan_intent::asks_for_test_source(task) {
                        5
                    } else {
                        4
                    }
                } else {
                    3
                },
                pool_fifths: 3,
                after: if crate::plan_intent::asks_for_test_source(task) {
                    96
                } else {
                    SOURCE_AFTER
                },
                ..Self::default()
            }
        } else {
            Self::default()
        }
    }
}

impl Default for SourceWindow {
    fn default() -> Self {
        Self {
            max_files: MAX_SOURCE_FILES,
            before: SOURCE_BEFORE,
            after: SOURCE_AFTER,
            pool_fifths: 2,
            max_per_file: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SearchHit, unique_paths_for_patterns};
    use super::schema_key_mentioned;

    #[test]
    fn schema_bonus_requires_the_requested_property_name() {
        let task = "Add Elevated priority to MCP context_compile schema";
        assert!(schema_key_mentioned(
            task,
            "\"priority\": {\"type\": \"string\"}"
        ));
        assert!(!schema_key_mentioned(
            task,
            "\"operation\": {\"enum\": [\"context_compile\"]}"
        ));
    }

    #[test]
    fn requested_ui_survives_preservation_checks_in_a_four_file_change() {
        let task =
            "Add priority with MCP schema and UI help; preserve Critical fail-closed behavior";
        let hits = [
            ("crates/core/src/evidence.rs", "pub enum EvidencePriority"),
            (
                "crates/mcp/src/tools.rs",
                "\"priority\": {\"type\": \"string\"}",
            ),
            ("crates/core/src/lib.rs", "CriticalItemExceedsBudget"),
            (
                "crates/adapter/src/coverage.rs",
                "CriticalItemExceedsBudget",
            ),
            ("ui/src/help.ts", "priority help"),
        ]
        .map(|(path, text)| SearchHit {
            path: path.to_owned(),
            line: 8,
            text: text.to_owned(),
        });
        let chosen =
            unique_paths_for_patterns(&hits, 4, &["criticalitemexceedsbudget".to_owned()], task);
        assert!(
            chosen.iter().any(|hit| hit.path == "ui/src/help.ts"),
            "{chosen:?}"
        );
    }
}
