//! Operator policy and classifier selection for agent prepare.

use cortex_router::{RoutingRequest, route};

use crate::CortexMcpState;
use crate::llm_route::{LlmBackend, LlmRouteConfig, RoutedWork};

pub(super) fn combined_warnings(compiled: &[String], routing: Option<&str>) -> Vec<String> {
    let mut warnings = compiled.to_vec();
    if let Some(warning) = routing {
        warnings.push(format!("routing classifier: {warning}"));
    }
    warnings
}

pub(super) fn route_prepare(
    state: &CortexMcpState,
    request: &RoutingRequest,
    alias: Option<&str>,
) -> RoutedWork {
    match alias {
        Some(alias)
            if !matches!(
                LlmRouteConfig::from_env().resolve_backend(),
                Ok(LlmBackend::Composer)
            ) =>
        {
            let mut work = RoutedWork::lexical(route(request));
            work.classifier_model = Some(alias.to_owned());
            work.warning = Some("classifierModel blocked by operator backend policy".to_owned());
            work
        }
        Some(alias) => match crate::composer_llm::router_for_alias(alias) {
            Ok(router) => router.decide_prepare(request),
            Err(error) => {
                let mut work = RoutedWork::lexical(route(request));
                work.attempted = true;
                work.warning = Some(error);
                work.classifier_model = Some(alias.to_owned());
                work
            }
        },
        None => state.llm_router.as_ref().map_or_else(
            || RoutedWork::lexical(route(request)),
            |router| router.decide_prepare(request),
        ),
    }
}
