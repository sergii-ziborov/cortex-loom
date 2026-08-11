use serde_json::json;
use weavatrix_rust::Weavatrix;

use super::evidence::{EvidenceFragment, EvidenceKind, fragments, native_call};

#[allow(clippy::too_many_arguments)]
pub(super) fn retry_wide_search(
    engine: &mut Weavatrix,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    search_hits: &mut Vec<crate::source_followup::SearchHit>,
    task: &str,
    symbol: Option<&str>,
    hints: crate::PlanHints,
    missing: &[String],
    budget: u32,
    policy: crate::plan::PlanPolicy,
) {
    let queries = crate::verify::retry_search_queries(task, symbol, hints, missing);
    if queries.is_empty() {
        warnings.push("wide search retry skipped: no missing semantic terms".to_owned());
        return;
    }
    let token_budget = policy
        .search_tokens
        .min(budget.saturating_mul(2) / 5)
        .max(200);
    let query_count = u32::try_from(queries.len()).unwrap_or(u32::MAX).max(1);
    let per_query_budget = (token_budget / query_count).max(200);
    for (index, query) in queries.into_iter().enumerate() {
        let arguments = retry_search_arguments(&query, per_query_budget);
        match native_call(engine, "search_code", arguments) {
            Ok(value) => {
                search_hits.extend(crate::source_followup::hits_from_search(&value));
                evidence.extend(fragments(
                    &format!("WX-RETRY-SEARCH-{}", index + 1),
                    EvidenceKind::SearchHits,
                    "weavatrix:search_code",
                    &value,
                ));
            }
            Err(error) => warnings.push(format!("wide search retry unavailable: {error}")),
        }
    }
}

fn retry_search_arguments(query: &str, token_budget: u32) -> serde_json::Value {
    json!({
        "query": query,
        "is_regex": true,
        "before": 2,
        "after": 2,
        "max_results": 40,
        "glob": "{src,apps,crates,ui,config}/**/*",
        "token_budget": token_budget,
    })
}

#[cfg(test)]
mod tests {
    use super::retry_search_arguments;

    #[test]
    fn recovery_search_stays_inside_source_and_config_trees() {
        let arguments = retry_search_arguments("ArchiveOptions", 1_400);

        assert_eq!(arguments["glob"], "{src,apps,crates,ui,config}/**/*");
    }
}
