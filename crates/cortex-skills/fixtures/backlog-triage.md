---
name: Backlog Triage
description: Rank work by evidence of user harm, not by who asked loudest.
when-to-use: Use when the queue is longer than the next planning window.
license: MIT
version: "2.0"
audience: engineers
---
# Backlog Triage

Rank work by evidence of user harm, not by who asked loudest.

## Sort

1. Cluster items by the user-visible failure or opportunity they name.
2. Drop or park items with no evidence and no deadline. [kind: evidence_gate]

## Rank

3. Order the rest by harm reduced per unit of effort. [depends: 2]
4. Cap the committed set to what fits the window. [depends: 3]
5. Escalate conflicting priorities to a named decision owner. [kind: human_gate] [depends: 4]

## Commit

6. Hand conflicting product-versus-reliability calls to the decision owner. [kind: upstream_agent] [depends: 5]
7. Publish the ranked slice and the explicit deferrals. [kind: terminal] [depends: 5]

```text
An infinite backlog is a refusal to choose.
```

- [ ] Every committed item has a harm statement.
- [ ] Deferred items are visible, not deleted quietly.
