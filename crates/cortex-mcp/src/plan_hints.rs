//! Translate active-skill metadata into protocol-independent `PlanHints`.

use cortex_domain::GraphDocument;
use cortex_store::GraphStore;
use cortex_weavatrix::{IntentHint, PlanHints};

pub(crate) fn seed_bundled_skills(store: &GraphStore) -> Result<(), String> {
    for skill in cortex_skills::bundled_skills() {
        let graph = cortex_skills::import_skill_markdown(skill.source, skill.markdown)
            .map_err(|error| format!("bundled skill {} is invalid: {error}", skill.id))?;
        store
            .seed_or_refresh_unsaved(&graph)
            .map_err(|error| format!("could not seed bundled skill {}: {error}", skill.id))?;
    }
    Ok(())
}

pub(crate) fn from_graph(graph: &GraphDocument) -> Result<PlanHints, String> {
    Ok(PlanHints {
        intent: optional(graph, "context-intent", "context_intent")
            .map(parse_intent)
            .transpose()?,
        source_followup: optional(graph, "source-followup", "source_followup")
            .map(|value| parse_bool("source-followup", value))
            .transpose()?,
        skip_change_plan: optional(graph, "skip-change-plan", "skip_change_plan")
            .map(|value| parse_bool("skip-change-plan", value))
            .transpose()?
            .unwrap_or(false),
    })
}

fn optional<'a>(graph: &'a GraphDocument, dashed: &str, underscored: &str) -> Option<&'a str> {
    graph
        .metadata
        .get(&format!("frontmatter.{dashed}"))
        .or_else(|| graph.metadata.get(&format!("frontmatter.{underscored}")))
        .map(String::as_str)
}

fn parse_intent(value: &str) -> Result<IntentHint, String> {
    match value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
        .as_str()
    {
        "identifier_change" | "identifier" => Ok(IntentHint::IdentifierChange),
        "blast_radius" | "blast" => Ok(IntentHint::BlastRadius),
        "api_contract" | "api" => Ok(IntentHint::ApiContract),
        "module_topology" | "module" => Ok(IntentHint::ModuleTopology),
        "runtime_config" | "config" => Ok(IntentHint::RuntimeConfig),
        "git_history" | "git" | "history" => Ok(IntentHint::GitHistory),
        "stack_trace" | "stacktrace" | "backtrace" => Ok(IntentHint::StackTrace),
        "test_selection" | "tests" | "select_tests" => Ok(IntentHint::TestSelection),
        other => Err(format!(
            "skill context-intent `{other}` is unsupported; expected identifier_change, blast_radius, api_contract, module_topology, runtime_config, git_history, stack_trace, or test_selection"
        )),
    }
}

fn parse_bool(name: &str, value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("skill {name} must be true or false, got `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_frontmatter_becomes_typed_plan_hints() {
        let mut graph = cortex_domain::default_control_plane();
        graph.metadata.insert(
            "frontmatter.context-intent".to_owned(),
            "runtime-config".to_owned(),
        );
        graph
            .metadata
            .insert("frontmatter.source-followup".to_owned(), "true".to_owned());
        graph
            .metadata
            .insert("frontmatter.skip-change-plan".to_owned(), "true".to_owned());
        assert_eq!(
            from_graph(&graph).unwrap(),
            PlanHints {
                intent: Some(IntentHint::RuntimeConfig),
                source_followup: Some(true),
                skip_change_plan: true,
            }
        );
    }

    #[test]
    fn invalid_skill_hints_fail_closed() {
        let mut graph = cortex_domain::default_control_plane();
        graph.metadata.insert(
            "frontmatter.source-followup".to_owned(),
            "sometimes".to_owned(),
        );
        assert!(from_graph(&graph).unwrap_err().contains("true or false"));
    }

    #[test]
    fn standalone_mcp_storage_receives_hint_aware_bundled_skills() {
        let store = GraphStore::open_in_memory().unwrap();
        seed_bundled_skills(&store).unwrap();
        let graph = store
            .list()
            .unwrap()
            .into_iter()
            .find(|graph| graph.name == "Configuration Drift")
            .unwrap();
        assert_eq!(
            from_graph(&graph).unwrap().intent,
            Some(IntentHint::RuntimeConfig)
        );
    }
}
