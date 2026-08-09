---
name: Root Cause Debugging
description: Diagnose reproducible failures before proposing or applying a correction.
version: "1.0.0"
mechanics: systematic-debugging
---
# Root Cause Debugging

Use this sequence for test failures, build errors, regressions, hangs, or unexpected behavior.

## Reproduce

1. Capture the exact symptom, command, inputs, environment, and complete error. [kind: deterministic]
2. Reproduce the failure without changing production code. [kind: test_gate] [depends: 1]
3. Ask Weavatrix for callers, data flow, configuration, and recent boundary evidence. [kind: weavatrix] [depends: 2]

## Explain

4. Trace the bad state backward to the first violated invariant. [kind: agent_task] [depends: 3]
5. Compare the failing path with a working path and state one falsifiable hypothesis. [kind: evidence_gate] [depends: 4]
6. Run one bounded diagnostic experiment that changes one variable. [kind: deterministic] [depends: 5]
7. Escalate when the evidence contradicts the hypothesis or the fault is high risk. [kind: upstream_agent] [depends: 6]

## Correct

8. Add a regression test that fails on the confirmed root cause. [kind: test_gate] [depends: 6]
9. Hand the smallest root-cause correction to the upstream coding agent. [kind: upstream_agent] [depends: 8]
10. Verify the symptom, regression test, and surrounding gates. [kind: evidence_gate] [depends: 9]
11. Hand an unproven correction or residual high risk to the upstream owner. [kind: handoff] [depends: 10]
12. Finish with the cause, evidence, correction, and residual risk. [kind: terminal] [depends: 10]
