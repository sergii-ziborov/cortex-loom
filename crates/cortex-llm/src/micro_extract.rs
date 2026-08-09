use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use serde_json::{Map, Value, json};

const MAX_VERIFIED_INPUT_BYTES: usize = 64 * 1024;
const MAX_FIELDS: usize = 32;
const MAX_FIELD_BYTES: usize = 64;
const MAX_VALUES_PER_FIELD: usize = 64;
const DEFAULT_OUTPUT_TOKENS: u32 = 128;
const MAX_OUTPUT_TOKENS: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicroExtractRequest {
    verified_input: String,
    allowed_fields: Vec<String>,
    max_output_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicroExtractOutput {
    pub fields: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicroExtractError {
    EmptyVerifiedInput,
    InputTooLarge,
    NoAllowedFields,
    InvalidField(String),
    InvalidOutputBudget(u32),
    InvalidOutput(String),
    UnsupportedValue { field: String, value: String },
}

impl Display for MicroExtractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyVerifiedInput => formatter.write_str("verified input must not be empty"),
            Self::InputTooLarge => write!(
                formatter,
                "verified input exceeds {MAX_VERIFIED_INPUT_BYTES} bytes"
            ),
            Self::NoAllowedFields => formatter.write_str("allowed fields must not be empty"),
            Self::InvalidField(field) => write!(formatter, "invalid allowed field: {field:?}"),
            Self::InvalidOutputBudget(tokens) => write!(
                formatter,
                "micro extraction output budget {tokens} must be in 1..={MAX_OUTPUT_TOKENS}"
            ),
            Self::InvalidOutput(message) => {
                write!(formatter, "invalid extraction output: {message}")
            }
            Self::UnsupportedValue { field, value } => write!(
                formatter,
                "field {field:?} contains a value absent from verified input: {value:?}"
            ),
        }
    }
}

impl std::error::Error for MicroExtractError {}

impl MicroExtractRequest {
    #[must_use]
    pub fn verified_input(&self) -> &str {
        &self.verified_input
    }

    #[must_use]
    pub fn allowed_fields(&self) -> &[String] {
        &self.allowed_fields
    }

    #[must_use]
    pub const fn max_output_tokens(&self) -> u32 {
        self.max_output_tokens
    }

    /// Build a closed, non-authoritative extraction request.
    ///
    /// # Errors
    ///
    /// Rejects empty or oversized verified input and invalid field lists.
    pub fn new(verified_input: &str, allowed_fields: &[&str]) -> Result<Self, MicroExtractError> {
        let verified_input = verified_input.trim();
        if verified_input.is_empty() {
            return Err(MicroExtractError::EmptyVerifiedInput);
        }
        if verified_input.len() > MAX_VERIFIED_INPUT_BYTES {
            return Err(MicroExtractError::InputTooLarge);
        }
        if allowed_fields.is_empty() || allowed_fields.len() > MAX_FIELDS {
            return Err(MicroExtractError::NoAllowedFields);
        }
        let mut unique = BTreeSet::new();
        for field in allowed_fields {
            let field = field.trim();
            if field.is_empty()
                || field.len() > MAX_FIELD_BYTES
                || !field
                    .chars()
                    .all(|character| character.is_alphanumeric() || character == '_')
                || !unique.insert(field.to_owned())
            {
                return Err(MicroExtractError::InvalidField(field.to_owned()));
            }
        }
        Ok(Self {
            verified_input: verified_input.to_owned(),
            allowed_fields: unique.into_iter().collect(),
            max_output_tokens: DEFAULT_OUTPUT_TOKENS,
        })
    }

    /// Replace the default output cap without widening the hard bound.
    ///
    /// # Errors
    ///
    /// Rejects zero or more than 256 output tokens.
    pub fn with_max_output_tokens(mut self, tokens: u32) -> Result<Self, MicroExtractError> {
        if !(1..=MAX_OUTPUT_TOKENS).contains(&tokens) {
            return Err(MicroExtractError::InvalidOutputBudget(tokens));
        }
        self.max_output_tokens = tokens;
        Ok(self)
    }

    #[must_use]
    pub fn output_schema(&self) -> Value {
        let properties: Map<String, Value> = self
            .allowed_fields
            .iter()
            .map(|field| {
                (
                    field.clone(),
                    json!({
                        "oneOf": [
                            {"type": "string"},
                            {"type": "array", "maxItems": MAX_VALUES_PER_FIELD, "items": {"type": "string"}},
                            {"type": "null"}
                        ]
                    }),
                )
            })
            .collect();
        json!({
            "type": "object",
            "properties": properties,
            "additionalProperties": false
        })
    }

    /// Validate that output is closed and every extracted value is literal.
    ///
    /// # Errors
    ///
    /// Rejects unknown fields, free-form shapes, duplicates, and values absent
    /// from the verified input.
    pub fn validate_output(&self, value: &Value) -> Result<MicroExtractOutput, MicroExtractError> {
        let object = value.as_object().ok_or_else(|| {
            MicroExtractError::InvalidOutput("expected one JSON object".to_owned())
        })?;
        let allowed: BTreeSet<&str> = self.allowed_fields.iter().map(String::as_str).collect();
        let mut fields = BTreeMap::new();
        for (field, value) in object {
            if !allowed.contains(field.as_str()) {
                return Err(MicroExtractError::InvalidOutput(format!(
                    "field {field:?} was not allowed"
                )));
            }
            let values = output_values(field, value)?;
            for extracted in &values {
                if !self.verified_input.contains(extracted) {
                    return Err(MicroExtractError::UnsupportedValue {
                        field: field.clone(),
                        value: extracted.clone(),
                    });
                }
            }
            fields.insert(field.clone(), values);
        }
        Ok(MicroExtractOutput { fields })
    }
}

fn output_values(field: &str, value: &Value) -> Result<Vec<String>, MicroExtractError> {
    let raw: Vec<&str> = match value {
        Value::Null => Vec::new(),
        Value::String(value) => vec![value],
        Value::Array(values) if values.len() <= MAX_VALUES_PER_FIELD => values
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                MicroExtractError::InvalidOutput(format!("field {field:?} must contain strings"))
            })?,
        _ => {
            return Err(MicroExtractError::InvalidOutput(format!(
                "field {field:?} must be a string, string array, or null"
            )));
        }
    };
    let mut unique = BTreeSet::new();
    let mut result = Vec::with_capacity(raw.len());
    for item in raw {
        if item.is_empty() || !unique.insert(item) {
            return Err(MicroExtractError::InvalidOutput(format!(
                "field {field:?} contains an empty or duplicate value"
            )));
        }
        result.push(item.to_owned());
    }
    Ok(result)
}
