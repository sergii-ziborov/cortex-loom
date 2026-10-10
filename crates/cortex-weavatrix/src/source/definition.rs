//! Lexical fallback for definition heads and complete source spans.

/// Where a named definition head starts in source text.
#[must_use]
pub fn definition_head_index(text: &str, symbol: &str) -> Option<usize> {
    // ASCII lowercasing preserves byte offsets into the original source.
    let lower = text.to_ascii_lowercase();
    let symbol = symbol.to_ascii_lowercase();
    for keyword in [
        "fn ",
        "struct ",
        "enum ",
        "trait ",
        "type ",
        "class ",
        "interface ",
        "function ",
        "def ",
        "func ",
        "record ",
        "const ",
        "static ",
    ] {
        let mut from = 0;
        while let Some(relative) = lower[from..].find(keyword) {
            let head = from + relative;
            let name_start = head + keyword.len();
            let after_name = name_start + symbol.len();
            if lower[name_start..].starts_with(symbol.as_str())
                && !lower[after_name..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Some(head);
            }
            from = name_start;
        }
    }
    None
}

/// Whether the complete named item is present in a source window.
///
/// Constants and statics need their terminating semicolon, including when
/// their initializer contains a block or an array with an inner semicolon.
#[must_use]
pub fn definition_is_complete(text: &str, symbol: &str) -> Option<bool> {
    let head = definition_head_index(text, symbol)?;
    let rest = &text[head..];
    let rust_lifetimes = ["fn ", "struct ", "enum ", "trait "]
        .iter()
        .any(|head| rest.starts_with(head))
        || rest.contains("&'");
    let semicolon_item = rest.starts_with("const ") || rest.starts_with("static ");
    let mut braces = 0_i32;
    let mut brackets = 0_i32;
    let mut parentheses = 0_i32;
    let mut opened = false;
    let mut mode = BraceMode::Code;
    let mut previous = '\0';
    for (offset, character) in rest.char_indices() {
        if character == '\''
            && rust_lifetimes
            && mode == BraceMode::Code
            && is_lifetime(&rest[offset + 1..])
        {
            previous = character;
            continue;
        }
        mode = advance_brace_mode(mode, previous, character);
        if mode == BraceMode::Code {
            match character {
                '{' => {
                    braces += 1;
                    opened = true;
                }
                '}' => {
                    braces -= 1;
                    if opened && braces == 0 && !semicolon_item {
                        return Some(true);
                    }
                }
                '[' if semicolon_item => brackets += 1,
                ']' if semicolon_item => brackets -= 1,
                '(' if semicolon_item => parentheses += 1,
                ')' if semicolon_item => parentheses -= 1,
                ';' if braces == 0
                    && brackets == 0
                    && parentheses == 0
                    && (semicolon_item || !opened) =>
                {
                    return Some(true);
                }
                _ => {}
            }
        }
        previous = character;
    }
    Some(false)
}

fn is_lifetime(tail: &str) -> bool {
    let mut chars = tail.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_') && chars.next() != Some('\'')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BraceMode {
    Code,
    LineComment,
    BlockComment,
    String,
    Char,
}

fn advance_brace_mode(mode: BraceMode, previous: char, character: char) -> BraceMode {
    match mode {
        BraceMode::LineComment if character == '\n' => BraceMode::Code,
        BraceMode::LineComment => BraceMode::LineComment,
        BraceMode::BlockComment if previous == '*' && character == '/' => BraceMode::Code,
        BraceMode::BlockComment => BraceMode::BlockComment,
        BraceMode::String if previous != '\\' && character == '"' => BraceMode::Code,
        BraceMode::String => BraceMode::String,
        BraceMode::Char if previous != '\\' && character == '\'' => BraceMode::Code,
        BraceMode::Char => BraceMode::Char,
        BraceMode::Code if previous == '/' && character == '/' => BraceMode::LineComment,
        BraceMode::Code if previous == '/' && character == '*' => BraceMode::BlockComment,
        BraceMode::Code if character == '"' => BraceMode::String,
        BraceMode::Code if character == '\'' => BraceMode::Char,
        BraceMode::Code => BraceMode::Code,
    }
}

#[cfg(test)]
mod tests {
    use super::{definition_head_index, definition_is_complete};

    #[test]
    fn rust_constants_and_statics_are_complete_only_at_the_outer_semicolon() {
        assert!(definition_head_index("const MAX_PACKETS: usize = 32;", "MAX_PACKETS").is_some());
        assert_eq!(
            definition_is_complete("const MAX_PACKETS: usize = 32;", "MAX_PACKETS"),
            Some(true)
        );
        assert_eq!(
            definition_is_complete("const MAX_PACKETS: usize =", "MAX_PACKETS"),
            Some(false)
        );
        assert_eq!(
            definition_is_complete("static ITEMS: [u8; 2] = [1, 2]", "ITEMS"),
            Some(false)
        );
        assert_eq!(
            definition_is_complete("static ITEMS: [u8; 2] = [1, 2];", "ITEMS"),
            Some(true)
        );
        assert_eq!(
            definition_is_complete("const VALUE: u8 = { let x = 1; x }", "VALUE"),
            Some(false)
        );
        assert_eq!(
            definition_is_complete("const VALUE: u8 = { let x = 1; x };", "VALUE"),
            Some(true)
        );
        assert_eq!(
            definition_is_complete("const LABEL: &'static str = \"ok\";", "LABEL"),
            Some(true)
        );
    }
}
