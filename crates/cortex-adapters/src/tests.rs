use cortex_domain::default_control_plane;

use super::*;

fn launch_with_windows_path() -> McpLaunch {
    McpLaunch {
        command: "C:\\Tools\\cortex\\cortex-mcp.exe".to_owned(),
        args: vec!["--profile=\"code\"".to_owned()],
    }
}

#[test]
fn every_agent_bundle_derives_from_the_canonical_graph() {
    let graph = default_control_plane();
    for agent in [AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::Copilot] {
        let bundle = export_adapter(&graph, agent, &McpLaunch::default()).unwrap();
        assert_eq!(bundle.agent, agent);
        assert_eq!(bundle.graph_id, graph.id);
        assert_eq!(bundle.files.len(), 2);
        let instructions = &bundle.files[0].content;
        assert!(
            instructions.contains(&graph.name),
            "instructions embed the canonical skill view"
        );
        assert!(
            instructions.contains("route_work"),
            "usage contract is present"
        );
        assert!(
            bundle
                .notes
                .iter()
                .any(|note| note.contains("Preview-only")),
            "nothing is ever written by the exporter"
        );
    }
}

#[test]
fn claude_bundle_registers_the_mcp_server_without_touching_refactor_boundaries() {
    let graph = default_control_plane();
    let bundle = export_adapter(&graph, AgentKind::ClaudeCode, &McpLaunch::default()).unwrap();
    assert!(bundle.files[0].path.ends_with("SKILL.md"));
    assert_eq!(bundle.files[1].path, ".mcp.json");
    let parsed: serde_json::Value = serde_json::from_str(&bundle.files[1].content).unwrap();
    assert_eq!(parsed["mcpServers"]["cortex-loom"]["type"], "stdio");
    assert_eq!(parsed["mcpServers"]["cortex-loom"]["command"], "cargo");
}

#[test]
fn codex_toml_snippet_escapes_windows_paths() {
    let graph = default_control_plane();
    let bundle = export_adapter(&graph, AgentKind::Codex, &launch_with_windows_path()).unwrap();
    let snippet = &bundle.files[1].content;
    assert!(snippet.contains("[mcp_servers.cortex-loom]"));
    assert!(
        snippet.contains(r#"command = "C:\\Tools\\cortex\\cortex-mcp.exe""#),
        "backslashes are escaped: {snippet}"
    );
    assert!(
        snippet.contains(r#"args = ["--profile=\"code\""]"#),
        "quotes are escaped: {snippet}"
    );
}

#[test]
fn copilot_bundle_targets_vscode_and_instruction_files() {
    let graph = default_control_plane();
    let bundle = export_adapter(&graph, AgentKind::Copilot, &McpLaunch::default()).unwrap();
    assert!(bundle.files[0].path.starts_with(".github/instructions/"));
    assert!(bundle.files[0].content.starts_with("---\napplyTo:"));
    assert_eq!(bundle.files[1].path, ".vscode/mcp.json");
    let parsed: serde_json::Value = serde_json::from_str(&bundle.files[1].content).unwrap();
    assert_eq!(parsed["servers"]["cortex-loom"]["type"], "stdio");
}

#[test]
fn agent_kind_parsing_is_exact() {
    assert_eq!(AgentKind::parse("claude_code"), Some(AgentKind::ClaudeCode));
    assert_eq!(AgentKind::parse("codex"), Some(AgentKind::Codex));
    assert_eq!(AgentKind::parse("copilot"), Some(AgentKind::Copilot));
    assert_eq!(AgentKind::parse("cursor"), None);
}
