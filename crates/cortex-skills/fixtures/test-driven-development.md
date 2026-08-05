---
name: Test-Driven Development
description: Grow behavior through failing tests, minimal code, and refactoring.
when-to-use: Use when adding or changing behaviour that a test could specify.
license: MIT
version: "2.0"
audience: engineers
---
# Test-Driven Development

Write the test before the code it justifies. The failing test is the
specification; the passing test is the receipt.

## Red

1. Write one small test that states the next behavior.
2. Run it and watch it fail for the stated reason. [kind: test_gate] [depends: 1]

```text
A test that fails for the wrong reason proves nothing. Read the message
before you read the code.
```

## Green

3. Write the least code that makes the test pass. [depends: 2]
4. Run the whole suite, not just the new test. [kind: test_gate] [depends: 3]
5. Hand the design question upstream when going green needs a decision the test cannot make. [kind: upstream_agent] [depends: 3]

## Refactor

6. Remove duplication introduced while going green. [depends: 4]
7. Re-run the suite after each refactoring move. [kind: test_gate] [depends: 6]
8. Commit when the suite is green and the diff tells one story. [kind: terminal] [depends: 7]

## Cadence

- Prefer many small cycles over one large cycle.
- Never write production code without a failing test demanding it.
- [ ] Every commit on this path left the suite green.
- [ ] No test was weakened to make the code pass.
