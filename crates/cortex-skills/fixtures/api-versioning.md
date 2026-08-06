---
name: API Versioning
description: Change a published API without surprising an existing consumer.
when-to-use: Use when a wire or library contract already has callers outside this change.
license: MIT
version: "2.0"
audience: engineers
---
# API Versioning

Change a published API without surprising an existing consumer.

## Inventory

1. List every consumer the current version still serves. [kind: weavatrix]
2. Separate additive changes from breaking ones. [depends: 1]
3. Stop if the inventory cannot be completed. [kind: upstream_agent] [depends: 1]

## Publish

4. Ship the additive path under a new version or optional field first. [depends: 2]
5. Keep the old shape until every inventoried consumer has moved. [kind: evidence_gate] [depends: 4]
6. Document the removal date beside the new contract. [depends: 5]

## Retire

7. Remove the old shape only after the documented date and a final search. [kind: terminal] [depends: 6]

```text
Silence from a consumer is not consent to break them.
```

- [ ] Every known consumer has a migration owner.
- [ ] The removal date is published where callers will see it.
