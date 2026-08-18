mod adapter;
mod budget;
mod certificate;
mod certified;
mod context;
mod definition;
mod fold;
mod hints;
mod languages;
mod layers;
mod mechanisms;
pub mod plan;
mod plan_intent;
mod refactor_preview;
mod run_memory;
mod snapshot;
mod source_followup;
mod verify;

pub use adapter::{
    EvidenceBundle, EvidenceFragment, EvidenceKind, WeavatrixAdapter, WeavatrixConfig,
    WeavatrixError,
};
pub use budget::{BudgetPin, adaptive_budget};
pub use certified::compile_certified_bundle;
pub use context::{
    CompiledEvidenceBundle, compile_evidence_bundle, compile_evidence_bundle_layered,
    compile_probe_bundle,
};
pub use fold::{
    DEFAULT_SOURCE_GLOB, fold_text, search_glob, segment_identifier, window_covers_span,
};
pub use hints::{IntentHint, PlanHints};
pub use languages::{LanguageInventory, inventory};
pub use plan_intent::{TaskIntent, asks_for_prior_attempts, detect};
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
        assert_eq!(weavatrix_rust::VERSION, "2.6.0");
    }
}
