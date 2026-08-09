mod adapter;
mod context;
mod hints;
pub mod plan;
mod plan_intent;
mod source_followup;
mod transport;
mod verify;

pub use adapter::{
    EvidenceBundle, EvidenceFragment, EvidenceKind, RefactorOperation, WeavatrixAdapter,
    WeavatrixConfig, WeavatrixError,
};
pub use context::{CompiledEvidenceBundle, compile_evidence_bundle};
pub use hints::{IntentHint, PlanHints};
pub use transport::{McpChild, McpCommand, McpError};
pub use verify::{EvidenceSufficiency, assess_compiled};
