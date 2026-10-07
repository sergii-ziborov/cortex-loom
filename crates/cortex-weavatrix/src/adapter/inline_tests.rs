//! One bounded source read for tests inside a named Rust owner file.

use std::fs;
use std::path::Path;

use serde_json::json;
use weavatrix_rust::Weavatrix;

use super::evidence::{EvidenceFragment, EvidenceKind, fragments, native_call};

pub(super) fn append(
    engine: &mut Weavatrix,
    root: &Path,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    owner_path: &str,
    task: &str,
    budget: u32,
) {
    if !crate::plan_intent::asks_for_test_source(task)
        || !cortex_context::safe_relative_path(owner_path)
    {
        return;
    }
    let identifiers = crate::plan::extract_identifiers(task);
    let Some(symbol) = crate::fold::graph_seed(&identifiers) else {
        return;
    };
    if evidence.iter().any(|fragment| {
        fragment.kind == EvidenceKind::SourceReads
            && fragment.locator.path.as_deref() == Some(owner_path)
            && fragment.content.contains("#[cfg(test)]")
            && fragment.content.contains("#[test]")
            && fragment
                .content
                .to_ascii_lowercase()
                .contains(&symbol.to_ascii_lowercase())
    }) {
        return;
    }
    let Ok(path) = root.join(owner_path).canonicalize() else {
        return;
    };
    if !path.starts_with(root) || !path.is_file() {
        return;
    }
    let Ok(source) = fs::read_to_string(&path) else {
        return;
    };
    let Some(line) = inline_test_section(&source, symbol) else {
        return;
    };
    let arguments = json!({
        "path": owner_path,
        "start_line": line,
        "before": 0,
        "after": 64,
        "token_budget": (budget / 4).clamp(500, 900),
    });
    match native_call(engine, root, "read_source", arguments) {
        Ok(value) => {
            let mut added = fragments(
                "WX-INLINE-TEST",
                EvidenceKind::SourceReads,
                "weavatrix:read_source inline_tests",
                &value,
            );
            for fragment in &mut added {
                fragment.locator.path = Some(owner_path.to_owned());
            }
            if !added.is_empty() {
                warnings.push(format!("inline test source: {owner_path}:{line}"));
                evidence.extend(added);
            }
        }
        Err(error) => warnings.push(format!("inline test source unavailable: {error}")),
    }
}

fn inline_test_section(source: &str, symbol: &str) -> Option<u32> {
    let lines: Vec<&str> = source.lines().collect();
    let symbol = symbol.to_ascii_lowercase();
    let mut section = None;
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]" {
            section = Some(index);
        }
        if line.trim() == "#[test]"
            && lines[index..lines.len().min(index + 48)]
                .iter()
                .any(|body| body.to_ascii_lowercase().contains(&symbol))
        {
            return u32::try_from(section? + 1).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::inline_test_section;

    #[test]
    fn finds_the_inline_suite_that_mentions_the_named_owner() {
        let source = "struct PacketStore {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn keeps_packets() { let _ = PacketStore {}; }\n}";
        assert_eq!(inline_test_section(source, "PacketStore"), Some(2));
        assert_eq!(inline_test_section(source, "OtherStore"), None);
    }
}
