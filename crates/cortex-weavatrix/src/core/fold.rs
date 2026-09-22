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

/// Whether `value` is a source file name (`api.rs`) or a repo-relative path.
#[must_use]
pub fn is_source_file_name(value: &str) -> bool {
    let lower = fold_text(value);
    SOURCE_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '_' | '-' | '.')
        })
}

/// Graph tools take a symbol. A path or `api.rs` is a file, not a label.
#[must_use]
pub fn is_graph_symbol(value: &str) -> bool {
    !is_repo_source_path(value) && !is_source_file_name(value)
}

/// First identifier that is a real symbol, not a crate or file path.
#[must_use]
pub fn graph_seed(identifiers: &[String]) -> Option<&str> {
    identifiers
        .iter()
        .map(String::as_str)
        .find(|identifier| is_graph_symbol(identifier))
}

/// Repository-relative source files named in the identifiers.
#[must_use]
pub fn named_source_files(identifiers: &[String]) -> Vec<String> {
    identifiers
        .iter()
        .filter(|identifier| {
            is_repo_source_path(identifier)
                && SOURCE_SUFFIXES
                    .iter()
                    .any(|suffix| fold_text(identifier).ends_with(suffix))
        })
        .cloned()
        .collect()
}

/// Crate/app/`src` directory named in the task, if any.
#[must_use]
pub fn named_crate_scope(identifiers: &[String]) -> Option<String> {
    repo_path_scope(identifiers).or_else(|| {
        identifiers
            .iter()
            .find(|identifier| is_repo_source_path(identifier))
            .cloned()
    })
}

/// A repository-relative crate, app, or `src/` path named in a task.
///
/// `crates/sweeploom-cli` is an identifier. Hyphenated prose such as
/// `warning-only` is not.
#[must_use]
pub fn is_repo_source_path(value: &str) -> bool {
    let lower = fold_text(value);
    let rooted =
        lower.starts_with("crates/") || lower.starts_with("apps/") || lower.starts_with("src/");
    rooted
        && (3..=160).contains(&value.len())
        && !value.ends_with('/')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '_' | '-' | '.')
        })
}

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
    search_glob_in(identifiers, None)
}

/// Same as [`search_glob`], but a repository inventory can replace the
/// multi-language default when the task names no suffix.
#[must_use]
pub fn search_glob_in(identifiers: &[String], inventory_glob: Option<&str>) -> String {
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
    let file_glob = match suffixes.as_slice() {
        [] => inventory_glob.unwrap_or(DEFAULT_SOURCE_GLOB).to_owned(),
        [only] => format!("**/*{only}"),
        many => {
            let inner = many
                .iter()
                .map(|suffix| suffix.trim_start_matches('.'))
                .collect::<Vec<_>>()
                .join(",");
            format!("**/*.{{{inner}}}")
        }
    };
    if let Some(scope) = repo_path_scope(identifiers) {
        return format!("{scope}/{file_glob}");
    }
    file_glob
}

fn repo_path_scope(identifiers: &[String]) -> Option<String> {
    for identifier in identifiers {
        if !is_repo_source_path(identifier) {
            continue;
        }
        let lower = fold_text(identifier);
        if SOURCE_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix)) {
            return identifier
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_owned());
        }
    }
    identifiers
        .iter()
        .find(|identifier| {
            is_repo_source_path(identifier)
                && !SOURCE_SUFFIXES
                    .iter()
                    .any(|suffix| fold_text(identifier).ends_with(suffix))
        })
        .cloned()
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
        assert_eq!(search_glob(&["src/format.ts".to_owned()]), "src/**/*.ts");
        assert_eq!(
            search_glob(&["ArchiveOptions".to_owned()]),
            DEFAULT_SOURCE_GLOB
        );
        assert_eq!(
            search_glob(&["crates/sweeploom-cli".to_owned()]),
            format!("crates/sweeploom-cli/{DEFAULT_SOURCE_GLOB}")
        );
        assert!(!is_repo_source_path("warning-only"));
        assert!(is_repo_source_path("crates/sweeploom-cli"));
        assert!(is_source_file_name("api.rs"));
        assert!(!is_graph_symbol("crates/sweeploom-cli/src/api.rs"));
        assert_eq!(
            graph_seed(&[
                "crates/sweeploom-cli/src/api.rs".to_owned(),
                "RetryLimitTooLarge".to_owned()
            ]),
            Some("RetryLimitTooLarge")
        );
        assert_eq!(
            named_source_files(&["crates/sweeploom-cli/src/api.rs".to_owned()]),
            ["crates/sweeploom-cli/src/api.rs"]
        );
    }
}
