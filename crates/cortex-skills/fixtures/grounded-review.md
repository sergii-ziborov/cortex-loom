---
name: Grounded Review
description: Review changes against evidence, invariants, and blast radius.
when-to-use: Use when reviewing someone else's change, or your own before sending it.
license: MIT
version: "2.0"
reviewers: "one required"
---
# Grounded Review

A review is a verification, not a vibe. Cite what you checked.

## Understand

1. Read the stated intent and the linked evidence first.
2. Map the blast radius across callers, dependents, and persisted formats. [kind: weavatrix] [depends: 1]

## Verify

3. Check every invariant the change touches. [kind: evidence_gate] [depends: 2]
4. Run the gates locally when the change alters behavior. [kind: test_gate] [depends: 3]
5. Escalate a change whose risk you cannot bound from the evidence. [kind: upstream_agent] [depends: 3]

## Decide

6. Approve or reject with an explicit reason and cited evidence. [kind: review_gate] [depends: 4]
7. Close the review only once the decision is recorded. [kind: terminal] [depends: 6]

- Look for missing tests before commenting on style.
- [ ] Rejections name the failing invariant or the missing proof.
- [ ] Requested changes are actionable, not aesthetic.

Ссылки и Unicode-текст — часть давления: проверка UTF-8 в лейблах — ✓.
