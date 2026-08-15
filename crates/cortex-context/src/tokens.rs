//! Runtime token counting. The four-character rule is a comparative unit,
//! not a safety bound.

/// One chat turn for [`TokenCounter::count_message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Counts tokens for a budget. Implementations name themselves so a ledger
/// can say which counter produced a number.
///
/// Vendor tokenizers (OpenAI/Codex, Hugging Face JSON for Qwen, a
/// Claude-calibrated table) plug in here. Runtime compile uses
/// [`ConservativeCounter`] until one of those is wired for the active model.
pub trait TokenCounter {
    fn id(&self) -> &str;
    fn revision(&self) -> &str;
    fn count(&self, text: &str) -> u32;
    fn count_parts(&self, parts: &[&str]) -> u32 {
        parts
            .iter()
            .map(|part| self.count(part))
            .fold(0_u32, u32::saturating_add)
    }
    fn count_message(&self, message: &ChatMessage) -> u32 {
        self.count_parts(&[&message.role, &message.content])
    }
    fn count_tool_schema(&self, schema: &blazingly_json::Value) -> u32 {
        self.count(&schema.to_string())
    }
}

/// Historical comparative unit: `chars.div_ceil(4)`.
///
/// Fine for ranking two packets of the same language on the same bench.
/// Unsafe as a remaining-context bound for Cyrillic, CJK, JSON, or code.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CharDiv4Counter;

impl TokenCounter for CharDiv4Counter {
    fn id(&self) -> &'static str {
        "char_div4"
    }

    fn revision(&self) -> &'static str {
        "v1"
    }

    fn count(&self, text: &str) -> u32 {
        let tokens = text.chars().count().div_ceil(4).max(1);
        u32::try_from(tokens).unwrap_or(u32::MAX)
    }
}

/// Conservative overestimate for runtime budgets.
///
/// Non-Latin letters and punctuation cost one token each. ASCII words cost
/// one token per three characters. This is not a vendor tokenizer; it is
/// meant to refuse a packet before a model does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConservativeCounter;

impl TokenCounter for ConservativeCounter {
    fn id(&self) -> &'static str {
        "conservative"
    }

    fn revision(&self) -> &'static str {
        "v1"
    }

    fn count(&self, text: &str) -> u32 {
        let mut tokens = 0_u32;
        let mut ascii_run = 0_u32;
        for character in text.chars() {
            if character.is_whitespace() {
                flush_ascii(&mut tokens, &mut ascii_run);
                continue;
            }
            if character.is_ascii_alphanumeric() {
                ascii_run = ascii_run.saturating_add(1);
                continue;
            }
            flush_ascii(&mut tokens, &mut ascii_run);
            tokens = tokens.saturating_add(1);
        }
        flush_ascii(&mut tokens, &mut ascii_run);
        tokens.max(1)
    }
}

fn flush_ascii(tokens: &mut u32, ascii_run: &mut u32) {
    if *ascii_run == 0 {
        return;
    }
    *tokens = tokens.saturating_add(ascii_run.div_ceil(3));
    *ascii_run = 0;
}

/// Comparative helper used by benches. Runtime compile uses
/// [`ConservativeCounter`].
#[must_use]
pub fn estimate_tokens(value: &str) -> u32 {
    CharDiv4Counter.count(value)
}

pub const RUNTIME_COUNTER: ConservativeCounter = ConservativeCounter;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBreakdown {
    pub counter_id: String,
    pub tokenizer_revision: String,
    /// Every candidate, before budget or dedup.
    pub candidate_tokens: u32,
    /// Included items counted on their original bodies.
    pub selected_before_dedup_tokens: u32,
    /// What the packet actually contains after dedup.
    pub delivered_tokens: u32,
    /// Whole items dropped to fit the budget. Not a saving.
    pub budget_omitted_tokens: u32,
    /// Repeated lines removed from included items. This is a saving.
    pub dedup_saved_tokens: u32,
    /// Headers and wrappers around the evidence bodies.
    pub rendering_overhead_tokens: u32,
    /// Same unit as [`Self::candidate_tokens`]; kept so a ledger can say
    /// "estimated" without knowing which compile field to read.
    #[serde(default)]
    pub estimated_tokens: u32,
    /// Filled when an executor later reports vendor usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_provider_tokens: Option<u32>,
    /// `actual - estimated` once both exist. Positive means we under-counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimation_error: Option<i32>,
    /// Tokens on the wire after compile (packet body).
    #[serde(default)]
    pub wire_tokens: u32,
    /// Tool/JSON schema tokens counted separately from evidence.
    #[serde(default)]
    pub schema_tokens: u32,
    /// Instruction / wrapper tokens.
    #[serde(default)]
    pub instruction_tokens: u32,
    /// Evidence bodies as selected, before wrappers.
    #[serde(default)]
    pub evidence_tokens: u32,
    /// Reserved output budget. Compile does not consume this.
    #[serde(default)]
    pub output_tokens: u32,
}

impl TokenBreakdown {
    #[must_use]
    pub fn for_counter(counter: &dyn TokenCounter) -> Self {
        Self {
            counter_id: counter.id().to_owned(),
            tokenizer_revision: counter.revision().to_owned(),
            candidate_tokens: 0,
            selected_before_dedup_tokens: 0,
            delivered_tokens: 0,
            budget_omitted_tokens: 0,
            dedup_saved_tokens: 0,
            rendering_overhead_tokens: 0,
            estimated_tokens: 0,
            actual_provider_tokens: None,
            estimation_error: None,
            wire_tokens: 0,
            schema_tokens: 0,
            instruction_tokens: 0,
            evidence_tokens: 0,
            output_tokens: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conservative_overestimates_cyrillic_against_char_div4() {
        let text = "Предыдущая попытка молча пропустила архив";
        assert!(
            ConservativeCounter.count(text) > CharDiv4Counter.count(text),
            "cyrillic must not be treated as four chars per token"
        );
    }

    #[test]
    fn two_russian_sentences_do_not_share_a_div4_cost_with_code() {
        let rust = "fn finish_block()";
        let russian = "предыдущая попытка";
        assert_eq!(CharDiv4Counter.count(rust), CharDiv4Counter.count(russian));
        assert!(ConservativeCounter.count(russian) > ConservativeCounter.count(rust));
    }

    #[test]
    fn message_and_schema_counts_are_conservative() {
        let message = ChatMessage {
            role: "user".to_owned(),
            content: "предыдущая попытка".to_owned(),
        };
        assert!(
            ConservativeCounter.count_message(&message) > CharDiv4Counter.count_message(&message)
        );
        let schema = blazingly_json::json!({
            "type": "object",
            "properties": { "id": { "type": "string" } }
        });
        assert!(ConservativeCounter.count_tool_schema(&schema) >= 1);
    }
}
