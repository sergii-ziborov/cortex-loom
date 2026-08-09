---
name: Interface Contract Change
description: Change a published contract without breaking the consumers already on it.
when-to-use: Use when altering an API, schema, or format a consumer already depends on.
license: MIT
version: "2.0"
audience: engineers
context-intent: api_contract
source-followup: true
skip-change-plan: true
---
# Interface Contract Change

A contract is whatever a consumer already depends on, which is usually more
than the documentation admits: field order, error text, and absent keys all
count.

## Establish

1. Write down the current contract from the consumer side, not the implementation side.
2. Identify which consumers you can change and which you cannot. [kind: weavatrix] [depends: 1]
3. Decide whether the change is additive, restrictive, or a rename. [kind: branch] [depends: 1]

```text
Additive is cheap, restrictive is expensive, and a rename is both at once.
```

## Sequence

4. Add the new shape before removing the old one. [depends: 3]
5. Accept both shapes for as long as an unreachable consumer might still send the old one. [depends: 4]
6. Prove the old consumer still works against the new implementation. [kind: test_gate] [depends: 4]
7. Remove the old shape only after traffic on it reaches zero and stays there. [kind: evidence_gate] [depends: 5]

## Prove

8. Take an explicit approval before a restrictive change or a rename ships. [kind: review_gate] [depends: 6]
9. Escalate when a consumer you cannot change would break. [kind: upstream_agent] [depends: 2]
10. Record the removal date and the signal that will authorize it. [depends: 7]
11. Publish the change with its deprecation path documented. [kind: terminal] [depends: 10]

- An error message that a consumer parses is part of the contract.
- Silence from a consumer is not evidence they migrated.
- [ ] The schema version moved if the shape did.
- [ ] The deprecation is discoverable from the old path itself.
