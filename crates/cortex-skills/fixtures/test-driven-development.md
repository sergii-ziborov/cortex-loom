---
name: Test-Driven Development
description: Grow behavior through failing tests, minimal code, and refactoring.
license: MIT
version: "1.0"
audience: engineers
---
# Test-Driven Development

Write the test before the code it justifies. The failing test is the
specification; the passing test is the receipt.

## Red

1. Write one small test that states the next behavior.
2. Run it and watch it fail for the stated reason.

```text
A test that fails for the wrong reason proves nothing.
```

## Green

3. Write the least code that makes the test pass. [depends: 2]
4. Run the whole suite, not just the new test. [depends: 3]

## Refactor

5. Remove duplication introduced while going green. [depends: 4]
- [ ] Keep every test passing after each refactoring move.
- [ ] Commit when the suite is green and the diff is coherent.

## Cadence

- Prefer many small cycles over one large cycle.
- Never write production code without a failing test demanding it.
