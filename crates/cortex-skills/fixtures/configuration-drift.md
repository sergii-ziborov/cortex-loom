---
name: Configuration Drift
description: Reconcile running config with the declared source of truth.
when-to-use: Use when environments disagree or a live value cannot be explained from git.
license: MIT
version: "2.0"
audience: engineers
---
# Configuration Drift

Reconcile running config with the declared source of truth.

## Compare

1. Export the live config and diff it against the declared source. [kind: evidence_gate]
2. Label each delta as intentional, accidental, or unknown. [depends: 1]

## Reconcile

3. Codify intentional deltas or delete them from live. [depends: 2]
4. Fix accidental deltas in the source that owns them. [depends: 3]
5. Escalate unknown deltas; do not guess. [kind: upstream_agent] [depends: 2]

## Prevent

6. Add a check that fails when live and source diverge again. [kind: test_gate] [depends: 4]
7. Document the source of truth in one place. [kind: terminal] [depends: 6]

```text
A snowflake environment is an outage scheduled for later.
```

- [ ] Every delta is classified.
- [ ] Drift detection runs without a human remembering.
