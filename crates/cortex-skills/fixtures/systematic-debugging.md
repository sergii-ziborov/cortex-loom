---
name: Systematic Debugging
description: Reproduce, bisect, and prove the cause before changing code.
license: MIT
severity: high
---
# Systematic Debugging

Guessing is the slowest debugger. Every move below either shrinks the search
space or proves a hypothesis.

## Reproduce

1. Capture the exact failing command, input, and environment.
2. Reduce the reproduction until removing anything makes it pass.

## Localize

3. State one falsifiable hypothesis about the cause. [depends: 2]
4. Bisect history, configuration, or input, whichever is cheapest to halve.
5. Confirm the culprit by flipping only it and watching the failure follow. [depends: 3, 4]

## Fix and lock

6. Write a test that fails on the culprit before fixing it. [depends: 5]
- [ ] Apply the smallest fix that makes the new test pass.
- [x] Record the root cause where the next reader will look.

```text
If the fix works and you do not know why, the bug is still alive.
```
