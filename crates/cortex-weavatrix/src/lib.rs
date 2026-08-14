mod adapter;
mod context;
mod hints;
pub mod plan;
mod plan_intent;
mod refactor_preview;
mod source_followup;
mod verify;

pub use adapter::{
    EvidenceBundle, EvidenceFragment, EvidenceKind, WeavatrixAdapter, WeavatrixConfig,
    WeavatrixError,
};
pub use context::{CompiledEvidenceBundle, compile_evidence_bundle};
pub use hints::{IntentHint, PlanHints};
pub use refactor_preview::{PreviewChange, RefactorPreview, preview_refactor_plan};
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
