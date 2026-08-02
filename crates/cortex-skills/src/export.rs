use cortex_domain::{GraphDocument, GraphNode};

use crate::SkillError;

/// Export a compiled skill graph as stable, readable Markdown.
pub fn export_skill_markdown(graph: &GraphDocument) -> Result<String, SkillError> {
    graph.validate()?;
    if graph.metadata.get("compiler").map(String::as_str) != Some("cortex-skills") {
        return Err(SkillError::UnsupportedGraph(
            "graph was not produced by cortex-skills".to_owned(),
        ));
    }

    let description = graph.metadata.get("description").map_or("", String::as_str);
    let mut output = String::from("---\n");
    output.push_str(&format!("name: {}\n", yaml_string(&graph.name)));
    output.push_str(&format!("description: {}\n", yaml_string(description)));
    let mut extra: Vec<_> = graph
        .metadata
        .iter()
        .filter_map(|(key, value)| key.strip_prefix("frontmatter.").map(|key| (key, value)))
        .collect();
    extra.sort_by_key(|(key, _)| *key);
    for (key, value) in extra {
        output.push_str(&format!("{key}: {}\n", yaml_string(value)));
    }
    output.push_str("---\n\n");
    output.push_str(&format!("# {}\n", graph.name));

    let mut nodes: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| role(node) != Some("skill"))
        .collect();
    nodes.sort_by_key(|node| order(node));
    let mut previous_was_step = false;
    for node in nodes {
        match role(node) {
            Some("section") => {
                ensure_blank_line(&mut output);
                let level = node
                    .config
                    .get("headingLevel")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(2)
                    .clamp(1, 6);
                output.push_str(&"#".repeat(level as usize));
                output.push(' ');
                output.push_str(&node.label);
                output.push('\n');
                previous_was_step = false;
            }
            Some("guidance") => {
                ensure_blank_line(&mut output);
                let text = if node.description.is_empty() {
                    &node.label
                } else {
                    &node.description
                };
                output.push_str(text.trim());
                output.push('\n');
                previous_was_step = false;
            }
            Some("workflow_step") => {
                if !previous_was_step {
                    ensure_blank_line(&mut output);
                }
                let indent = node
                    .config
                    .get("indent")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                output.push_str(&" ".repeat(indent as usize));
                output.push_str(&marker(node));
                output.push_str(&node.label);
                output.push('\n');
                previous_was_step = true;
            }
            _ => {}
        }
    }
    Ok(format!("{}\n", output.trim_end()))
}

fn role(node: &GraphNode) -> Option<&str> {
    node.config.get("role").and_then(serde_json::Value::as_str)
}

fn order(node: &GraphNode) -> u64 {
    node.config
        .get("order")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX)
}

fn marker(node: &GraphNode) -> String {
    match node
        .config
        .get("marker")
        .and_then(serde_json::Value::as_str)
    {
        Some("numbered") => format!(
            "{}. ",
            node.config
                .get("listNumber")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1)
        ),
        Some("checklist") => {
            if node
                .config
                .get("checked")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "- [x] ".to_owned()
            } else {
                "- [ ] ".to_owned()
            }
        }
        _ => "- ".to_owned(),
    }
}

fn ensure_blank_line(output: &mut String) {
    if !output.ends_with("\n\n") {
        if !output.ends_with('\n') {
            output.push('\n');
        }
        output.push('\n');
    }
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}
