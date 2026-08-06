---
name: Documentation Sync
description: Update the docs that would lie after this change lands.
when-to-use: Use when behaviour, contracts, or operator steps change.
license: MIT
version: "2.0"
audience: engineers
---
# Documentation Sync

Update the docs that would lie after this change lands.

## Find

1. Search for every doc that names the old behaviour. [kind: weavatrix]
2. Include runbooks and examples, not only the README. [depends: 1]

## Align

3. Edit each hit so a reader following it gets the new behaviour. [depends: 2]
4. Delete docs that now describe a path that no longer exists. [depends: 3]
5. Link the commit or PR from any external mirror that must catch up. [kind: evidence_gate] [depends: 3]

## Check

6. Have a second person follow the doc once cold. [kind: human_gate] [depends: 5]
7. Escalate when the product owner will not accept the doc change with the code. [kind: upstream_agent] [depends: 6]
8. Land docs in the same change as the behaviour. [kind: terminal] [depends: 6]

```text
Docs that disagree with the code train people to ignore both.
```

- [ ] Every removed path lost its documentation.
- [ ] A cold reader completed the happy path.
