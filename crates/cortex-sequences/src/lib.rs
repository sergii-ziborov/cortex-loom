mod activation;
mod catalog;
mod error;
mod lint;
mod packet;
mod template;

pub use error::SequenceError;
pub use lint::{DiagnosticCode, DiagnosticSeverity, SequenceDiagnostic, lint_sequence};
pub use packet::{ActiveStepPacket, active_step_packet};
pub use template::{
    ActivationHints, SequenceTemplate, TemplateRef, TemplateVersion, instantiate_template,
    templates,
};

#[cfg(test)]
mod tests;
pub use activation::{SequenceCandidate, candidate_templates};
