mod adapter;
mod context;
pub mod plan;
mod plan_intent;
mod transport;

pub use adapter::{
    EvidenceBundle, EvidenceFragment, EvidenceKind, RefactorOperation, WeavatrixAdapter,
    WeavatrixConfig, WeavatrixError,
};
pub use context::{CompiledEvidenceBundle, compile_evidence_bundle};
pub use transport::{McpChild, McpCommand, McpError};
