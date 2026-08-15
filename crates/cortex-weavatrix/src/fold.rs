//! Unicode fold and identifier segmentation for planning.

use unicode_normalization::UnicodeNormalization;

/// NFKC + Unicode case fold + collapsed whitespace.
#[must_use]
pub fn fold_text(value: &str) -> String {
    value
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Split an identifier into searchable pieces.
///
/// `formatGroupedResult` → `format`, `Grouped`, `Result`.
/// `foo_bar-baz` and `a::b` split on separators. Paths keep the last
/// segment as well as the suffixes.
#[must_use]
pub fn segment_identifier(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for raw in value.split(['_', '-', '.', '/', ':', '\\']) {
        if raw.is_empty() {
            continue;
        }
        split_camel(raw, &mut parts);
    }
    parts
}

fn split_camel(value: &str, parts: &mut Vec<String>) {
    let chars: Vec<char> = value.chars().collect();
    let mut start = 0;
    for index in 1..chars.len() {
        let previous = chars[index - 1];
        let current = chars[index];
        let boundary = (previous.is_lowercase() && current.is_uppercase())
            || (previous.is_uppercase()
                && current.is_uppercase()
                && chars.get(index + 1).is_some_and(|next| next.is_lowercase()));
        if boundary {
            parts.push(chars[start..index].iter().collect());
            start = index;
        }
    }
    parts.push(chars[start..].iter().collect());
}

/// Known source suffixes the identifier detector already recognises.
pub const SOURCE_SUFFIXES: &[&str] = &[
    ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".kt", ".cs", ".sql", ".proto",
    ".toml", ".json", ".yaml", ".yml",
];

/// Default search glob when the task names no file suffix.
///
/// Not `**/*.rs`: identifier detection already accepts TypeScript, SQL,
/// and proto paths, and a mixed-language repo should not be searched as
/// if it were only Rust.
pub const DEFAULT_SOURCE_GLOB: &str = "**/*.{rs,ts,tsx,js,jsx,py,go,java,kt,cs,sql,proto}";

/// Search glob from identifiers: named suffixes win, otherwise the
/// multi-language default.
#[must_use]
pub fn search_glob(identifiers: &[String]) -> String {
    let mut suffixes = Vec::new();
    for identifier in identifiers {
        let lower = fold_text(identifier);
        if let Some(suffix) = SOURCE_SUFFIXES
            .iter()
            .copied()
            .find(|candidate| lower.ends_with(candidate))
            && !suffixes.contains(&suffix)
        {
            suffixes.push(suffix);
        }
    }
    match suffixes.as_slice() {
        [] => DEFAULT_SOURCE_GLOB.to_owned(),
        [only] => format!("**/*{only}"),
        many => {
            let inner = many
                .iter()
                .map(|suffix| suffix.trim_start_matches('.'))
                .collect::<Vec<_>>()
                .join(",");
            format!("**/*.{{{inner}}}")
        }
    }
}

/// Whether a `read_source` window already covers a Weavatrix node span.
/// Prefer this over counting braces when the graph knows the extent.
#[must_use]
pub fn window_covers_span(
    hit_line: u32,
    before: u32,
    after: u32,
    span_start: u32,
    span_end: u32,
) -> bool {
    let window_start = hit_line.saturating_sub(before).max(1);
    let window_end = hit_line.saturating_add(after);
    window_start <= span_start && window_end >= span_end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_and_separators_split() {
        assert_eq!(
            segment_identifier("formatGroupedResult"),
            ["format", "Grouped", "Result"]
        );
        assert_eq!(segment_identifier("foo_bar-baz"), ["foo", "bar", "baz"]);
    }

    #[test]
    fn named_suffix_narrows_the_glob() {
        assert_eq!(search_glob(&["src/format.ts".to_owned()]), "**/*.ts");
        assert_eq!(
            search_glob(&["ArchiveOptions".to_owned()]),
            DEFAULT_SOURCE_GLOB
        );
    }
}
