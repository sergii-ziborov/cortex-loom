use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceError {
    UnknownTemplate(String),
    InvalidCopy(String),
    InvalidSequence(String),
    NodeNotFound(String),
    Skill(String),
}

impl Display for SequenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTemplate(id) => write!(formatter, "unknown sequence template: {id}"),
            Self::InvalidCopy(message) => formatter.write_str(message),
            Self::InvalidSequence(message) => write!(formatter, "invalid sequence: {message}"),
            Self::NodeNotFound(id) => write!(formatter, "sequence node not found: {id}"),
            Self::Skill(message) => write!(formatter, "invalid sequence template: {message}"),
        }
    }
}

impl std::error::Error for SequenceError {}

impl From<cortex_skills::SkillError> for SequenceError {
    fn from(value: cortex_skills::SkillError) -> Self {
        Self::Skill(value.to_string())
    }
}
