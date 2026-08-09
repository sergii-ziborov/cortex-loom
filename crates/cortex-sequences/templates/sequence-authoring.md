---
name: Sequence Authoring
description: Create or revise an editable Cortex sequence from observed workflow failures.
version: "1.0.0"
mechanics: writing-skills
---
# Sequence Authoring

Use this sequence when a reusable workflow needs a new template or a measured correction.

## Observe

1. Define the trigger, desired behavior, and concrete failure the sequence must prevent. [kind: deterministic]
2. Run a baseline scenario without the proposed guidance and record the failure. [kind: test_gate] [depends: 1]
3. Gather existing graph, run, evidence, and competing-template behavior. [kind: weavatrix] [depends: 2]
4. Confirm the failure is judgment work rather than a constraint better enforced in code. [kind: evidence_gate] [depends: 3]

## Author

5. Draft the smallest typed steps, gates, recovery path, budgets, and upstream fallback. [kind: agent_task] [depends: 4]
6. Keep only the active step eligible for methodology context. [kind: quality_gate] [depends: 5]
7. Escalate high-risk executor choices and mutation authority to the upstream agent. [kind: upstream_agent] [depends: 6]

## Pressure test

8. Run structural lint, round-trip, baseline, pressure, and counter-example scenarios. [kind: test_gate] [depends: 7]
9. Allow one bounded wording or graph correction tied to an observed failure. [kind: retry] [depends: 8]
10. Hand unresolved safety or activation ambiguity to an upstream reviewer. [kind: handoff] [depends: 9]
11. Finish with a versioned editable template and measured behavior change. [kind: terminal] [depends: 8]
