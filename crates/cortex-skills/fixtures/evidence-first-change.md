---
name: Evidence-First Change
description: Gather citable evidence before proposing an edit, and cite it in the diff.
when-to-use: Use when an edit will have to be defended later with citations.
license: MIT
version: "2.0"
audience: engineers
---
# Evidence-First Change

An edit is a claim about the system. Collect the evidence that makes the claim
checkable before writing the edit, not after someone questions it.

## Ground

1. State the change as one sentence a reviewer could disagree with.
2. Name the invariant the change must not break.
3. Collect the smallest evidence set that decides the question. [kind: weavatrix] [depends: 1]

```text
If you cannot name what would prove you wrong, you are not ready to edit.
```

## Bound

4. Mark each piece of evidence verified, unverified, or contradictory. [kind: evidence_gate] [depends: 3]
5. Escalate instead of guessing when anything is contradictory. [kind: upstream_agent] [depends: 4]
6. Record what you deliberately left out and why. [depends: 4]

## Change

7. Draft the edit against the evidence, not against memory. [kind: local_model] [depends: 6]
8. Check the draft against the invariant named in step 2. [kind: quality_gate] [depends: 7]
9. Cite the deciding evidence in the commit message or the review. [depends: 8]
10. Hand an unverified or high-risk change to the upstream agent. [kind: upstream_agent] [depends: 8]
11. Land the change once its claims are cited and checked. [kind: terminal] [depends: 9]

- Evidence gathered for one attempt does not carry into the next.
- Unverified evidence keeps the change under upstream review.
- [ ] Every claim in the description traces to something cited.
- [ ] Nothing critical was dropped to fit a summary.
