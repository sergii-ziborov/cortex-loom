/// Naming-convention words in prose are not source identifiers. Backticked
/// uses remain explicit and are still eligible for identifier search.
pub(super) fn is_style_label(value: &str) -> bool {
    matches!(
        value,
        "snake_case" | "camelCase" | "PascalCase" | "SCREAMING_SNAKE" | "kebab-case"
    )
}

/// A creation task without a named owner type needs domain search, not a
/// complete definition of a convention word or stringly MCP tool label.
#[must_use]
pub fn is_new_feature_without_owner(task: &str) -> bool {
    super::intent::is_creation(task)
        && !super::extract_identifiers(task)
            .iter()
            .any(|name| is_owner_type(name))
}

fn is_owner_type(name: &str) -> bool {
    name.contains("::")
        || (name.chars().next().is_some_and(char::is_uppercase)
            && name.chars().any(char::is_lowercase))
}

#[cfg(test)]
mod tests {
    use super::is_new_feature_without_owner;

    #[test]
    fn convention_and_mcp_tool_labels_do_not_become_owner_definitions() {
        assert!(is_new_feature_without_owner(
            "Add Elevated priority with snake_case serialization and MCP context_compile schema"
        ));
        assert!(!is_new_feature_without_owner(
            "Add Elevated to EvidencePriority and update its rank"
        ));
    }
}
