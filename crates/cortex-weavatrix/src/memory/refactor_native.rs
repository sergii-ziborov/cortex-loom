//! Native refactor plans from `weavatrix-rust-refactor`, previewed by Cortex.
//!
//! The planner proves rename, signature, move, and extract sites from the same
//! `weavatrix-rust` graph Cortex reads and returns a `weavatrix.edit-plan.v1`
//! envelope. Cortex converts that envelope into `weavatrix.refactor-plan.v1`
//! and sends it through the same validation and in-memory preview as a plan
//! an upstream agent wrote. Nothing here applies a plan. The planner session
//! is read-only, `mode` and `confirm_token` are refused, and the planner's
//! confirm token is dropped from the answer.
//!
//! The planner takes the shared reader lock that `weavatrix-refactor` writers
//! respect. That lock creates an empty `.weavatrix/refactor-write.lock` in the
//! repository when it is missing. No source file is written.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value, json};
use weavatrix_refactor_plan::{
    Completeness, DeleteFile, RefactorOperation, RefactorPlan, RenameFile,
};
use weavatrix_rust::RepositoryState;
use weavatrix_rust_refactor::operations::RefactorSession;

use crate::{WeavatrixError, preview_refactor_plan};

/// Plan producers Cortex takes from the native planner.
///
/// `apply_edit_plan` and `rollback_last_apply` write, so they are absent.
/// The other producers stay with the upstream agent's own plan.
pub const NATIVE_PLAN_OPERATIONS: [&str; 8] = [
    "rename_symbol",
    "rename_related_symbols",
    "change_signature",
    "move_file",
    "move_symbol",
    "extract_duplicate",
    "extract_window",
    "share_method_body",
];

/// Arguments that would hand Cortex apply authority.
const APPLY_ARGUMENTS: [&str; 2] = ["mode", "confirm_token"];

/// Planner answer fields Cortex does not return. `plan` is replaced by the
/// converted plan; the rest describe the planner's own apply path.
const DROPPED_ANSWER_FIELDS: [&str; 6] = [
    "plan",
    "confirmToken",
    "expiresAt",
    "next",
    "previewed",
    "previewBlocked",
];

/// A native `COMPLETE` arrives without the typed `completenessProof` the
/// plan validator requires, and Cortex cannot check the claim itself.
const UNPROVEN_COMPLETE: &str = "NATIVE_COMPLETE_WITHOUT_PROOF: the planner reported COMPLETE \
without a completenessProof; Cortex records the plan as PARTIAL";

const PREVIEW_ONLY: &str = "Cortex never applies a plan. Review the preview, then make the \
edits with your own tools or hand the plan to a host that owns apply.";

/// Validate the operation and its arguments before the planner sees them.
///
/// # Errors
///
/// Returns [`WeavatrixError::InvalidArguments`] for an operation outside
/// [`NATIVE_PLAN_OPERATIONS`], arguments that are not an object, or any
/// `mode` / `confirm_token` argument.
pub fn planning_arguments(operation: &str, arguments: &Value) -> Result<Value, WeavatrixError> {
    if !NATIVE_PLAN_OPERATIONS.contains(&operation) {
        return Err(WeavatrixError::InvalidArguments(format!(
            "{operation} is not a native plan operation; use one of: {}",
            NATIVE_PLAN_OPERATIONS.join(", ")
        )));
    }
    let Some(object) = arguments.as_object() else {
        return Err(WeavatrixError::InvalidArguments(
            "refactor arguments must be an object".to_owned(),
        ));
    };
    if let Some(key) = APPLY_ARGUMENTS
        .iter()
        .find(|key| object.contains_key(**key))
    {
        return Err(WeavatrixError::InvalidArguments(format!(
            "{key} is not accepted: Cortex plans and previews refactors and never applies them"
        )));
    }
    let mut planned = object.clone();
    planned.remove("output_format");
    Ok(Value::Object(planned))
}

/// Ask the native planner for one plan through a read-only session.
///
/// # Errors
///
/// Returns [`WeavatrixError::InvalidArguments`] when the planner does not
/// declare `operation`. A planner refusal is a status in the answer.
pub fn native_plan(
    state: &RepositoryState,
    operation: &str,
    arguments: &Value,
) -> Result<Value, WeavatrixError> {
    RefactorSession::read_only()
        .call(state, operation, arguments)
        .map_err(WeavatrixError::InvalidArguments)
}

