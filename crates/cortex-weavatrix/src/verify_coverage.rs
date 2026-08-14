//! Task-specific source coverage contracts for sufficiency and retry.

use crate::plan::extract_identifiers;
use crate::plan_intent::TaskIntent;

pub(super) struct CoverageRequirement {
    pub label: String,
    pub content_patterns: Vec<String>,
    pub search_patterns: Vec<String>,
}

pub(super) fn coverage_requirements(
    task: &str,
    symbol: Option<&str>,
    intent: TaskIntent,
) -> Vec<CoverageRequirement> {
    let lower = task.to_ascii_lowercase();
    let mut requirements = Vec::new();
    let mut identifiers = extract_identifiers(task);
    if let Some(symbol) = symbol
        && !identifiers.iter().any(|identifier| identifier == symbol)
    {
        identifiers.insert(0, symbol.to_owned());
    }
    if let Some(symbol) = symbol
        && crate::plan_intent::is_creation(task)
    {
        let future_member_prefix = format!("{symbol}::");
        identifiers.retain(|identifier| !identifier.starts_with(&future_member_prefix));
    }
    let runtime_flag =
        runtime_flag_requirement(symbol.or_else(|| identifiers.first().map(String::as_str)));
    for identifier in identifiers.into_iter().take(4) {
        requirements.push(requirement(
            format!("identifier:{identifier}"),
            &[&identifier.to_ascii_lowercase()],
            &[&crate::plan::search_pattern(&[identifier])],
        ));
    }
    requirements.extend(lifecycle_requirements(&lower, symbol, intent, runtime_flag));
    requirements.extend(route_store_requirements(&lower, intent));
    requirements.extend(broad_silence_requirements(&lower));
    requirements.extend(quiet_mode_requirements(&lower));
    requirements
}

/// Quiet/result-mode questions need the quiet path, not only `finish_block`.
fn quiet_mode_requirements(lower: &str) -> Vec<CoverageRequirement> {
    if !lower.contains("quiet") {
        return Vec::new();
    }
    vec![requirement(
        "quiet_path",
        &["quiet_match", "fn quiet"],
        &["quiet_match", "fn quiet"],
    )]
}

/// Enumerating "why did this silently miss" questions need the three
/// mechanism classes that naive file reads always see and a thin symbol
/// packet drops: the enable flag, a count/size ceiling, and a path skip.
fn broad_silence_requirements(lower: &str) -> Vec<CoverageRequirement> {
    if !crate::plan_intent::is_broad(lower) {
        return Vec::new();
    }
    let silent = lower.contains("silent")
        || lower.contains("nothing")
        || (lower.contains("miss") && lower.contains("archive"));
    if !silent {
        return Vec::new();
    }
    vec![
        requirement(
            "option_enabled",
            &["pub enabled", " enabled:", ".enabled"],
            &[r"\benabled\b"],
        ),
        requirement(
            "count_limit",
            &["max_entries", "maxentries"],
            &["max_entries"],
        ),
        requirement(
            "path_guard",
            &["safe_virtual_path", "../", "parent dir", "traversal"],
            &["safe_virtual_path", r"\.\./", "traversal"],
        ),
    ]
}

