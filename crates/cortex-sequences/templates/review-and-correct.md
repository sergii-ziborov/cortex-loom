---
name: Review and Correct
description: Request grounded review, evaluate findings, and correct only verified defects.
version: "1.0.0"
mechanics: requesting-code-review, receiving-code-review
---
# Review and Correct

Use this sequence when a bounded change or plan is ready for independent scrutiny.

## Prepare review

1. State the intended behavior, scope, acceptance criteria, and verification already run. [kind: deterministic]
2. Gather the exact diff, contracts, dependents, and cited source evidence. [kind: weavatrix] [depends: 1]
3. Confirm the review packet is complete enough to reproduce every claim. [kind: evidence_gate] [depends: 2]
4. Hand the packet to an upstream reviewer with explicit questions. [kind: upstream_agent] [depends: 3]

## Evaluate findings

5. Classify each finding as verified, unclear, incorrect, or out of scope. [kind: review_gate] [depends: 4]
6. Reproduce actionable findings before changing code. [kind: test_gate] [depends: 5]
7. Ask for clarification when a finding has no reproducible contract. [kind: handoff] [depends: 5]

## Correct

8. Hand only verified corrections to the upstream coding agent. [kind: upstream_agent] [depends: 6]
9. Re-run the affected tests and review the resulting diff. [kind: evidence_gate] [depends: 8]
10. Hand unresolved findings or remaining high risk to the upstream owner. [kind: handoff] [depends: 9]
11. Finish with resolved findings, rejected findings, evidence, and remaining risk. [kind: terminal] [depends: 9]
