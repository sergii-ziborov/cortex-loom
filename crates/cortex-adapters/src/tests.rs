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
            instructions.contains("cortex_prepare"),
            "usage contract is present"
        );
        assert!(
            instructions.contains("budgetClass"),
            "budget class remains an optional pin"
        );
        assert!(
            !instructions.contains("usage_report"),
            "usage_report is not a prompt-visible workflow tool"
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
    assert_eq!(
        parsed["mcpServers"]["cortex-loom"]["args"],
        serde_json::json!(["run", "-p", "cortex-mcp", "--", "--profile", "agent"])
    );
    assert_eq!(
        parsed["mcpServers"]["cortex-loom"]["tools"],
        serde_json::json!(["cortex_prepare", "cortex_expand"])
    );
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

/// The library shipped with the crate, compiled.
fn bundled_graphs() -> Vec<cortex_domain::GraphDocument> {
    cortex_skills::bundled_skills()
        .iter()
        .map(|skill| cortex_skills::import_skill_markdown(skill.source, skill.markdown).unwrap())
        .collect()
}

/// The always-applied file must never grow with the library.
///
/// Copilot applies its instruction file to every prompt and Codex has the
/// same problem through AGENTS.md. Inlining one workflow there was
/// affordable; inlining a library is a per-turn tax on work that has nothing
/// to do with any of them.
#[test]
fn an_always_applied_file_carries_the_catalogue_and_never_a_workflow_body() {
    let graphs = bundled_graphs();
    let bodies: usize = graphs
        .iter()
        .map(|graph| export_skill_markdown(graph).unwrap().chars().count())
        .sum();

    for agent in [AgentKind::Codex, AgentKind::Copilot] {
        let bundle = export_library_adapter(&graphs, agent, &McpLaunch::default()).unwrap();
        let always_applied = &bundle.files[0].content;
        assert!(
            always_applied.chars().count() * 3 < bodies,
            "{agent:?}: always-applied file is {} chars against {bodies} of workflows",
            always_applied.chars().count()
        );
        for graph in &graphs {
            assert!(
                always_applied.contains(&graph.id),
                "{agent:?}: {} is not discoverable",
                graph.id
            );
        }
        // A step line from a workflow body must not have leaked in.
        assert!(
            !always_applied.contains("Run it and watch it fail for the stated reason"),
            "{agent:?}: a workflow body leaked into the always-applied file"
        );
        assert!(
            always_applied.contains("cortex_prepare"),
            "{agent:?}: no way to fetch evidence"
        );
    }
}

/// Claude Code defers by itself, so it gets the real files.
#[test]
fn claude_code_gets_one_lazily_loaded_file_per_workflow() {
    let graphs = bundled_graphs();
    let bundle =
        export_library_adapter(&graphs, AgentKind::ClaudeCode, &McpLaunch::default()).unwrap();
    assert_eq!(
        bundle.files.len(),
        graphs.len() + 1,
        "one per skill plus .mcp.json"
    );
    for graph in &graphs {
        assert!(
            bundle
                .files
                .iter()
                .any(|file| file.path == format!(".claude/skills/{}/SKILL.md", graph.id)),
            "{} has no skill file",
            graph.id
        );
    }
}
