//! Unicode-aware folding for lexical classifiers.
//!
//! ASCII lowercase treats Cyrillic, Hebrew, and German ß as opaque, so a
//! Russian mutation cue never fires. NFKC plus Unicode case fold is the
//! minimum that makes mixed-language tasks comparable.

use unicode_normalization::UnicodeNormalization;

/// Compatibility-normalize, case-fold, and collapse whitespace.
#[must_use]
pub fn fold_text(value: &str) -> String {
    value
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when the task mixes Latin letters with another script.
#[must_use]
pub fn mixed_script(value: &str) -> bool {
    let mut latin = false;
    let mut other = false;
    for ch in value.chars() {
        if ch.is_ascii_alphabetic() {
            latin = true;
        } else if ch.is_alphabetic() {
            other = true;
        }
        if latin && other {
            return true;
        }
    }
    false
}

/// Fold, then keep letters/digits and turn everything else into spaces so
/// phrase matching can use word boundaries.
#[must_use]
pub fn fold_words(value: &str) -> String {
    fold_text(value)
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyrillic_case_folds() {
        assert_eq!(fold_text("  ДОБАВЬ  Обработку "), "добавь обработку");
    }

    #[test]
    fn nfkc_composes_compatibility_forms() {
        assert_eq!(fold_text("ﬁx"), "fix");
    }
}
