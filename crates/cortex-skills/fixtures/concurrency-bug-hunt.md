---
name: Concurrency Bug Hunt
description: Turn a race into a reliable failure before changing synchronization.
when-to-use: Use when symptoms smell like ordering, locking, or shared mutable state.
license: MIT
version: "2.0"
audience: engineers
---
# Concurrency Bug Hunt

Turn a race into a reliable failure before changing synchronization.

## Stabilize

1. Capture a reproduction that fails more often than it passes. [kind: evidence_gate]
2. State the shared state and the conflicting operations. [depends: 1]

## Prove

3. Add a stress or thread-sanitizer style test that fails on the race. [kind: test_gate] [depends: 2]
4. Avoid sleep as the only synchronization in the test. [depends: 3]

## Fix

5. Apply the smallest correct synchronization or redesign. [depends: 4]
6. Re-run the stress test until the failure rate collapses. [kind: quality_gate] [depends: 5]
7. Escalate when the race needs an architectural ownership change. [kind: upstream_agent] [depends: 6]
8. Record the invariant the lock or queue now protects. [kind: terminal] [depends: 6]

```text
If you cannot make it fail, you cannot know you fixed it.
```

- [ ] Stress test fails without the fix.
- [ ] The protected invariant is written down.
