---
name: Systematic Debugging
description: Reproduce, bisect, and prove the cause before changing code.
when-to-use: Use when something fails and the cause is not yet known.
license: MIT
version: "2.0"
severity: high
---
# Systematic Debugging

Guessing is the slowest debugger. Every move below either shrinks the search
space or proves a hypothesis.

## Reproduce

1. Capture the exact failing command, input, and environment.
2. Reduce the reproduction until removing anything makes it pass.
3. Confirm the reproduction is reliable before reasoning from it. [kind: evidence_gate] [depends: 2]

## Localize

4. State one falsifiable hypothesis about the cause. [depends: 3]
5. Bisect history, configuration, or input, whichever is cheapest to halve. [depends: 4]
6. Confirm the culprit by flipping only it and watching the failure follow. [kind: evidence_gate] [depends: 5]
7. Escalate when three hypotheses in a row survive their tests. [kind: upstream_agent] [depends: 4]

## Fix and lock

8. Write a test that fails on the culprit before fixing it. [kind: test_gate] [depends: 6]
9. Apply the smallest fix that makes the new test pass. [depends: 8]
10. Record the root cause where the next reader will look. [kind: terminal] [depends: 9]

```text
If the fix works and you do not know why, the bug is still alive.
```

- A reproduction you cannot rerun is an anecdote.
- [ ] The new test fails without the fix.
- [x] The root cause is written down, not just remembered.
