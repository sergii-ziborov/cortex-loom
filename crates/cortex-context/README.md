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

```rust
use cortex_context::{
    compile_context, ContextRequest, EvidenceItem, EvidencePriority, EvidenceState,
};

let request = ContextRequest {
    items: vec![
        EvidenceItem {
            id: "TASK".to_owned(),
            source: "request:user".to_owned(),
            content: "Rename the export helper without breaking callers.".to_owned(),
            priority: EvidencePriority::Critical,
            state: EvidenceState::Verified,
            relevance: None,
        },
        EvidenceItem {
            id: "SRC-1".to_owned(),
            source: "src/export.rs:120".to_owned(),
            content: "fn render_frontmatter(..) { /* ... */ }".to_owned(),
            priority: EvidencePriority::High,
            state: EvidenceState::Verified,
            relevance: None,
        },
    ],
    max_tokens: 4_000,
};

let packet = compile_context(&request).expect("the budget fits both items");
assert_eq!(packet.included_ids, ["TASK", "SRC-1"]);
assert!(!packet.requires_upstream);
```

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
