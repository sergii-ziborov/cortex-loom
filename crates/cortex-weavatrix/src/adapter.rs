use std::collections::HashMap;
use std::env;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weavatrix_rust::{Weavatrix, operations};

use crate::{McpChild, McpCommand, McpError};

const MAX_EVIDENCE_CHARS: usize = 24_000;

#[derive(Debug, Clone)]
pub struct WeavatrixConfig {
    pub program: String,
    pub refactor_script: Option<PathBuf>,
    pub timeout: Duration,
    pub max_frame_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceBundle {
    pub repository: String,
    pub graph_status: String,
    pub module_map: String,
    pub symbol_context: Option<String>,
    pub verification: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefactorOperation {
    RenameSymbol,
    RenameRelatedSymbols,
    MoveFile,
    MoveSymbol,
    ChangeSignature,
    EditSymbol,
}

impl RefactorOperation {
    fn tool_name(self) -> &'static str {
        match self {
            Self::RenameSymbol => "rename_symbol",
            Self::RenameRelatedSymbols => "rename_related_symbols",
            Self::MoveFile => "move_file",
            Self::MoveSymbol => "move_symbol",
            Self::ChangeSignature => "change_signature",
            Self::EditSymbol => "edit_symbol",
        }
    }

    fn uses_preview_envelope(self) -> bool {
        matches!(self, Self::RenameSymbol | Self::RenameRelatedSymbols)
    }
}

#[derive(Debug)]
pub enum WeavatrixError {
    NotFound(String),
    Transport(McpError),
    InvalidArguments(String),
    Engine(String),
    LockPoisoned,
}

impl Display for WeavatrixError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(message) | Self::InvalidArguments(message) | Self::Engine(message) => {
                formatter.write_str(message)
            }
            Self::Transport(error) => Display::fmt(error, formatter),
            Self::LockPoisoned => formatter.write_str("Weavatrix session lock was poisoned"),
        }
    }
}

impl std::error::Error for WeavatrixError {}

impl From<McpError> for WeavatrixError {
    fn from(value: McpError) -> Self {
        Self::Transport(value)
    }
}

#[derive(Clone)]
pub struct WeavatrixAdapter {
    config: WeavatrixConfig,
    engines: Arc<Mutex<HashMap<PathBuf, Weavatrix>>>,
}

impl WeavatrixConfig {
    pub fn discover() -> Result<Self, WeavatrixError> {
        let program =
            env::var("CORTEX_LOOM_WEAVATRIX_COMMAND").unwrap_or_else(|_| "node".to_owned());
        let refactor_script = if let Ok(script) = env::var("CORTEX_LOOM_REFACTOR_SCRIPT") {
            Some(PathBuf::from(script))
        } else {
            discover_refactor_script()
        };
        Ok(Self {
            program,
            refactor_script,
            timeout: Duration::from_secs(90),
            max_frame_bytes: 16 * 1024 * 1024,
        })
    }
}

