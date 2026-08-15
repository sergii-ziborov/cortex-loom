# cortex-context

Deterministic, budget-bounded evidence selection for LLM context packets.

You hand it typed evidence items with explicit priorities and trust states
plus a token budget. It returns one packet with stable citation IDs, an
explicit list of what was omitted, token-savings accounting, and a
fail-closed escalation flag — without asking a model to decide what matters.

## Why

Context assembly is usually an implicit, unauditable step: something
concatenates strings until a limit is hit. This crate makes it a decision you
can inspect and test:

- **Priority, not vibes.** Selection order is `contradictory-first`, then
  explicit priority, then optional semantic relevance, then submission order.
- **Critical evidence never disappears quietly.** If a `Critical` item does
  not fit the budget, you get `ContextError::CriticalItemExceedsBudget`, not a
  silently shorter prompt.
- **Omissions are reported.** `omitted_ids` tells the caller exactly what was
  dropped, so a downstream agent can ask for it.
- **Unverified input forces escalation.** Any non-verified item sets
  `requires_upstream`.
- **Overlap is paid for once.** Evidence assembled from several tools repeats
  itself, and no single tool can see that — each budgets its own answer in
  isolation. With `deduplicate` (on by default) a line that a
  higher-priority item already carried is replaced by
  `same source span as [id]` only when span, content, snapshot, blob, trust,
  and derivation all match. Short lines are never touched, and an item that
  would render empty keeps its content.
- **Trust is visible in the packet text.** Headings carry
  `EXACT SOURCE`, `UNVERIFIED PLAN`, or `CONTRADICTORY — group C7`, not just
  an envelope flag.
- **IDs are revision-stable.** `packetId` is `pk_<hash>`, citations are
  `ev_<hash>`, and `snapshotId` plus per-item `blobHash` detect a stale
  packet after the file changes.

```rust
use cortex_context::{
    compile_context, ContextRequest, EvidenceItem, EvidencePriority, EvidenceState,
};

let request = ContextRequest {
    items: vec![
        EvidenceItem::new(
            "TASK",
            "request:user",
            "Rename the export helper without breaking callers.",
            EvidencePriority::Critical,
            EvidenceState::Verified,
        ),
        EvidenceItem::new(
            "SRC-1",
            "src/export.rs:120",
            "fn render_frontmatter(..) { /* ... */ }",
            EvidencePriority::High,
            EvidenceState::Verified,
        ),
    ],
    max_tokens: 4_000,
    deduplicate: true,
};

let packet = compile_context(&request).expect("the budget fits both items");
assert_eq!(packet.included_ids, ["TASK", "SRC-1"]);
assert!(!packet.requires_upstream);
assert_eq!(packet.deduplicated_lines, 0, "nothing overlapped here");
```

## Token budgets

`compile_context` uses a conservative counter so Cyrillic, CJK, and
punctuation cannot sneak a packet past the remaining context window.
`estimate_tokens` and `CharDiv4Counter` stay as the comparative
four-character unit for benches. A `TokenBreakdown` splits
`budgetOmittedTokens` from `dedupSavedTokens`; those are different
kinds of economy and must not be added together as "saved".

Vendor tokenizers implement [`TokenCounter`]. Until one is wired for
the active model, runtime compile keeps the conservative fallback.

## Ranking

The [`ranking`] module holds pure, deterministic retrieval primitives with
pinned parameters: cosine similarity, BM25, reciprocal-rank fusion, and a
rank-space structural graph boost. They produce the optional `relevance`
scores above. Nothing here calls a model — embedding vectors come from the
caller — and relevance only reorders items *within* a priority band, so
policy always dominates semantics.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
