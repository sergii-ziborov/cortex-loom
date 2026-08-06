---
name: Migration Cutover
description: Plan and execute a cutover that can be rolled back in one step.
when-to-use: Use when traffic or data must move from an old path to a new one.
license: MIT
version: "2.0"
audience: engineers
---
# Migration Cutover

Plan and execute a cutover that can be rolled back in one step.

## Freeze

1. Name the exact cutover window and the rollback owner.
2. Freeze writes on the old path or make them dual-write. [kind: evidence_gate]
3. Prove the new path serves a canary load before the window. [kind: test_gate] [depends: 2]

## Switch

4. Flip one control — route, flag, or DNS — not a bundle of edits. [depends: 3]
5. Watch the error budget and the rollback signal for the full window. [kind: quality_gate] [depends: 4]
6. Escalate if any invariant the freeze named is breached. [kind: upstream_agent] [depends: 5]

## Settle

7. Delete or fence the old path only after the window passes clean. [kind: terminal] [depends: 5]

```text
A cutover with two switches is two cutovers pretending to be one.
```

- [ ] Rollback was rehearsed against the same signal used in production.
- [ ] The old path cannot accept new writes after settle.
