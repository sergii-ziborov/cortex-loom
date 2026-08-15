//! Second-hop type expansion and callee-file follow-up for broad questions.

use weavatrix_rust::Weavatrix;

use super::evidence::{EvidenceFragment, EvidenceKind};
use super::source_reads::append_definition_read_as;

/// Second-hop type expansion for enumerating questions.
///
/// A broad question answered from one symbol's neighbourhood stays blind to
/// the types that neighbourhood *uses*. Field types (`archives: ArchiveOptions`)
/// outrank frequent signature noise so the hop that answers the question is
/// not spent on `CompiledQuery` / `Collector`. Three rounds leave a slot for
/// the type that only appears after an intermediate definition is read.
pub(super) fn append_type_expansion_reads(
    engine: &mut Weavatrix,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    task: &str,
    budget: u32,
) {
    const MAX_EXPANSIONS: usize = 5;
    const READS_PER_ROUND: usize = 2;
    const CANDIDATES_PER_ROUND: usize = 6;
    let mut read = 0_usize;
    let mut tried: Vec<String> = Vec::new();
    for _round in 0..3 {
        if read >= MAX_EXPANSIONS {
            break;
        }
        let picks = expansion_candidates(evidence, task, &tried, CANDIDATES_PER_ROUND);
        if picks.is_empty() {
            break;
        }
        let mut this_round = 0_usize;
        for name in picks {
            if this_round >= READS_PER_ROUND || read >= MAX_EXPANSIONS {
                break;
            }
            tried.push(name.clone());
            if append_definition_read_as(
                engine,
                evidence,
                warnings,
                &[],
                &name,
                budget,
                false,
                &format!("WX-TYPE-{}", read + 1),
                EvidenceKind::TypeExpansion,
            ) {
                read += 1;
                this_round += 1;
            }
        }
        if this_round == 0 {
            break;
        }
    }
}

/// Call-target files from a rendered symbol bundle.
///
/// Search only names the files that matched the query. On a broad question
/// the answering skip/limit often lives in a callee (`search_tar` →
/// `safe_virtual_path` in `containers.rs`). The graph already listed those
/// files; this just turns them into source windows.
#[must_use]
pub(super) fn callee_hits_from_evidence(
    evidence: &[EvidenceFragment],
) -> Vec<crate::source_followup::SearchHit> {
    let mut hits = Vec::new();
    for fragment in evidence {
        if fragment.kind != EvidenceKind::SymbolContext {
            continue;
        }
        for line in fragment.content.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("-> ") {
                continue;
            }
            if let Some(hit) = callee_hit(trimmed) {
                hits.push(hit);
            }
        }
    }
    hits
}

fn callee_hit(line: &str) -> Option<crate::source_followup::SearchHit> {
    let (path, rest) = line
        .split_whitespace()
        .find_map(|token| token.split_once(':'))
        .filter(|(path, _)| {
            std::path::Path::new(path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        })?;
    let line = rest
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(1);
    Some(crate::source_followup::SearchHit {
        path: path.to_owned(),
        line,
        text: String::new(),
    })
}

fn expansion_candidates(
    evidence: &[EvidenceFragment],
    task: &str,
    tried: &[String],
    limit: usize,
) -> Vec<String> {
    let source_text: String = evidence
        .iter()
        .filter(|fragment| {
            matches!(
                fragment.kind,
                EvidenceKind::SourceReads | EvidenceKind::TypeExpansion
            )
        })
        .map(|fragment| fragment.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let field_types = field_type_names(&source_text);
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for candidate in pascal_case_words(&source_text) {
        *counts.entry(candidate).or_default() += 1;
    }
    let task_words: Vec<String> = task
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 5)
        .map(ToOwned::to_owned)
        .collect();
    let mut ranked: Vec<(bool, bool, bool, usize, String)> = counts
        .into_iter()
        .filter(|(name, _)| !tried.iter().any(|seen| seen == name))
        .filter(|(name, _)| {
            !evidence.iter().any(|fragment| {
                crate::source_followup::definition_is_complete(&fragment.content, name)
                    == Some(true)
            })
        })
        .map(|(name, count)| {
            let lower = name.to_ascii_lowercase();
            let affinity = task_words.iter().any(|word| lower.contains(word.as_str()));
            let field = field_types.iter().any(|field| field == &name);
            let policy = lower.ends_with("options")
                || lower.ends_with("policy")
                || lower.ends_with("config");
            (policy, field, affinity, count, name)
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| right.3.cmp(&left.3))
            .then_with(|| right.4.len().cmp(&left.4.len()))
            .then_with(|| left.4.cmp(&right.4))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, _, _, _, name)| name)
        .collect()
}

