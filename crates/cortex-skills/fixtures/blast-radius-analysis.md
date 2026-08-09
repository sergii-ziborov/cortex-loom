---
name: Blast Radius Analysis
description: Map everything a change can reach before deciding how to make it.
when-to-use: Use before a change whose true scope is still unknown.
license: MIT
version: "2.0"
audience: engineers
context-intent: blast_radius
source-followup: true
skip-change-plan: true
---
# Blast Radius Analysis

Scope is discovered, not assumed. The cost of a change is the closure of what
it touches, and that closure is usually larger than the diff.

## Map

1. List the direct callers of every symbol the change alters. [kind: weavatrix]
2. Follow the callers outward until the boundary stops moving. [kind: weavatrix] [depends: 1]
3. Include the boundaries a compiler cannot see: serialized formats, database columns, wire protocols, and file names. [depends: 2]
4. Stop and say so when the closure cannot be enumerated. [kind: upstream_agent] [depends: 2]

## Weigh

5. Separate what breaks at compile time from what breaks at run time. [depends: 3]
6. Separate what breaks for us from what breaks for a consumer already on the old version. [depends: 5]
7. Confirm the radius against the evidence rather than against intuition. [kind: evidence_gate] [depends: 6]

```text
A change that compiles everywhere and breaks one persisted document is the
expensive kind.
```

## Decide

8. Choose between one edit, a compatibility shim, or a versioned parallel path. [kind: branch] [depends: 7]
9. Write the rollback before the rollout when the radius crosses a boundary. [depends: 8]
10. Rehearse the rollback once, then record the chosen scope. [kind: terminal] [depends: 9]

- Dead code inside the radius is deleted, not migrated.
- A radius you cannot enumerate is a reason to shrink the change.
- [ ] Each boundary crossing has a named owner.
- [ ] The rollback was executed once, not just written.
