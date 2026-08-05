---
name: Dependency Upgrade
description: Move one dependency at a time and prove the behaviour did not move with it.
when-to-use: Use when adopting a new version of a third-party dependency.
license: MIT
version: "2.0"
audience: engineers
---
# Dependency Upgrade

An upgrade is a behaviour change authored by someone else. Treat it as a diff
you did not write and cannot fully read.

## Prepare

1. Upgrade one dependency per change, never a batch.
2. Read the changelog for the range you are crossing, not just the newest entry.
3. Write down every behaviour the release notes call out that this codebase relies on. [kind: evidence_gate] [depends: 2]

## Move

4. Take the smallest version step that clears the reason for upgrading. [kind: branch] [depends: 1]
5. Let the compiler and the test suite find the mechanical breaks first. [kind: test_gate] [depends: 4]
6. Fix behaviour changes that no test noticed, using the notes from the reading pass. [depends: 3]
7. Revert rather than patch when the upgrade cannot be explained. [kind: upstream_agent] [depends: 6]

```text
A green suite after an upgrade proves the suite still passes, not that the
behaviour is unchanged.
```

## Verify

8. Re-measure whatever the dependency is on the critical path for. [kind: test_gate] [depends: 6]
9. Pin the new version explicitly and record why the range was chosen. [depends: 8]
10. Take the licence and supply-chain review before the version lands. [kind: human_gate] [depends: 9]
11. Land the upgrade with its reason recorded beside the pin. [kind: terminal] [depends: 10]

- A transitive upgrade is still an upgrade; check what moved underneath.
- [ ] The lockfile change was reviewed, not skipped.
- [ ] Nothing newly added arrived under unreviewed terms.
