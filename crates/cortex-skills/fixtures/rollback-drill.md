---
name: Rollback Drill
description: Practice the rollback until it is boring.
when-to-use: Use before a risky deploy or after a rollback path changes.
license: MIT
version: "2.0"
audience: engineers
---
# Rollback Drill

Practice the rollback until it is boring.

## Script

1. Write the rollback as numbered operator steps with expected signals.
2. Identify the latest artifact or backup the rollback needs. [kind: evidence_gate]

## Drill

3. Execute the rollback in staging against a realistic failure. [depends: 2]
4. Time the drill and note every missing permission or signal. [depends: 3]
5. Fix the gaps before the production window. [kind: quality_gate] [depends: 4]

## Ready

6. Escalate if the drill cannot complete because access or backups are missing. [kind: upstream_agent] [depends: 5]
7. Attach the drill record to the change that needs it. [kind: terminal] [depends: 5]

```text
An untested rollback is fan fiction.
```

- [ ] Drill completed in staging.
- [ ] Gaps found in the drill are closed.
