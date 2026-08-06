---
name: Flaky Test Quarantine
description: Isolate an unreliable test without laundering the failure into silence.
when-to-use: Use when a test fails intermittently and is blocking honest signal.
license: MIT
version: "2.0"
audience: engineers
---
# Flaky Test Quarantine

Isolate an unreliable test without laundering the failure into silence.

## Capture

1. Record the flake rate, the last green SHA, and the owning suite.
2. Reproduce once outside CI or say that you could not. [kind: evidence_gate]

## Quarantine

3. Move the test behind an explicit quarantine gate, not a skip comment. [depends: 2]
4. Open a tracked item with a deadline to delete or fix it. [depends: 3]
5. Refuse new features that expand the quarantined surface. [kind: quality_gate] [depends: 4]

## Resolve

6. Either fix with a deterministic test or delete the assertion. [kind: test_gate] [depends: 4]
7. Escalate when the flake survives three honest reproduction attempts. [kind: upstream_agent] [depends: 6]
8. Remove the quarantine entry in the same change. [kind: terminal] [depends: 6]

```text
A skipped test is a forgotten promise.
```

- [ ] The quarantine list has an owner and a date.
- [ ] CI still fails on non-quarantined regressions.
