---
name: Accessibility Audit
description: Check the interaction path with keyboard, contrast, and names before shipping UI.
when-to-use: Use when a user-facing flow changes layout, focus, or copy.
license: MIT
version: "2.0"
audience: engineers
---
# Accessibility Audit

Check the interaction path with keyboard, contrast, and names before shipping UI.

## Path

1. Walk the changed flow with keyboard only once.
2. Confirm every interactive control has an accessible name. [kind: evidence_gate]

## Perception

3. Check contrast and motion against the project's stated bar. [depends: 2]
4. Fix blockers before visual polish. [depends: 3]
5. Escalate pattern-level debt that this change cannot absorb. [kind: upstream_agent] [depends: 3]

## Lock

6. Add or update an automated a11y check where the failure was mechanical. [kind: test_gate] [depends: 4]
7. Record remaining known gaps with owners. [kind: terminal] [depends: 5]

```text
If it only works with a mouse, it is unfinished.
```

- [ ] Keyboard path completed without traps.
- [ ] Blockers are fixed or explicitly owned.
