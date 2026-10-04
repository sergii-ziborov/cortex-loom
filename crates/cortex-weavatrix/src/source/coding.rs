use super::{SearchHit, hits};

/// A search match may land halfway through the chosen `tests.rs`. When a
/// coding task asks to add tests, read from line 1 so imports and local test
/// helpers arrive with the assertions. Path selection still uses the owning
/// crate score; only the window location changes.
pub(super) fn open_coding_tests_from_head(chosen: &mut [SearchHit], task: &str) {
    if !crate::plan::is_new_feature_without_owner(task)
        || !crate::plan_intent::asks_for_test_source(task)
    {
        return;
    }
    for hit in chosen {
        if hits::already_a_test_path(&hit.path) {
            hit.line = 1;
        }
    }
}