/// Turn a planner answer into Cortex's preview reply.
///
/// An answer with a plan is converted and previewed against `root`. An
/// answer without one (a refusal, `NO_EDITS`, a cycle verdict) keeps its
/// status and reason and has no preview. A native `COMPLETE` is recorded as
/// `PARTIAL`: the planner sends no typed proof for it, and Cortex does not
/// invent one.
///
/// # Errors
///
/// Returns [`WeavatrixError`] when the native plan fails Cortex's own
/// validation or preview: unsafe paths, stale hashes, or a destination that
/// already exists or has no parent directory.
pub fn native_plan_response(
    root: &Path,
    operation: &str,
    answer: &Value,
) -> Result<Value, WeavatrixError> {
    let status = answer
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("NO_STATUS")
        .to_owned();
    let claimed = answer.get("completeness").and_then(Value::as_str);
    let unproven = claimed == Some("COMPLETE");
    let completeness = if unproven { Some("PARTIAL") } else { claimed };
    let mut response = json!({
        "mode": "preview",
        "planner": {
            "crate": "weavatrix-rust-refactor",
            "version": weavatrix_rust_refactor::VERSION,
            "contract": weavatrix_rust_refactor::CONTRACT_VERSION,
        },
        "operation": operation,
        "status": status.as_str(),
        "native": sanitized(answer),
        "next": PREVIEW_ONLY,
    });
    if unproven {
        response["warnings"] = json!([UNPROVEN_COMPLETE]);
    }
    let envelope = answer.get("plan").filter(|plan| !plan.is_null());
    if let Some(envelope) = envelope {
        let plan = refactor_plan_from_envelope(envelope, completeness)?;
        let encoded = serde_json::to_vec(&plan).map_err(|error| {
            WeavatrixError::Engine(format!("cannot encode native refactor plan: {error}"))
        })?;
        let preview = preview_refactor_plan(root, &encoded).map_err(|error| {
            WeavatrixError::InvalidArguments(format!(
                "native {operation} plan failed Cortex preview ({status}): {error}"
            ))
        })?;
        response["plan"] = serde_json::to_value(&plan).map_err(|error| {
            WeavatrixError::Engine(format!("cannot encode native refactor plan: {error}"))
        })?;
        response["preview"] = serde_json::to_value(&preview).map_err(|error| {
            WeavatrixError::Engine(format!("cannot encode refactor preview: {error}"))
        })?;
    }
    Ok(response)
}

/// Convert a `weavatrix.edit-plan.v1` envelope into `weavatrix.refactor-plan.v1`.
///
/// File entries become `modify` operations. A `renames` entry becomes a
/// `rename` that carries the edits of the file it moves, and a `deletes`
/// entry becomes a `delete`. The envelope is read by the planner's own
/// reader, so a missing hash or an unprovable provenance is refused here too.
///
/// # Errors
///
/// Returns [`WeavatrixError::InvalidArguments`] for a malformed envelope or a
/// rename / delete entry that is missing a path or hash.
pub fn refactor_plan_from_envelope(
    envelope: &Value,
    completeness: Option<&str>,
) -> Result<RefactorPlan, WeavatrixError> {
    let edit_plan =
        weavatrix_rust_refactor::envelope::read_envelope(envelope).map_err(|error| {
            WeavatrixError::InvalidArguments(format!(
                "native plan is not a valid edit plan ({}): {}",
                error.code, error.reason
            ))
        })?;
    let mut edits = BTreeMap::new();
    let mut order = Vec::with_capacity(edit_plan.files.len());
    for file in edit_plan.files {
        order.push(file.path.clone());
        edits.insert(file.path.clone(), file);
    }
    let mut operations = Vec::new();
    for entry in entries(&edit_plan.extensions, "renames") {
        let from = field(entry, "from", "rename")?;
        let to = field(entry, "to", "rename")?;
        let sha256 = field(entry, "sha256", "rename")?;
        let moved = edits
            .remove(from)
            .map(|file| file.edits)
            .unwrap_or_default();
        operations.push(RefactorOperation::Rename(
            RenameFile::new(from, to, sha256).with_edits(moved),
        ));
    }
    for entry in entries(&edit_plan.extensions, "deletes") {
        let path = field(entry, "path", "delete")?;
        let sha256 = field(entry, "sha256", "delete")?;
        operations.push(RefactorOperation::Delete(DeleteFile::new(path, sha256)));
    }
    let modified = order
        .iter()
        .filter_map(|path| edits.remove(path))
        .map(RefactorOperation::Modify);
    let operations = modified.chain(operations).collect::<Vec<_>>();
    let mut plan = RefactorPlan::new(edit_plan.operation, operations);
    plan.completeness = completeness.map(Completeness::new);
    Ok(plan)
}

fn entries<'a>(extensions: &'a BTreeMap<String, Value>, key: &str) -> &'a [Value] {
    extensions
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn field<'a>(entry: &'a Value, key: &str, kind: &str) -> Result<&'a str, WeavatrixError> {
    entry.get(key).and_then(Value::as_str).ok_or_else(|| {
        WeavatrixError::InvalidArguments(format!("native {kind} entry has no {key}"))
    })
}

fn sanitized(answer: &Value) -> Value {
    let mut kept = Map::new();
    if let Some(object) = answer.as_object() {
        for (key, value) in object {
            if !DROPPED_ANSWER_FIELDS.contains(&key.as_str()) {
                kept.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(kept)
}

#[cfg(test)]
#[path = "refactor_native_tests.rs"]
mod tests;
