---
name: Discover and Plan
description: Turn an unclear engineering task into a bounded evidence-backed plan.
version: "1.0.0"
mechanics: brainstorming, writing-plans
---
# Discover and Plan

Use this sequence when the desired outcome or implementation boundary still needs discovery.

## Frame

1. Restate the outcome, constraints, exclusions, and unresolved choices. [kind: deterministic]
2. Offer the smallest distinct approaches with their real trade-offs. [kind: agent_task] [depends: 1]
3. Record the selected direction before implementation planning. [kind: human_gate] [depends: 2]

## Ground

4. Ask Weavatrix for revision-bound source, contract, and dependent evidence. [kind: weavatrix] [depends: 3]
5. Check that every planned decision has cited evidence and no contradiction. [kind: evidence_gate] [depends: 4]
6. Run one bounded source follow-up when a named decision remains thin. [kind: weavatrix] [depends: 5]
7. Escalate ambiguity, contradiction, or high-risk scope to the upstream coding agent. [kind: upstream_agent] [depends: 5]

## Plan

8. Write ordered implementation slices with files, tests, risks, budgets, and acceptance criteria. [kind: agent_task] [depends: 6]
9. Review the plan against the chosen direction and cited evidence. [kind: review_gate] [depends: 8]
10. Hand an unresolved review decision to the upstream owner. [kind: handoff] [depends: 9]
11. Finish with an executable plan or an explicit upstream handoff. [kind: terminal] [depends: 9]
