---
name: Data Migration
description: Move stored data with a reversible plan and a measured backfill.
when-to-use: Use when rows, blobs, or indexes must change shape under live traffic.
license: MIT
version: "2.0"
audience: engineers
---
# Data Migration

Move stored data with a reversible plan and a measured backfill.

## Design

1. State the before and after schemas as data, not prose.
2. Choose expand-contract over in-place rewrite when readers still exist. [depends: 1]
3. Write the down migration or the restore path first. [kind: evidence_gate] [depends: 2]

## Backfill

4. Backfill in batches with a progress meter and a kill switch. [depends: 3]
5. Compare old and new reads on a sample until they agree. [kind: test_gate] [depends: 4]
6. Escalate if the sample diverges after a retry. [kind: upstream_agent] [depends: 5]

## Contract

7. Remove the old columns only after dual-read is idle. [kind: terminal] [depends: 5]

```text
A migration without a down path is a one-way door.
```

- [ ] Sample parity was measured, not assumed.
- [ ] The kill switch was tested once.
