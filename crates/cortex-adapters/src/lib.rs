//! Vendor-specific packaging of the canonical, vendor-neutral graph.
//!
//! An adapter renders the wiring one coding agent needs — skill instructions
//! plus MCP registration — from one canonical graph. Export is preview-only:
//! it returns file paths and contents and never writes to disk, so applying
//! the wiring stays an explicit human or upstream-agent action. The canonical
//! graph is the source of truth; every instruction file is derived from the
//! same `cortex-skills` Markdown view.

use std::fmt::{Display, Formatter};

use cortex_domain::GraphDocument;
use cortex_skills::export_skill_markdown;
use serde::{Deserialize, Serialize};

pub const MCP_SERVER_NAME: &str = "cortex-loom";

/// Shared usage contract embedded into every vendor instruction file.
const USAGE_NOTE: &str = "Use `route_work` before acting on a task, passing `runId` when you execute a run node. Fetch bounded, citable repository evidence with `weavatrix_context_compile` (default `maxTokens: 4000` — the measured sweet spot; large fragments arrive as split `WX-*-n` sub-citations) and keep every `TASK`/`WX-*` citation ID in derived output. When you finish a task, self-report your consumption with `usage_report { runId, agent, inputTokens, outputTokens }` so savings can be credited against real upstream cost. Local-model output is advisory only. High-risk, ambiguous, unverified, or mutating work stays with the upstream agent or a human gate, and Weavatrix Refactor remains preview-only.";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    Copilot,
}

impl AgentKind {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude_code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "copilot" => Some(Self::Copilot),
            _ => None,
        }
    }
}

/// How the agent should launch the Cortex Loom MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpLaunch {
    pub command: String,
    pub args: Vec<String>,
}