fn field_type_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let Some((_, after)) = line.split_once(':') else {
            continue;
        };
        for candidate in pascal_case_words(after) {
            if !names.iter().any(|seen| seen == &candidate) {
                names.push(candidate);
            }
        }
    }
    names
}

fn pascal_case_words(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "Self",
        "String",
        "Option",
        "Result",
        "Some",
        "None",
        "Err",
        "Vec",
        "Box",
        "Arc",
        "Rc",
        "Cell",
        "RefCell",
        "PathBuf",
        "Path",
        "HashMap",
        "HashSet",
        "BTreeMap",
        "BTreeSet",
        "Value",
        "Debug",
        "Clone",
        "Copy",
        "Default",
        "PartialEq",
        "Serialize",
        "Deserialize",
        "Read",
        "Write",
        "Cursor",
        "Iterator",
        "Into",
        "From",
        "TryFrom",
        "Send",
        "Sync",
        "Sized",
        "Ord",
        "Eq",
        "Hash",
        "Display",
        "Error",
        "Instant",
        "Duration",
    ];
    let mut words = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            current.push(character);
        } else {
            if is_pascal_case(&current) && !STOP.contains(&current.as_str()) {
                words.push(std::mem::take(&mut current));
            }
            current.clear();
        }
    }
    if is_pascal_case(&current) && !STOP.contains(&current.as_str()) {
        words.push(current);
    }
    words
}

fn is_pascal_case(word: &str) -> bool {
    word.len() >= 6
        && word.chars().next().is_some_and(char::is_uppercase)
        && word.chars().any(char::is_lowercase)
        && !word.contains('_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::evidence::EvidenceFragment;

    fn fragment(kind: EvidenceKind, content: &str) -> EvidenceFragment {
        EvidenceFragment::new("WX", kind, "test", content)
    }

    #[test]
    fn field_types_outrank_frequent_signature_noise() {
        let evidence = [fragment(
            EvidenceKind::SourceReads,
            "fn search(query: &Arc<CompiledQuery>, collector: &Arc<Collector>) {}\n\
             pub struct SearchOptions {\n    pub archives: ArchiveOptions,\n}\n",
        )];
        let picks = expansion_candidates(
            &evidence,
            "List every mechanism that can silently cause an archive miss.",
            &[],
            3,
        );
        assert_eq!(picks.first().map(String::as_str), Some("ArchiveOptions"));
    }

    #[test]
    fn outgoing_call_edges_become_source_hits() {
        let evidence = [fragment(
            EvidenceKind::SymbolContext,
            "symbol search_compressed_tar (function) src/archive/compression.rs:9-28\n\
             relationships: 2\n\
               -> calls read_limited (function) src/archive/containers.rs:186 via src/archive/compression.rs:18\n\
               -> calls search_tar (function) src/archive/containers.rs:94 via src/archive/compression.rs:19\n\
               <- calls compressed_tar (method) src/archive/dispatch.rs:103 via src/archive/dispatch.rs:104\n",
        )];
        let hits = callee_hits_from_evidence(&evidence);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "src/archive/containers.rs");
        assert_eq!(hits[0].line, 186);
        assert_eq!(hits[1].path, "src/archive/containers.rs");
        assert_eq!(hits[1].line, 94);
    }
}
