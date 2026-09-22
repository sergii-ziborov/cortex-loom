//! Path/line hits parsed from panics and structural JSON.

use super::SearchHit;

/// Prefer the head of a test suite on a test-selection question.
///
/// Search for the changed symbol fills every slot with mid-file calls
/// (`compile_context(` at line 25, 61, 110…). The names of those tests
/// sit at the top of the file.
#[must_use]
pub fn test_suite_head_bonus(hit: &SearchHit, task: &str) -> i32 {
    if crate::plan_intent::detect(task) != crate::plan_intent::TaskIntent::TestSelection
        || hit.line > 8
    {
        return 0;
    }
    let lower = hit.path.replace('\\', "/").to_ascii_lowercase();
    if lower.ends_with("tests.rs") || lower.contains("/tests/") || lower.contains(".test.") {
        200
    } else {
        0
    }
}

/// Keep the suite that lives next to the changed symbol, not every
/// `tests.rs:1` `select_tests` named from a dirty working tree.
#[must_use]
pub fn same_crate_test_bonus(hit: &SearchHit, hits: &[SearchHit], task: &str) -> i32 {
    if crate::plan_intent::detect(task) != crate::plan_intent::TaskIntent::TestSelection {
        return 0;
    }
    let path = hit.path.replace('\\', "/");
    if !already_a_test_path(&path) {
        return 0;
    }
    let Some(root) = crate_root(&path) else {
        return 0;
    };
    let identifiers = crate::plan::extract_identifiers(task);
    let owner_mentions_symbol = hits.iter().any(|other| {
        let other_path = other.path.replace('\\', "/");
        if already_a_test_path(&other_path) || fixture_task_list(&other_path) {
            return false;
        }
        crate_root(&other_path) == Some(root)
            && identifiers.iter().any(|identifier| {
                other
                    .text
                    .to_ascii_lowercase()
                    .contains(&identifier.to_ascii_lowercase())
            })
    });
    if owner_mentions_symbol { 80 } else { -300 }
}

fn crate_root(path: &str) -> Option<&str> {
    path.find("/src/")
        .or_else(|| path.find("/tests/"))
        .map(|index| &path[..index])
}

pub(crate) fn fixture_task_list(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    lower.ends_with("/tasks.rs")
        || lower.ends_with("/probe_tasks.rs")
        || lower.ends_with("/lang_tasks.rs")
        || lower.ends_with("/intent_tasks.rs")
        || lower.ends_with("/agent_cases.rs")
        || lower.ends_with("/questions.rs")
        || lower.contains("/bench/")
}

/// Keep a catalog file only when the task names that file.
///
/// Otherwise search for a crate or `api.rs` path lands on the ask list that
/// quotes the task (`agent_cases.rs`), and the packet steers the agent to
/// unused catalog helpers instead of the production target.
pub(crate) fn keep_search_hit(path: &str, task: &str) -> bool {
    !fixture_task_list(path) || task_names_path(task, path)
}

pub(crate) fn keep_product_hits(hits: &[SearchHit], task: &str) -> Vec<SearchHit> {
    hits.iter()
        .filter(|hit| keep_search_hit(&hit.path, task))
        .cloned()
        .collect()
}

pub(crate) fn task_names_path(task: &str, path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or(&normalized);
    let task_fold = crate::fold::fold_text(task);
    task_fold.contains(&crate::fold::fold_text(name))
        || task_fold.contains(&crate::fold::fold_text(&normalized))
}

/// Drop ask-catalog matches from a `search_code` payload so the rendered
/// packet does not quote the bench questions as if they were the target.
pub fn retain_product_search_matches(value: &mut serde_json::Value, task: &str) {
    let Some(matches) = value
        .get_mut("matches")
        .and_then(|item| item.as_array_mut())
    else {
        return;
    };
    matches.retain(|entry| {
        let path = entry
            .get("path")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        keep_search_hit(path, task)
    });
}

/// Open a file the task named, even when search only found catalog quotes.
pub fn prepend_named_source_hits(hits: &mut Vec<SearchHit>, task: &str) {
    let extra: Vec<SearchHit> = crate::plan::extract_identifiers(task)
        .into_iter()
        .filter(|identifier| {
            crate::fold::is_repo_source_path(identifier)
                && crate::fold::SOURCE_SUFFIXES
                    .iter()
                    .any(|suffix| crate::fold::fold_text(identifier).ends_with(suffix))
                && keep_search_hit(identifier, task)
                && !hits
                    .iter()
                    .any(|seen| seen.path.replace('\\', "/") == identifier.replace('\\', "/"))
        })
        .map(|path| SearchHit {
            path,
            line: 1,
            text: String::new(),
        })
        .collect();
    if extra.is_empty() {
        return;
    }
    let mut combined = extra;
    combined.append(hits);
    *hits = combined;
}

