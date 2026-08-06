---
name: Dependency Audit
description: Decide whether a dependency's risk is acceptable for this repository.
when-to-use: Use when adding a crate or package, or when an advisory lands.
license: MIT
version: "2.0"
audience: engineers
---
# Dependency Audit

Decide whether a dependency's risk is acceptable for this repository.

## Inventory

1. List the new dependency and its recursive runtime surface.
2. Note license, maintenance signal, and advisory status. [kind: evidence_gate]

## Decide

3. Accept, replace, or vendor with an explicit reason. [depends: 2]
4. Escalate critical advisories with no patch. [kind: upstream_agent] [depends: 2]
5. Pin versions where floating would silently expand risk. [depends: 3]

## Record

6. Update the lockfile and any allow-list in the same change. [kind: terminal] [depends: 3]

```text
A dependency is code you chose not to write and still own.
```

- [ ] License and advisories were checked.
- [ ] The decision reason is recorded.
