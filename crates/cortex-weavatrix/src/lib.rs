mod adapter;
mod context;
mod transport;

pub use adapter::{
    EvidenceBundle, EvidenceFragment, EvidenceKind, RefactorOperation, WeavatrixAdapter,
    WeavatrixConfig, WeavatrixError,
};
pub use context::{CompiledEvidenceBundle, compile_evidence_bundle};
pub use transport::{McpChild, McpCommand, McpError};