impl Default for McpLaunch {
    /// Development launch from this workspace; installed deployments override
    /// it with the packaged binary path.
    fn default() -> Self {
        Self {
            command: "cargo".to_owned(),
            args: vec!["run".to_owned(), "-p".to_owned(), "cortex-mcp".to_owned()],
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterFile {
    /// Repository-relative path the content is intended for.
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterBundle {
    pub agent: AgentKind,
    pub graph_id: String,
    pub files: Vec<AdapterFile>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    Skill(String),
}

impl Display for AdapterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skill(message) => write!(formatter, "skill export failed: {message}"),
        }
    }
}

impl std::error::Error for AdapterError {}

/// Render the wiring files for one agent from one canonical graph.
///
/// Graphs compiled by `cortex-skills` reuse the round-trip `SKILL.md` view;
/// every other canonical graph gets a deterministic generic view so the
/// adapter works for any graph without inventing skill provenance.
pub fn export_adapter(
    graph: &GraphDocument,
    agent: AgentKind,
    launch: &McpLaunch,
) -> Result<AdapterBundle, AdapterError> {
    let skill_markdown = match export_skill_markdown(graph) {
        Ok(markdown) => markdown,
        Err(error) => {
            if graph.metadata.get("compiler").map(String::as_str) == Some("cortex-skills") {
                return Err(AdapterError::Skill(error.to_string()));
            }
            graph_instructions(graph)
        }
    };
    let bundle = match agent {
        AgentKind::ClaudeCode => claude_code(graph, &skill_markdown, launch),
        AgentKind::Codex => codex(graph, &skill_markdown, launch),
        AgentKind::Copilot => copilot(graph, &skill_markdown, launch),
    };
    Ok(bundle)
}

/// Deterministic generic instructions for graphs without skill provenance.
fn graph_instructions(graph: &GraphDocument) -> String {
    use std::fmt::Write as _;

    let description = graph
        .metadata
        .get("description")
        .map(String::as_str)
        .unwrap_or_default();
    let mut output = String::new();
    let _ = writeln!(output, "---\nname: {}", graph.name);
    let _ = writeln!(output, "description: {description}\n---\n");
    let _ = writeln!(output, "# {}\n", graph.name);
    let _ = writeln!(
        output,
        "Canonical Cortex Loom graph `{}` (revision {}, {} nodes, {} edges). The typed graph is the source of truth; this file is a generated view.\n",
        graph.id,
        graph.revision,
        graph.nodes.len(),
        graph.edges.len()
    );
    let _ = writeln!(output, "## Nodes\n");
    for node in &graph.nodes {
        let _ = write!(output, "- {:?} `{}`: {}", node.kind, node.id, node.label);
        if node.description.is_empty() {
            output.push('\n');
        } else {
            let _ = writeln!(output, " — {}", node.description);
        }
    }
    output
}

fn claude_code(graph: &GraphDocument, skill: &str, launch: &McpLaunch) -> AdapterBundle {
    let mcp = serde_json::json!({
        "mcpServers": {
            "cortex-loom": {
                "type": "stdio",
                "command": launch.command,
                "args": launch.args,
                "tools": ["*"]
            }
        }
    });
    AdapterBundle {
        agent: AgentKind::ClaudeCode,
        graph_id: graph.id.clone(),
        files: vec![
            AdapterFile {
                path: format!(".claude/skills/{}/SKILL.md", graph.id),
                content: with_usage_note(skill),
            },
            AdapterFile {
                path: ".mcp.json".to_owned(),
                content: pretty_json(&mcp),
            },
        ],
        notes: vec![
            "Preview-only: nothing was written; place the files yourself.".to_owned(),
            "Merge the mcpServers entry if .mcp.json already exists.".to_owned(),
        ],
    }
}

fn codex(graph: &GraphDocument, skill: &str, launch: &McpLaunch) -> AdapterBundle {
    let args = launch
        .args
        .iter()
        .map(|argument| toml_string(argument))
        .collect::<Vec<_>>()
        .join(", ");
    let config = format!(
        "[mcp_servers.{MCP_SERVER_NAME}]\ncommand = {}\nargs = [{args}]\n",
        toml_string(&launch.command)
    );
    AdapterBundle {
        agent: AgentKind::Codex,
        graph_id: graph.id.clone(),
        files: vec![
            AdapterFile {
                path: format!("docs/agents/codex-{}.md", graph.id),
                content: format!(
                    "## Cortex Loom workflow: {}\n\n{USAGE_NOTE}\n\n{skill}",
                    graph.name
                ),
            },
            AdapterFile {
                path: "codex-config-snippet.toml".to_owned(),
                content: config,
            },
        ],
        notes: vec![
            "Preview-only: nothing was written; place the files yourself.".to_owned(),
            "Append the workflow section to AGENTS.md or reference it from there.".to_owned(),
            "Add the snippet to ~/.codex/config.toml (shared MCP host configuration).".to_owned(),
        ],
    }
}

fn copilot(graph: &GraphDocument, skill: &str, launch: &McpLaunch) -> AdapterBundle {
    let mcp = serde_json::json!({
        "servers": {
            "cortex-loom": {
                "type": "stdio",
                "command": launch.command,
                "args": launch.args
            }
        }
    });
    AdapterBundle {
        agent: AgentKind::Copilot,
        graph_id: graph.id.clone(),
        files: vec![
            AdapterFile {
                path: format!(
                    ".github/instructions/cortex-loom-{}.instructions.md",
                    graph.id
                ),
                content: format!("---\napplyTo: \"**\"\n---\n\n{USAGE_NOTE}\n\n{skill}"),
            },
            AdapterFile {
                path: ".vscode/mcp.json".to_owned(),
                content: pretty_json(&mcp),
            },
        ],
        notes: vec![
            "Preview-only: nothing was written; place the files yourself.".to_owned(),
            "Merge the servers entry if .vscode/mcp.json already exists.".to_owned(),
        ],
    }
}

fn with_usage_note(skill: &str) -> String {
    format!("{skill}\n\n## Cortex Loom usage contract\n\n{USAGE_NOTE}\n")
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Escape one TOML basic string, including Windows path separators.
fn toml_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests;
