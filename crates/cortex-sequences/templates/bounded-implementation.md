---
name: Bounded Implementation
description: Implement an approved change in small test-led slices with evidence gates.
version: "1.0.0"
mechanics: executing-plans, test-driven-development
---
# Bounded Implementation

Use this sequence only after scope and acceptance criteria are explicit.

## Admit

1. Check repository state, approved scope, authority, and the next plan slice. [kind: deterministic]
2. Gather the source and dependent evidence needed for that slice. [kind: weavatrix] [depends: 1]
3. Refuse mutation when evidence, authority, or the plan boundary is incomplete. [kind: evidence_gate] [depends: 2]

## Change

4. Write one behavior test and confirm it fails for the intended missing behavior. [kind: test_gate] [depends: 3]
5. Hand the minimal code change to the upstream coding agent. [kind: upstream_agent] [depends: 4]
6. Run the focused test and the smallest relevant regression set. [kind: test_gate] [depends: 5]
7. Allow one bounded correction when verification identifies a concrete defect. [kind: retry] [depends: 6]
8. Stop and hand off when the correction still fails or scope expands. [kind: handoff] [depends: 7]

## Close slice

9. Verify the diff cites evidence and contains no unrelated mutation. [kind: review_gate] [depends: 6]
10. Record test output and completion criteria for the slice. [kind: evidence_gate] [depends: 9]
11. Finish the slice or advance to the next approved slice. [kind: terminal] [depends: 10]
