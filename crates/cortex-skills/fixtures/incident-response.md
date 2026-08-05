---
name: Incident Response
description: Stop the bleeding, then find the cause, then prevent the class.
when-to-use: Use when production is broken right now.
license: MIT
version: "2.0"
severity: high
---
# Incident Response

Recovery and diagnosis compete for the same hour. Recovery wins, but only if
you preserve enough evidence for diagnosis to be possible afterwards.

## Stabilize

1. Say out loud what is broken, for whom, and since when.
2. Capture the evidence a mitigation will destroy: logs, metrics, a failing request, the running configuration. [kind: evidence_gate] [depends: 1]
3. Decide between mitigating, rolling back, and waiting. [kind: branch] [depends: 2]
4. Apply the smallest reversible action. [depends: 3]
5. Confirm recovery with the same signal that showed the failure. [kind: test_gate] [depends: 4]

```text
A mitigation you cannot undo is a second incident waiting for its turn.
```

## Diagnose

6. Build the timeline before the theory. [depends: 2]
7. State the cause as a falsifiable claim and test it against the captured evidence. [kind: evidence_gate] [depends: 6]
8. Escalate to the owning team when the cause crosses a boundary you do not control. [kind: handoff] [depends: 6]

## Prevent

9. Fix the cause, then ask what class of failure it belongs to. [depends: 7]
10. Add the detection that would have shortened this incident. [depends: 9]
11. Close the incident with an owner and a date for the follow-up. [kind: human_gate] [depends: 10]
12. Record the outcome where the next responder will look. [kind: terminal] [depends: 11]

- Blame the missing guardrail, never the person who tripped over its absence.
- Reopen the incident when the fix is only a mitigation.
- [ ] The timeline is written down while the details are still cheap to recall.
- [ ] The follow-up work has an owner and a date, not a wish.