/// `path.rs:LINE` pairs pasted in a panic or stack frame.
#[must_use]
pub fn hits_from_stack_text(text: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for token in text.split_whitespace() {
        let token = token.trim_matches(|c: char| c == '`' || c == '"' || c == ',');
        let Some((path, line)) = rust_path_and_line(token) else {
            continue;
        };
        if !hits.iter().any(|seen: &SearchHit| seen.path == path) {
            hits.push(SearchHit {
                path,
                line,
                text: token.to_owned(),
            });
        }
    }
    hits
}

fn rust_path_and_line(token: &str) -> Option<(String, u32)> {
    let mut path = String::new();
    let mut parts = token.split(':');
    loop {
        let segment = parts.next()?;
        if path.is_empty() {
            path.push_str(segment);
        } else {
            path.push(':');
            path.push_str(segment);
        }
        if is_rust_path(&path.replace('\\', "/")) {
            let line: u32 = parts
                .next()?
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok()?;
            return Some((path.replace('\\', "/"), line.max(1)));
        }
    }
}

/// A test-selection question needs the suite head and one later call
/// site, not four mid-file windows of the same `tests.rs`.
pub fn keep_suite_head_and_one_later(chosen: &mut Vec<SearchHit>, task: &str) {
    if crate::plan_intent::detect(task) != crate::plan_intent::TaskIntent::TestSelection {
        return;
    }
    let mut suites: std::collections::BTreeMap<String, Vec<SearchHit>> =
        std::collections::BTreeMap::new();
    let mut others = Vec::new();
    for hit in chosen.drain(..) {
        let path = hit.path.replace('\\', "/");
        if already_a_test_path(&path) {
            suites.entry(path).or_default().push(hit);
        } else {
            others.push(hit);
        }
    }
    let mut kept = Vec::new();
    for mut hits in suites.into_values() {
        hits.sort_by_key(|hit| hit.line);
        kept.extend(hits.into_iter().take(2));
    }
    kept.extend(others);
    *chosen = kept;
}

/// Put the owning crate's `tests.rs` ahead of mid-file search hits.
pub fn prepend_sibling_test_hits(hits: &mut Vec<SearchHit>, task: &str, symbol: Option<&str>) {
    if crate::plan_intent::detect(task) != crate::plan_intent::TaskIntent::TestSelection {
        return;
    }
    let siblings = sibling_test_hits(hits, symbol);
    if siblings.is_empty() {
        return;
    }
    let mut combined = siblings;
    combined.append(hits);
    *hits = combined;
}

/// The crate's own `src/tests.rs` for the file that defines `symbol`.
///
/// `select_tests` walks dependents and often returns a far test
/// (`adversarial.rs`) while the suite that names the change lives next
/// to the owning file. Opening that sibling from line 1 is what a
/// "which tests should I run?" question actually needs.
#[must_use]
pub fn sibling_test_hits(hits: &[SearchHit], symbol: Option<&str>) -> Vec<SearchHit> {
    let product: Vec<&SearchHit> = hits
        .iter()
        .filter(|hit| !already_a_test_path(&hit.path.replace('\\', "/")))
        .collect();
    let owner = match symbol {
        Some(symbol) if !symbol.is_empty() => product
            .iter()
            .copied()
            .find(|hit| super::definition_head_index(&hit.text, symbol).is_some())
            .or_else(|| {
                product.iter().copied().find(|hit| {
                    hit.path.replace('\\', "/").ends_with("/src/lib.rs")
                        && hit
                            .text
                            .to_ascii_lowercase()
                            .contains(&symbol.to_ascii_lowercase())
                })
            })
            .or_else(|| {
                product.iter().copied().find(|hit| {
                    hit.text
                        .to_ascii_lowercase()
                        .contains(&symbol.to_ascii_lowercase())
                })
            }),
        _ => product.first().copied(),
    };
    let mut extra = Vec::new();
    let Some(hit) = owner else {
        return extra;
    };
    let path = hit.path.replace('\\', "/");
    if let Some(src) = path.find("/src/") {
        let sibling = format!("{}{}", &path[..src], "/src/tests.rs");
        if !hits
            .iter()
            .any(|seen| seen.path.replace('\\', "/") == sibling && seen.line <= 8)
        {
            extra.push(SearchHit {
                path: sibling,
                line: 1,
                text: String::new(),
            });
        }
    }
    extra
}

