---
name: Verification Before Completion
description: Require fresh, inspected evidence before claiming a task complete, committing it, releasing it, or reporting that a gate passed.
when-to-use: Use immediately before any completion, commit, release, deployment, or test-pass claim.
license: MIT
version: "1.0"
audience: engineers
---
# Verification Before Completion

A completion claim is an evidence claim. Produce the proof in the current
attempt before stating the outcome.

## Define the proof

1. Translate the acceptance criteria into observable checks.
2. List the commands or inspections that prove each check. [depends: 1]
3. Mark any criterion that cannot be verified locally. [kind: evidence_gate] [depends: 2]

## Verify fresh

4. Run every applicable check after the final change. [kind: test_gate] [depends: 3]
5. Inspect exit status, failures, skips, warnings, and tested scope. [kind: evidence_gate] [depends: 4]
6. Fix failures and restart verification from step 4. [kind: local_model] [depends: 5]
7. Escalate when required proof needs unavailable authority or external state. [kind: upstream_agent] [depends: 5]

## Make the claim

8. Compare the fresh evidence with every acceptance criterion. [kind: quality_gate] [depends: 5]
9. Report what passed, what was not tested, and the exact boundary of the claim. [kind: review_gate] [depends: 8]
10. Finish only when the evidence supports the wording of the claim. [kind: terminal] [depends: 9]

- A prior run is not fresh evidence after a code or configuration change.
- A command that was not run must not be described as passing.
- A partial suite proves only its named scope.
- [ ] Every completion sentence traces to evidence from this attempt.
- [ ] Skips and environmental limits are visible in the handoff.
