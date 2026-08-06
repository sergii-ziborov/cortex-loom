---
name: Observability First
description: Instrument the question you will ask at 2am before shipping the path.
when-to-use: Use when a new code path will be hard to inspect once it is live.
license: MIT
version: "2.0"
audience: engineers
---
# Observability First

Instrument the question you will ask at 2am before shipping the path.

## Questions

1. Write the three questions on-call will ask about this path.
2. Name the signal that answers each question. [depends: 1]

## Signals

3. Emit logs, metrics, or traces that carry those signals. [depends: 2]
4. Confirm the signals appear in a local or staging run. [kind: evidence_gate] [depends: 3]
5. Add an alert only where a human action exists. [kind: quality_gate] [depends: 4]

## Ship

6. Escalate when the path is live but the named signals are dark. [kind: upstream_agent] [depends: 4]
7. Land the instrumentation with the feature, not after the page. [kind: terminal] [depends: 5]

```text
An unobservable feature is an unpaid incident.
```

- [ ] Each on-call question has a named signal.
- [ ] Alerts map to a runbook step.