fn is_rust_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

fn already_a_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with("tests.rs")
        || lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.contains(".test.")
}

/// Path and line hits from `find_dead_code` candidates and `find_duplicates` sites.
#[must_use]
pub fn hits_from_health_report(value: &serde_json::Value) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    if let Some(candidates) = value
        .get("candidates")
        .and_then(serde_json::Value::as_array)
    {
        for item in candidates {
            if let Some(hit) = hit_from_span_node(item.get("node").unwrap_or(item)) {
                push_unique_hit(&mut hits, hit);
            }
        }
    }
    if let Some(pairs) = value.get("pairs").and_then(serde_json::Value::as_array) {
        for pair in pairs {
            for side in ["left", "right"] {
                if let Some(hit) = hit_from_clone_site(pair.get(side)) {
                    push_unique_hit(&mut hits, hit);
                }
            }
        }
    }
    if let Some(families) = value.get("families").and_then(serde_json::Value::as_array) {
        for family in families {
            for member in family
                .get("members")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(hit) = hit_from_clone_site(Some(member)) {
                    push_unique_hit(&mut hits, hit);
                }
            }
        }
    }
    hits
}

fn hit_from_span_node(node: &serde_json::Value) -> Option<SearchHit> {
    let span = node.get("span")?;
    let path = span.get("file").and_then(serde_json::Value::as_str)?;
    if path.is_empty() {
        return None;
    }
    let line = span
        .get("start")
        .and_then(|start| start.get("line"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    Some(SearchHit {
        path: path.replace('\\', "/"),
        line: u32::try_from(line).unwrap_or(1).max(1),
        text: node
            .get("label")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

fn hit_from_clone_site(site: Option<&serde_json::Value>) -> Option<SearchHit> {
    let site = site?;
    let path = site.get("path").and_then(serde_json::Value::as_str)?;
    if path.is_empty() {
        return None;
    }
    let line = site
        .get("start_line")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    Some(SearchHit {
        path: path.replace('\\', "/"),
        line: u32::try_from(line).unwrap_or(1).max(1),
        text: String::new(),
    })
}

fn push_unique_hit(hits: &mut Vec<SearchHit>, hit: SearchHit) {
    if !hits
        .iter()
        .any(|seen| seen.path == hit.path && seen.line.abs_diff(hit.line) <= 2)
    {
        hits.push(hit);
    }
}

/// File paths named by `select_tests` / `map_stacktrace` JSON.
#[must_use]
pub fn hits_from_json_paths(content: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("crates/") {
        let slice = &rest[start..];
        let end = slice
            .find(|c: char| c == '"' || c == '\'' || c.is_whitespace() || c == ',')
            .unwrap_or(slice.len());
        let path = slice[..end].trim_end_matches(':').replace('\\', "/");
        rest = &slice[end.max(1)..];
        if !is_rust_path(&path) {
            continue;
        }
        if !hits.iter().any(|seen: &SearchHit| seen.path == path) {
            hits.push(SearchHit {
                path,
                line: 1,
                text: String::new(),
            });
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::hits_from_health_report;
    use serde_json::json;

    #[test]
    fn dead_code_and_clone_sites_become_source_hits() {
        let value = json!({
            "candidates": [{
                "node": {
                    "label": "leftover_path",
                    "span": {"file": "crates/sweeploom-cli/src/cleanup.rs", "start": {"line": 40}}
                }
            }],
            "pairs": [{
                "left": {"path": "crates/sweeploom-ai/src/a.rs", "start_line": 3},
                "right": {"path": "crates/sweeploom-ai/src/b.rs", "start_line": 8}
            }]
        });
        let hits = hits_from_health_report(&value);
        assert!(hits.iter().any(|hit| {
            hit.path.ends_with("cleanup.rs") && hit.line == 40 && hit.text == "leftover_path"
        }));
        assert!(
            hits.iter()
                .any(|hit| hit.path.ends_with("a.rs") && hit.line == 3)
        );
        assert!(
            hits.iter()
                .any(|hit| hit.path.ends_with("b.rs") && hit.line == 8)
        );
    }
}
