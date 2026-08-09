mod catalog;
mod error;
mod template;

pub use error::SequenceError;
pub use template::{
    ActivationHints, SequenceTemplate, TemplateRef, TemplateVersion, instantiate_template,
    templates,
};

#[cfg(test)]
mod tests;
