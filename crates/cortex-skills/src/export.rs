use std::collections::HashMap;
use std::fmt::Write;

use cortex_domain::{GraphDocument, GraphNode, NodeKind};

use crate::SkillError;

/// Export a compiled skill graph as stable, readable Markdown.
///
/// Exporting twice is byte-identical, so import → export → import preserves
/// workflow semantics.
///
/// # Errors
///
/// Returns [`SkillError::InvalidGraph`] when the graph fails validation, and
/// [`SkillError::UnsupportedGraph`] unless `metadata["compiler"]` is
/// `"cortex-skills"`. Export reads the node roles and ordering that
/// [`crate::import_skill_markdown`] writes, so a graph built by other means
/// has no Markdown shape to recover; set that metadata key deliberately if
/// you produce compatible graphs yourself.
pub fn export_skill_markdown(graph: &GraphDocument) -> Result<String, SkillError> {
    graph.validate()?;
    if graph.metadata.get("compiler").map(String::as_str) != Some("cortex-skills") {
        return Err(SkillError::UnsupportedGraph(
            "graph was not produced by cortex-skills".to_owned(),
        ));
    }

    let mut output = String::new();
    write_header(graph, &mut output);
    let mut nodes: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| role(node) != Some("skill"))
        .collect();
    nodes.sort_by_key(|node| order(node));
    let dependencies = dependency_map(graph, &nodes);
    write_nodes(&nodes, &dependencies, &mut output);
    output.truncate(output.trim_end().len());
    output.push('\n');
    Ok(output)
}

// Writing into a `String` through `fmt::Write` is infallible, so the results
// below are discarded rather than unwrapped: this crate's production paths
// contain no panic.
fn write_header(graph: &GraphDocument, output: &mut String) {
    let description = graph.metadata.get("description").map_or("", String::as_str);
    let _ = writeln!(output, "---");
    let _ = writeln!(output, "name: {}", yaml_string(&graph.name));
    let _ = writeln!(output, "description: {}", yaml_string(description));
    let mut extra: Vec<_> = graph
        .metadata
        .iter()
        .filter_map(|(key, value)| key.strip_prefix("frontmatter.").map(|key| (key, value)))
        .collect();
    extra.sort_by_key(|(key, _)| *key);
    for (key, value) in extra {
        let _ = writeln!(output, "{key}: {}", yaml_string(value));
    }
    let _ = writeln!(output, "---\n\n# {}", crate::heading_text(&graph.name));
}

fn dependency_map<'a>(
    graph: &'a GraphDocument,
    nodes: &[&'a GraphNode],
) -> HashMap<&'a str, Vec<usize>> {
    let step_numbers: HashMap<_, _> = nodes
        .iter()
        .filter(|node| role(node) == Some("workflow_step"))
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index + 1))
        .collect();
    let mut dependencies: HashMap<&str, Vec<usize>> = HashMap::new();
    for edge in &graph.edges {
        if edge.label == "explicit dependency"
            && let (Some(source), Some(_)) = (
                step_numbers.get(edge.from.as_str()),
                step_numbers.get(edge.to.as_str()),
            )
        {
            dependencies
                .entry(edge.to.as_str())
                .or_default()
                .push(*source);
        }
    }
    for values in dependencies.values_mut() {
        values.sort_unstable();
        values.dedup();
    }
    dependencies
}

fn write_nodes(
    nodes: &[&GraphNode],
    dependencies: &HashMap<&str, Vec<usize>>,
    output: &mut String,
) {
    let mut previous_was_step = false;
    for node in nodes {
        match role(node) {
            Some("section") => {
                ensure_blank_line(output);
                output.push_str(&"#".repeat(heading_level(node)));
                output.push(' ');
                output.push_str(&node.label);
                output.push('\n');
                previous_was_step = false;
            }
            Some("guidance") => {
                ensure_blank_line(output);
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
                    ensure_blank_line(output);
                }
                output.push_str(&" ".repeat(indent(node)));
                output.push_str(&marker(node));
                output.push_str(&step_label(
                    node,
                    dependencies.get(node.id.as_str()).map(Vec::as_slice),
                ));
                output.push('\n');
                previous_was_step = true;
            }
            _ => {}
        }
    }
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

fn heading_level(node: &GraphNode) -> usize {
    node.config
        .get("headingLevel")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(2)
        .clamp(1, 6)
}

fn indent(node: &GraphNode) -> usize {
    node.config
        .get("indent")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
        .min(256)
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

/// One step line: the label, then the annotations the graph carries.
///
/// Annotations are written from the graph rather than copied from the label,
/// so editing a node's kind or its dependency edges in the editor changes the
/// exported Markdown — the graph is canonical and the Markdown is its view.
/// The order is fixed (`[kind: …]` before `[depends: …]`) so exporting twice
/// is byte-identical.
fn step_label(node: &GraphNode, dependencies: Option<&[usize]>) -> String {
    let mut result = crate::import::strip_annotations(&node.label);
    if node.kind != NodeKind::Deterministic {
        if !result.is_empty() {
            result.push(' ');
        }
        let _ = write!(result, "[kind: {}]", node.kind.as_str());
    }
    if let Some(dependencies) = dependencies
        && !dependencies.is_empty()
    {
        let natural_dependencies = crate::import::dependency_numbers(&result);
        let missing = dependencies
            .iter()
            .filter(|number| !natural_dependencies.contains(number))
            .map(usize::to_string)
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            if !result.is_empty() {
                result.push(' ');
            }
            let _ = write!(result, "[depends: {}]", missing.join(", "));
        }
    }
    result
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
