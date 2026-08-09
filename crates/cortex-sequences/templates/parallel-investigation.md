---
name: Parallel Investigation
description: Split independent read-only questions and synthesize them through one evidence gate.
version: "1.0.0"
mechanics: dispatching-parallel-agents, subagent-driven-development
---
# Parallel Investigation

Use this sequence only when two or more bounded questions have no shared mutable state.

## Partition

1. Prove the questions are independent and identify shared evidence they must not duplicate. [kind: deterministic]
2. Define one output contract, budget, and stop condition for each investigation. [kind: agent_task] [depends: 1]
3. Keep mutation, final judgment, and integration outside parallel lanes. [kind: quality_gate] [depends: 2]

## Investigate

4. Gather common revision-bound repository evidence once. [kind: weavatrix] [depends: 3]
5. Dispatch bounded read-only lanes through the upstream agent runtime. [kind: upstream_agent] [depends: 4]
6. Require each lane to return claims, citations, uncertainty, and no completion verdict. [kind: evidence_gate] [depends: 5]
7. Retry one failed lane only when its missing evidence is explicit and independent. [kind: retry] [depends: 6]
8. Hand coupled, contradictory, or mutating work back to one upstream owner. [kind: handoff] [depends: 7]

## Synthesize

9. Merge findings by evidence ID, preserving disagreements rather than voting. [kind: deterministic] [depends: 6]
10. Review the synthesis for missing facts, duplicate claims, and unsafe authority. [kind: review_gate] [depends: 9]
11. Finish with one bounded evidence packet and explicit unresolved questions. [kind: terminal] [depends: 10]
