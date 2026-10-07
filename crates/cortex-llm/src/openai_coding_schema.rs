//! Closed response shape for the advisory coding preview.

#[must_use]
pub fn coding_draft_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "edits": {
                "type": "array",
                "maxItems": 3,
                "items": {
                    "type": "object",
                    "properties": {
                        "evidenceId": {"type": "string"},
                        "find": {"type": "string"},
                        "replace": {"type": "string"},
                        "rationale": {"type": "string"}
                    },
                    "required": ["evidenceId", "find", "replace", "rationale"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["edits"],
        "additionalProperties": false
    })
}
