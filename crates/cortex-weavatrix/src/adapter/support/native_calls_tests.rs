use super::*;

#[test]
fn brace_globs_expand_in_order() {
    assert_eq!(
        expand_braces("**/*.{rs,ts,tsx}").unwrap(),
        ["**/*.rs", "**/*.ts", "**/*.tsx"]
    );
    assert_eq!(
        expand_braces("{crates,apps}/**/*.{rs,toml}").unwrap(),
        [
            "crates/**/*.rs",
            "crates/**/*.toml",
            "apps/**/*.rs",
            "apps/**/*.toml"
        ]
    );
    assert_eq!(
        expand_braces("src/{a,{b,c}}.rs").unwrap(),
        ["src/a.rs", "src/b.rs", "src/c.rs"]
    );
    assert_eq!(expand_braces("**/*.rs").unwrap(), ["**/*.rs"]);
    assert_eq!(expand_braces("src/{a,b.rs").unwrap(), ["src/{a,b.rs"]);
    assert_eq!(expand_braces("*.{rs,rs}").unwrap(), ["*.rs"]);
}

#[test]
fn too_many_alternatives_keep_the_original_glob() {
    let wide = format!(
        "**/*.{{{}}}",
        (0..40)
            .map(|index| format!("x{index}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_eq!(expand_braces(&wide), None);
}

fn hit(path: &str, line: u64) -> Value {
    json!({"path": path, "line": line, "text": "priority", "spans": []})
}

fn part(matches: &[Value], truncated: bool, dropped: u64) -> Value {
    let count = matches.len();
    json!({
        "backend": "liveindex",
        "matches": matches,
        "totals": {
            "matching_lines": count,
            "occurrences": count,
            "files_with_matches": count,
            "returned_matches": count
        },
        "files_searched": 9,
        "truncated": truncated,
        "warnings": [{"kind": "binary", "path": "a.png", "message": "skipped"}],
        "token_budget": {"requested": 400, "dropped_items": dropped, "fit": true, "applied": true}
    })
}

#[test]
fn merged_search_is_path_ordered_deduplicated_and_capped() {
    let merged = merge_search(
        &[
            part(&[hit("ui/b.ts", 3), hit("ui/a.ts", 9)], false, 0),
            part(&[hit("crates/x.rs", 1), hit("ui/a.ts", 9)], false, 0),
        ],
        &json!({"max_results": 2}),
    );
    let paths = merged["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["path"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["crates/x.rs", "ui/a.ts"]);
    assert_eq!(merged["truncated"], true);
    assert_eq!(merged["totals"]["returned_matches"], 2);
    assert_eq!(merged["totals"]["matching_lines"], 4);
    assert_eq!(merged["warnings"].as_array().unwrap().len(), 1);
    assert!(merged.get("token_budget").is_none());
}

#[test]
fn merged_search_refits_the_requested_budget() {
    let many = |prefix: &str| {
        (1..=20)
            .map(|line| hit(&format!("{prefix}/file.rs"), line))
            .collect::<Vec<_>>()
    };
    let merged = merge_search(
        &[part(&many("a"), false, 2), part(&many("b"), false, 1)],
        &json!({"max_results": 40, "token_budget": 400}),
    );
    let receipt = &merged["token_budget"];
    let kept = merged["matches"].as_array().unwrap().len();
    assert!(kept < 40, "{merged}");
    assert_eq!(receipt["requested"], 400);
    assert_eq!(receipt["fit"], true);
    assert_eq!(receipt["applied"], true);
    assert_eq!(receipt["dropped_items"], json!(3 + 40 - kept));
    assert!(receipt["estimated_tokens"].as_u64().unwrap() <= 400);
    assert_eq!(merged["truncated"], true);
    assert_eq!(merged["matches"][0]["path"], "a/file.rs");
}

#[test]
fn brace_search_reaches_every_alternative_on_the_resident_index() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut engine = Weavatrix::open(&root).unwrap();
    let value = call(
        &mut engine,
        "search_code",
        json!({
            "query": "fn expand_braces|export default function App",
            "is_regex": true,
            "glob": "{crates,ui/src}/**/*.{rs,tsx}",
            "max_results": 40
        }),
    )
    .unwrap();
    let paths = value["matches"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        paths.iter().any(|path| path.ends_with("native_calls.rs")),
        "{value}"
    );
    assert!(paths.contains(&"ui/src/App.tsx"), "{value}");
}

#[test]
fn memory_context_compiles_from_the_library() {
    let memory = crate::PriorRunMemory::from_parts(vec![crate::PriorRunEvent {
        run_id: "run-1".to_owned(),
        sequence: 4,
        kind: "node_failed".to_owned(),
        node_id: Some("root".to_owned()),
        detail: Some("evidence gate rejected".to_owned()),
        recorded_at: 1_700_000_000,
    }]);
    let arguments = memory
        .memory_arguments("still failing compile_context", 600)
        .unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut engine = Weavatrix::open(root).unwrap();
    let value = call(&mut engine, "memory_context", arguments).unwrap();
    assert_eq!(value["mutation"], "NONE");
    assert_eq!(value["view"]["facts"][0]["relation"], "follows_attempt");
    assert!(call(&mut engine, "memory_context", json!({})).is_err());
}

#[test]
fn an_ambiguous_reference_label_stays_an_error_naming_candidates() {
    let root = std::env::temp_dir().join(format!("cortex-ambiguous-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), "pub fn target() {}\n").unwrap();
    std::fs::write(root.join("src/b.rs"), "pub fn target() {}\n").unwrap();
    let mut engine = Weavatrix::open(&root).unwrap();
    let error = call(&mut engine, "find_references", json!({"label": "target"})).unwrap_err();
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        error.starts_with("ambiguous node label \"target\"; use one of: "),
        "{error}"
    );
    assert!(
        error.contains("src/a.rs") && error.contains("src/b.rs"),
        "{error}"
    );
}
