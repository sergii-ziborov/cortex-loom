use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use weavatrix_refactor_plan::RefactorOperation;
use weavatrix_rust::{RepositoryState, Weavatrix};

use super::{
    NATIVE_PLAN_OPERATIONS, native_plan, native_plan_response, planning_arguments,
    refactor_plan_from_envelope,
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
const PREFIX: &str = "cortex-weavatrix-native-plan-";
const CARGO: &str = "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n";
const PACKAGE: &str = "{\"name\":\"fixture\",\"version\":\"0.0.0\",\"type\":\"module\"}\n";

struct Repo(PathBuf);

impl Repo {
    fn new(files: &[(&str, &str)]) -> Self {
        let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("{PREFIX}{}-{suffix}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        for (path, contents) in files {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.0.join(path)).unwrap()
    }

    /// Plan and preview one operation the way the adapter does.
    fn plan(&self, operation: &str, arguments: &Value) -> Result<Value, crate::WeavatrixError> {
        let engine = Weavatrix::open(self.path()).unwrap();
        let arguments = planning_arguments(operation, arguments)?;
        let answer = native_plan(engine.state(), operation, &arguments)?;
        native_plan_response(self.path(), operation, &answer)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        assert!(
            self.0
                .file_name()
                .is_some_and(|name| { name.to_string_lossy().starts_with(PREFIX) })
        );
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

const RUST_RENAME: [(&str, &str); 5] = [
    ("Cargo.toml", CARGO),
    (
        "src/lib.rs",
        "pub mod core;\npub mod caller;\npub mod local;\n",
    ),
    (
        "src/core.rs",
        "pub fn resolve_target(value: u32) -> u32 {\n    value + 1\n}\n",
    ),
    (
        "src/caller.rs",
        "use crate::core::resolve_target;\n\npub fn run() -> u32 {\n    resolve_target(1)\n}\n",
    ),
    (
        "src/local.rs",
        "fn resolve_target(node: u32) -> u32 {\n    node\n}\n\npub fn pick() -> u32 {\n    resolve_target(2)\n}\n",
    ),
];

fn node_id(state: &RepositoryState, file: &str, label: &str) -> String {
    state
        .graph()
        .nodes()
        .iter()
        .find(|node| {
            node.label.trim_end_matches("()") == label
                && node.span.as_ref().is_some_and(|span| span.file == file)
        })
        .map(|node| node.id.as_str().to_owned())
        .expect("fixture symbol is in the graph")
}

fn assert_no_apply_authority(response: &Value) {
    let rendered = response.to_string().to_ascii_lowercase();
    for forbidden in [
        "confirmtoken",
        "confirm_token",
        "expiresat",
        "apply_edit_plan",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "forbidden field: {forbidden}"
        );
    }
    assert_eq!(response["mode"], "preview");
}

fn change<'a>(response: &'a Value, path: &str) -> &'a Value {
    response["preview"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["path"] == path)
        .unwrap_or_else(|| panic!("no preview change for {path}: {response}"))
}

#[test]
fn a_native_rename_is_previewed_and_nothing_is_written() {
    let repo = Repo::new(&RUST_RENAME);
    let id = node_id(
        Weavatrix::open(repo.path()).unwrap().state(),
        "src/core.rs",
        "resolve_target",
    );

    let response = repo
        .plan(
            "rename_symbol",
            &json!({"symbol": id, "new_name": "locate_target"}),
        )
        .unwrap();

    assert_eq!(response["status"], "PLANNED");
    assert_eq!(
        response["plan"]["schemaVersion"],
        "weavatrix.refactor-plan.v1"
    );
    assert_eq!(response["preview"]["completeness"], "PARTIAL");
    let core = change(&response, "src/core.rs");
    assert!(core["after"].as_str().unwrap().contains("fn locate_target"));
    let caller = change(&response, "src/caller.rs");
    assert!(
        caller["after"]
            .as_str()
            .unwrap()
            .contains("locate_target(1)")
    );
    assert!(
        response["preview"]["affectedPaths"]
            .as_array()
            .unwrap()
            .iter()
            .all(|path| path != "src/local.rs"),
        "the unrelated shadow stays out of the plan: {response}"
    );
    assert_no_apply_authority(&response);
    assert!(repo.read("src/core.rs").contains("fn resolve_target"));
    assert!(repo.read("src/caller.rs").contains("resolve_target(1)"));
}

#[test]
fn an_ambiguous_name_returns_candidates_and_no_preview() {
    let repo = Repo::new(&RUST_RENAME);

    let response = repo
        .plan(
            "rename_symbol",
            &json!({"symbol": "resolve_target", "new_name": "locate_target"}),
        )
        .unwrap();

    assert_eq!(response["status"], "NOT_FOUND");
    assert!(response["native"]["candidates"].as_array().unwrap().len() > 1);
    assert!(response.get("plan").is_none());
    assert!(response.get("preview").is_none());
}

#[test]
fn apply_arguments_and_writing_operations_are_refused() {
    let repo = Repo::new(&RUST_RENAME);
    for arguments in [
        json!({"symbol": "resolve_target", "new_name": "x", "mode": "apply"}),
        json!({"symbol": "resolve_target", "new_name": "x", "confirm_token": "t"}),
        json!(["resolve_target"]),
    ] {
        assert!(
            repo.plan("rename_symbol", &arguments).is_err(),
            "{arguments}"
        );
    }
    for operation in [
        "apply_edit_plan",
        "rollback_last_apply",
        "bulk_replace",
        "nope",
    ] {
        assert!(repo.plan(operation, &json!({})).is_err(), "{operation}");
    }
    assert_eq!(NATIVE_PLAN_OPERATIONS.len(), 8);
    assert!(repo.read("src/core.rs").contains("fn resolve_target"));
}

#[test]
fn a_native_signature_change_is_previewed() {
    let repo = Repo::new(&[
        ("package.json", PACKAGE),
        (
            "src/core.js",
            "export function compute(alpha, beta) {\n  return alpha + beta;\n}\n",
        ),
        (
            "src/caller.js",
            "import { compute } from './core.js';\n\nexport function run() {\n  return compute(1, 2);\n}\n",
        ),
    ]);
    let id = node_id(
        Weavatrix::open(repo.path()).unwrap().state(),
        "src/core.js",
        "compute",
    );

    let response = repo
        .plan(
            "change_signature",
            &json!({
                "symbol": id,
                "operation": {"kind": "add_parameter", "name": "gamma", "default": "0"},
            }),
        )
        .unwrap();

    assert_eq!(response["status"], "PLANNED");
    let core = change(&response, "src/core.js");
    assert!(
        core["after"]
            .as_str()
            .unwrap()
            .starts_with("export function compute(alpha, beta, gamma = 0) {")
    );
    assert_no_apply_authority(&response);
    assert!(repo.read("src/core.js").contains("compute(alpha, beta)"));
}

#[test]
fn a_native_file_move_previews_a_rename_and_moves_nothing() {
    let repo = Repo::new(&[
        ("package.json", PACKAGE),
        (
            "src/core.js",
            "export function compute() {\n  return 1;\n}\n",
        ),
        (
            "src/caller.js",
            "import { compute } from './core.js';\n\nexport function run() {\n  return compute();\n}\n",
        ),
        ("src/lib/keep.js", "export const keep = 1;\n"),
    ]);

    let response = repo
        .plan(
            "move_file",
            &json!({"from": "src/core.js", "to": "src/lib/core.js"}),
        )
        .unwrap();

    assert_eq!(response["native"]["completeness"], "COMPLETE");
    assert_eq!(response["preview"]["completeness"], "PARTIAL");
    assert!(
        response["warnings"][0]
            .as_str()
            .unwrap()
            .starts_with("NATIVE_COMPLETE_WITHOUT_PROOF")
    );
    let renamed = change(&response, "src/core.js");
    assert_eq!(renamed["kind"], "rename");
    assert_eq!(renamed["destination"], "src/lib/core.js");
    let caller = change(&response, "src/caller.js");
    assert!(
        caller["after"]
            .as_str()
            .unwrap()
            .contains("'./lib/core.js'")
    );
    assert!(repo.path().join("src/core.js").exists());
    assert!(!repo.path().join("src/lib/core.js").exists());
}

#[test]
fn a_move_into_a_missing_directory_fails_the_cortex_preview() {
    let repo = Repo::new(&[
        ("package.json", PACKAGE),
        (
            "src/core.js",
            "export function compute() {\n  return 1;\n}\n",
        ),
    ]);

    let error = repo
        .plan(
            "move_file",
            &json!({"from": "src/core.js", "to": "src/deep/core.js"}),
        )
        .unwrap_err();

    assert!(
        error.to_string().contains("failed Cortex preview"),
        "{error}"
    );
    assert!(!repo.path().join("src/deep").exists());
}

#[test]
fn a_native_duplicate_extraction_is_previewed() {
    let body = "fn ordered_sel(items: &[u32]) -> Vec<u32> {\n    let mut out = items.to_vec();\n    out.sort_unstable();\n    out\n}\n";
    let app =
        format!("{body}\npub fn show(items: &[u32]) -> Vec<u32> {{\n    ordered_sel(items)\n}}\n");
    let ui =
        format!("{body}\npub fn paint(items: &[u32]) -> Vec<u32> {{\n    ordered_sel(items)\n}}\n");
    let repo = Repo::new(&[
        ("Cargo.toml", CARGO),
        ("src/lib.rs", "pub mod app;\npub mod ui;\n"),
        ("src/app.rs", &app),
        ("src/ui.rs", &ui),
    ]);

    let response = repo.plan("extract_duplicate", &json!({})).unwrap();

    assert_eq!(response["status"], "PLANNED");
    let after = change(&response, "src/ui.rs")["after"].as_str().unwrap();
    assert!(after.contains("crate::app::ordered_sel"), "{after}");
    assert!(!after.contains("fn ordered_sel"), "{after}");
    assert_no_apply_authority(&response);
    assert_eq!(repo.read("src/ui.rs"), ui);
}

#[test]
fn an_envelope_rename_carries_the_edits_of_the_file_it_moves() {
    let hash = "0".repeat(64);
    let edit = json!({
        "startLine": 1, "startChar": 0, "endLine": 1, "endChar": 1,
        "before": "a", "after": "b", "provenance": "RESOLVED",
    });
    let envelope = json!({
        "schemaVersion": "weavatrix.edit-plan.v1",
        "operation": "move_file",
        "files": [
            {"path": "src/old.rs", "sha256": hash, "edits": [edit]},
            {"path": "src/user.rs", "sha256": hash, "edits": [edit]},
        ],
        "renames": [{"from": "src/old.rs", "to": "src/new.rs", "sha256": hash}],
        "deletes": [{"path": "src/gone.rs", "sha256": hash}],
    });

    let plan = refactor_plan_from_envelope(&envelope, Some("PARTIAL")).unwrap();

    assert_eq!(plan.operations.len(), 3);
    assert!(
        matches!(&plan.operations[0], RefactorOperation::Modify(file) if file.path == "src/user.rs")
    );
    assert!(matches!(
        &plan.operations[1],
        RefactorOperation::Rename(file) if file.to == "src/new.rs" && file.edits.len() == 1
    ));
    assert!(
        matches!(&plan.operations[2], RefactorOperation::Delete(file) if file.path == "src/gone.rs")
    );
    assert_eq!(plan.completeness.unwrap().as_str(), "PARTIAL");
    let broken = json!({
        "schemaVersion": "weavatrix.edit-plan.v1",
        "operation": "move_file",
        "files": [],
        "renames": [{"from": "src/old.rs", "sha256": hash}],
    });
    assert!(refactor_plan_from_envelope(&broken, None).is_err());
}
