---
name: Feature Flag Rollout
description: Ship dark, open gradually, and keep the flag from becoming permanent debt.
when-to-use: Use when a change must reach users in stages or be disabled remotely.
license: MIT
version: "2.0"
audience: engineers
---
# Feature Flag Rollout

Ship dark, open gradually, and keep the flag from becoming permanent debt.

## Dark

1. Ship the code behind a default-off flag with a named owner.
2. Prove both flag states in automated tests. [kind: test_gate]

## Open

3. Raise exposure in steps tied to an error-budget check. [kind: quality_gate] [depends: 2]
4. Halt and roll back the flag on budget breach. [depends: 3]

## Clean

5. Delete the flag and the losing branch once exposure is total. [kind: terminal] [depends: 4]
6. Refuse a second flag that depends on this one still existing. [kind: quality_gate] [depends: 5]

```text
A flag that outlives its rollout is a second configuration system.
```

- [ ] Both flag states are tested.
- [ ] The deletion date is on the flag record.
