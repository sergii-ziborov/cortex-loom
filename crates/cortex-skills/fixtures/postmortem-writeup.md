---
name: Postmortem Writeup
description: Turn an incident into durable fixes without blaming individuals.
when-to-use: Use after a mitigated incident while memories and timelines are fresh.
license: MIT
version: "2.0"
audience: engineers
---
# Postmortem Writeup

Turn an incident into durable fixes without blaming individuals.

## Timeline

1. Build a UTC timeline from pages, deploys, and dashboards. [kind: evidence_gate]
2. State impact in user terms and duration. [depends: 1]

## Causes

3. Name root causes and contributing factors separately. [depends: 2]
4. Reject human error as a terminal cause; ask which system allowed it. [depends: 3]

## Actions

5. Assign remediations with owners and dates. [kind: human_gate] [depends: 4]
6. Escalate systemic causes that this team cannot fund alone. [kind: upstream_agent] [depends: 5]
7. Publish the writeup where the org will find it. [kind: terminal] [depends: 5]

```text
A postmortem without actions is a story.
```

- [ ] Timeline is evidence-backed.
- [ ] Every action has an owner and a date.
