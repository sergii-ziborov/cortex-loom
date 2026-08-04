---
name: Grounded Review
description: Review changes against evidence, invariants, and blast radius.
license: MIT
reviewers: "one required"
---
# Grounded Review

A review is a verification, not a vibe. Cite what you checked.

## Understand

1. Read the stated intent and the linked evidence first.
2. Map the blast radius across callers, dependents, and persisted formats.

## Verify

3. Check every invariant the change touches. [depends: 2]
4. Run the gates locally when the change alters behavior. [depends: 3]
- Look for missing tests before commenting on style.

## Decide

5. Approve only with an explicit reason and cited evidence. [depends: 4]
- [ ] Rejections name the failing invariant or missing proof.
- [ ] Requested changes are actionable, not aesthetic.

Ссылки и Unicode-текст — часть давления: проверка UTF-8 в лейблах — ✓.