fn lifecycle_requirements(
    lower: &str,
    symbol: Option<&str>,
    intent: TaskIntent,
    runtime_flag: Option<CoverageRequirement>,
) -> Vec<CoverageRequirement> {
    let mut requirements = Vec::new();
    if intent == TaskIntent::BlastRadius
        && lower.contains("depend")
        && let Some(symbol) = symbol
    {
        let symbol = symbol.to_ascii_lowercase();
        requirements.push(CoverageRequirement {
            label: "caller_usage".to_owned(),
            content_patterns: vec![
                format!(", {symbol})"),
                format!(": {symbol}("),
                format!("= {symbol}("),
                format!("return {symbol}("),
                format!("match {symbol}("),
            ],
            search_patterns: vec![format!(
                r",\s*{symbol}\s*\)|[:=]\s*{symbol}\s*\(|(return|match)\s+{symbol}\s*\("
            )],
        });
    }
    if lower.contains("uncalibrat") || lower.contains("profile gate") {
        requirements.push(requirement(
            "profile_gate_state",
            &["gate_passed", "gatepassed"],
            &["gate_passed", "gatePassed"],
        ));
        requirements.push(requirement(
            "profile_rejection",
            &["notcalibrated", "not calibrated"],
            &["NotCalibrated", "not calibrated"],
        ));
        requirements.push(requirement(
            "profile_selection",
            &["fn select", "pub fn select"],
            &["fn select", "pub fn select"],
        ));
    }
    if lower.contains("env flag") || lower.contains("environment variable") {
        requirements.push(
            runtime_flag.unwrap_or_else(|| {
                requirement("runtime_flag", &["cortex_"], &["CORTEX_[A-Z0-9_]+"])
            }),
        );
    }
    if lower.contains("spawn") {
        requirements.push(requirement(
            "spawn_lifecycle",
            &["fn spawn", "pub fn spawn"],
            &["fn spawn", "pub fn spawn"],
        ));
    }
    if lower.contains("shadowhandle") && lower.contains("spawn") {
        requirements.push(requirement(
            "shadow_observe",
            &["fn observe", "pub fn observe"],
            &["fn observe", "pub fn observe"],
        ));
    }
    requirements
}

fn route_store_requirements(lower: &str, intent: TaskIntent) -> Vec<CoverageRequirement> {
    let mut requirements = Vec::new();
    if lower.contains("wire") || lower.contains("wiring") {
        if lower.contains("cortex_llm") {
            requirements.push(requirement("router_wiring", &["llmrouter"], &["LlmRouter"]));
        } else {
            requirements.push(requirement(
                "router_wiring",
                &["router"],
                &["[A-Za-z0-9_]*Router"],
            ));
        }
        requirements.push(requirement(
            "tier_merge",
            &["merge_", "merge tiers"],
            &["merge_[A-Za-z0-9_]+"],
        ));
    }
    if lower.contains("policy") || lower.contains("permit") || lower.contains("refuse cpu") {
        requirements.push(requirement(
            "policy_predicate",
            &["fn permits", "pub fn permits"],
            &["fn permits", "pub fn permits"],
        ));
    }
    if intent == TaskIntent::ModuleTopology
        && lower.contains("run")
        && (lower.contains("persist") || lower.contains("store"))
    {
        requirements.push(requirement(
            "run_persistence",
            &["run_store", "runstore"],
            &["run_store", "RunStore"],
        ));
        requirements.push(requirement(
            "store_entrypoint",
            &["fn open", "pub fn open"],
            &["fn open", "pub fn open"],
        ));
    }
    requirements
}

fn runtime_flag_requirement(identifier: Option<&str>) -> Option<CoverageRequirement> {
    let identifier = identifier?;
    if identifier.starts_with("CORTEX_") {
        return Some(requirement(
            "runtime_flag",
            &[&identifier.to_ascii_lowercase()],
            &[&crate::plan::search_pattern(&[identifier.to_owned()])],
        ));
    }
    let mut stem: String = identifier
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_uppercase)
        .collect();
    for suffix in ["HANDLE", "CONFIG", "PROFILE", "REGISTRY", "ROUTER"] {
        if stem.len() > suffix.len() && stem.ends_with(suffix) {
            stem.truncate(stem.len() - suffix.len());
            break;
        }
    }
    (!stem.is_empty()).then(|| CoverageRequirement {
        label: "runtime_flag".to_owned(),
        content_patterns: vec![format!("cortex_{}", stem.to_ascii_lowercase())],
        search_patterns: vec![format!("CORTEX_[A-Z0-9_]*{stem}[A-Z0-9_]*")],
    })
}

fn requirement(
    label: impl Into<String>,
    content_patterns: &[&str],
    search_patterns: &[&str],
) -> CoverageRequirement {
    CoverageRequirement {
        label: label.into(),
        content_patterns: content_patterns
            .iter()
            .map(|pattern| pattern.to_ascii_lowercase())
            .collect(),
        search_patterns: search_patterns
            .iter()
            .map(|pattern| (*pattern).to_owned())
            .collect(),
    }
}