impl WeavatrixAdapter {
    #[must_use]
    pub fn new(config: WeavatrixConfig) -> Self {
        Self {
            config,
            engines: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn prepare_context(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        let root = repository.canonicalize().map_err(|error| {
            WeavatrixError::Engine(format!("cannot open {}: {error}", repository.display()))
        })?;
        let mut sessions = self
            .engines
            .lock()
            .map_err(|_| WeavatrixError::LockPoisoned)?;
        if !sessions.contains_key(&root) {
            let engine = Weavatrix::open(&root).map_err(|error| {
                WeavatrixError::Engine(format!("Weavatrix graph build failed: {error}"))
            })?;
            sessions.insert(root.clone(), engine);
        }
        let engine = sessions.get_mut(&root).ok_or_else(|| {
            WeavatrixError::Engine("native Weavatrix session was not retained".to_owned())
        })?;
        let refreshed = engine.refresh_if_stale().map_err(|error| {
            WeavatrixError::Engine(format!("Weavatrix refresh failed: {error}"))
        })?;
        let graph_status = native_call(engine, "graph_stats", json!({}))?;
        let module_map = native_call(
            engine,
            "module_map",
            json!({"top_n": 24, "include_non_product": false}),
        )?;
        let symbol_context = symbol
            .map(|label| {
                native_call(
                    engine,
                    "context_bundle",
                    json!({
                        "label": label,
                        "max_related": 30,
                        "max_references": 30,
                        "max_source_files": 12
                    }),
                )
            })
            .transpose()?;
        let verification = native_call(
            engine,
            "verified_change",
            json!({
                "task": task,
                "phase": "plan",
                "duplicate_ratchet": true,
                "run_tests": false
            }),
        )?;
        Ok(EvidenceBundle {
            repository: repository.to_string_lossy().into_owned(),
            graph_status: extract_text(&graph_status),
            module_map: extract_text(&module_map),
            symbol_context: symbol_context.as_ref().map(extract_text),
            verification: extract_text(&verification),
            warnings: refreshed
                .then(|| "native Weavatrix graph refreshed from changed source evidence".to_owned())
                .into_iter()
                .collect(),
        })
    }

    pub fn preview_refactor(
        &self,
        repository: &Path,
        operation: RefactorOperation,
        arguments: Value,
    ) -> Result<String, WeavatrixError> {
        let mut arguments = arguments.as_object().cloned().ok_or_else(|| {
            WeavatrixError::InvalidArguments("refactor arguments must be a JSON object".to_owned())
        })?;
        arguments.remove("confirm_token");
        if operation.uses_preview_envelope() {
            arguments.insert("mode".to_owned(), Value::String("preview".to_owned()));
        }
        arguments.insert("output_format".to_owned(), Value::String("json".to_owned()));

        let mut client = self.refactor_client(repository)?;
        client.call_tool(
            "open_repo",
            json!({
                "path": repository.to_string_lossy(),
                "build": true,
                "mode": "full",
                "precision": "lsp",
                "output_format": "json"
            }),
        )?;
        let result = client.call_tool(operation.tool_name(), Value::Object(arguments))?;
        Ok(extract_text(&result))
    }

    fn refactor_client(&self, repository: &Path) -> Result<McpChild, WeavatrixError> {
        let script = self.config.refactor_script.as_ref().ok_or_else(|| {
            WeavatrixError::NotFound(
                "Weavatrix Refactor was not found; set CORTEX_LOOM_REFACTOR_SCRIPT".to_owned(),
            )
        })?;
        let mut client = McpChild::spawn(&McpCommand {
            program: self.config.program.clone(),
            args: vec![
                script.to_string_lossy().into_owned(),
                repository.to_string_lossy().into_owned(),
            ],
            cwd: repository.parent().map(Path::to_path_buf),
            timeout: self.config.timeout,
            max_frame_bytes: self.config.max_frame_bytes,
        })?;
        client.initialize()?;
        Ok(client)
    }
}

fn discover_refactor_script() -> Option<PathBuf> {
    let current = env::current_dir().ok()?;
    let mut roots = vec![current.clone()];
    roots.extend(current.ancestors().map(Path::to_path_buf));
    roots.into_iter().find_map(|root| {
        let direct = root.join("weavatrix-refactor/bin/weavatrix-refactor-mcp.mjs");
        let sibling = root.join("../weavatrix-refactor/bin/weavatrix-refactor-mcp.mjs");
        [direct, sibling]
            .into_iter()
            .find(|candidate| candidate.is_file())
    })
}

fn native_call(
    engine: &mut Weavatrix,
    name: &str,
    arguments: Value,
) -> Result<Value, WeavatrixError> {
    operations::call(engine, name, arguments).map_err(WeavatrixError::Engine)
}

fn extract_text(value: &Value) -> String {
    let structured = value
        .get("structuredContent")
        .and_then(|content| content.get("result"))
        .and_then(|result| result.get("text"))
        .and_then(Value::as_str);
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str);
    let text = structured
        .or(content)
        .map_or_else(|| value.to_string(), str::to_owned);
    truncate_chars(text, MAX_EVIDENCE_CHARS)
}

fn truncate_chars(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut result: String = value.chars().take(max_chars).collect();
    result.push_str("\n[truncated by Cortex Loom]");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refactor_allowlist_never_maps_to_apply() {
        for operation in [
            RefactorOperation::RenameSymbol,
            RefactorOperation::RenameRelatedSymbols,
            RefactorOperation::MoveFile,
            RefactorOperation::MoveSymbol,
            RefactorOperation::ChangeSignature,
            RefactorOperation::EditSymbol,
        ] {
            assert_ne!(operation.tool_name(), "apply_edit_plan");
        }
    }

    #[test]
    fn extracts_structured_text_before_fallback_content() {
        let value = json!({
            "content": [{"type": "text", "text": "fallback"}],
            "structuredContent": {"result": {"text": "structured"}}
        });
        assert_eq!(extract_text(&value), "structured");
    }
}
