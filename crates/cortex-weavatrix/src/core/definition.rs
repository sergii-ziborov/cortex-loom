//! Definition completeness from graph spans first, braces/indent second.

use cortex_context::EvidenceLocator;

use crate::source_followup::{definition_head_index, definition_is_complete};

/// True when the graph already named the extent and the read covers it.
#[must_use]
pub fn span_covers_definition(text: &str, locator: &EvidenceLocator) -> bool {
    let (Some(start), Some(end)) = (locator.start_line, locator.end_line) else {
        return false;
    };
    if end < start {
        return false;
    }
    let needed = end.saturating_sub(start).saturating_add(1);
    let got = u32::try_from(text.lines().count()).unwrap_or(u32::MAX);
    got >= needed
}

/// Prefer a Weavatrix node span; then braces; then an indented block
/// (Python) or a colon-terminated head (Go/Python signatures).
#[must_use]
pub fn definition_complete(
    text: &str,
    symbol: &str,
    locator: Option<&EvidenceLocator>,
) -> Option<bool> {
    definition_head_index(text, symbol)?;
    if let Some(locator) = locator
        && span_covers_definition(text, locator)
    {
        return Some(true);
    }
    if let Some(true) = definition_is_complete(text, symbol) {
        return Some(true);
    }
    if indent_block_complete(text, symbol) {
        return Some(true);
    }
    definition_is_complete(text, symbol)
}

fn indent_block_complete(text: &str, symbol: &str) -> bool {
    let Some(head) = definition_head_index(text, symbol) else {
        return false;
    };
    let rest = &text[head..];
    let mut lines = rest.lines();
    let Some(first) = lines.next() else {
        return false;
    };
    // A later `{` in the same fragment (JSON tail, another item) is not
    // this definition. Only a brace on the head means C-like syntax.
    if first.contains('{') {
        return false;
    }
    let head_indent = first.chars().take_while(|ch| ch.is_whitespace()).count();
    let colon_head = first.trim_end().ends_with(':');
    let mut body = 0_usize;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.chars().take_while(|ch| ch.is_whitespace()).count();
        if indent <= head_indent {
            return body > 0 || colon_head;
        }
        body += 1;
    }
    body > 0 || colon_head
}

#[cfg(test)]
mod tests {
    use super::{definition_complete, span_covers_definition};
    use cortex_context::EvidenceLocator;

    #[test]
    fn a_graph_span_beats_missing_braces() {
        let text = "def archive_options():\n    return False\n";
        let locator = EvidenceLocator {
            start_line: Some(1),
            end_line: Some(2),
            ..EvidenceLocator::default()
        };
        assert!(span_covers_definition(text, &locator));
        assert_eq!(
            definition_complete(text, "archive_options", Some(&locator)),
            Some(true)
        );
    }

    #[test]
    fn a_python_indent_block_is_complete_without_braces() {
        let text = "class ArchiveOptions:\n    enabled = True\n    max_entries = 32\n";
        assert_eq!(
            definition_complete(text, "ArchiveOptions", None),
            Some(true)
        );
    }

    #[test]
    fn a_python_def_stays_complete_when_a_later_brace_is_unrelated() {
        let text = "PY_RETRY_CAP = 8\n\ndef schedule_py_retry(attempt: int) -> int:\n    if attempt >= PY_RETRY_CAP:\n        return 0\n    return 2**attempt\n";
        assert_eq!(
            definition_complete(text, "schedule_py_retry", None),
            Some(true)
        );
        let with_json_tail = format!("{text}{{\"path\":\"retry.py\"}}\n");
        assert_eq!(
            definition_complete(&with_json_tail, "schedule_py_retry", None),
            Some(true)
        );
    }
}
