mod adapter;
mod compile;
mod core;
mod memory;
pub mod plan;
mod source;
mod verify;

pub use adapter::{
    EvidenceBundle, EvidenceFragment, EvidenceKind, WeavatrixAdapter, WeavatrixConfig,
    WeavatrixError,
};
pub use compile::{certificate, certified, context};
pub use core::{budget, definition, fold, hints, languages, layers, mechanisms, snapshot};
pub use memory::{refactor_preview, run_memory};
pub use plan::intent as plan_intent;
pub(crate) use source as source_followup;

pub use budget::{BudgetPin, adaptive_budget};
pub use certified::compile_certified_bundle;
pub use context::{
    CompiledEvidenceBundle, compile_evidence_bundle, compile_evidence_bundle_layered,
    compile_probe_bundle,
};
pub use fold::{
    DEFAULT_SOURCE_GLOB, fold_text, graph_seed, is_graph_symbol, is_source_file_name,
    named_crate_scope, named_source_files, search_glob, segment_identifier, window_covers_span,
};
pub use hints::{IntentHint, PlanHints};
pub use languages::{LanguageInventory, inventory};
pub use plan::asks_for_coverage;
pub use plan_intent::{
    TaskIntent, asks_for_dead_production, asks_for_duplicate_share, asks_for_prior_attempts, detect,
};
pub use refactor_preview::{PreviewChange, RefactorPreview, preview_refactor_plan};
pub use run_memory::{PriorRunEvent, PriorRunMemory};
pub use snapshot::repository_snapshot;
pub use verify::{EvidenceSufficiency, assess_compiled};

#[cfg(test)]
mod contract_tests {
    #[test]
    fn first_party_plan_contract_is_available() {
        let limits = weavatrix_refactor_plan::RefactorPlanLimits::default();
        assert!(limits.max_operations > 0);
        assert_eq!(weavatrix_rust::VERSION, "2.17.4");
    }
}
