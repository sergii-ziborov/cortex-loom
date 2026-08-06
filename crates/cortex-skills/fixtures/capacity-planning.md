---
name: Capacity Planning
description: Size the next bottleneck before users find it.
when-to-use: Use when load is rising or a launch will multiply traffic.
license: MIT
version: "2.0"
audience: engineers
---
# Capacity Planning

Size the next bottleneck before users find it.

## Model

1. State the load unit and the resource that saturates first.
2. Project demand across the planning horizon with assumptions listed. [depends: 1]

## Test

3. Validate the model with a load test or a production extrapolation. [kind: evidence_gate] [depends: 2]
4. Name the scaling action and its lead time. [depends: 3]
5. Escalate if lead time exceeds the demand ramp. [kind: upstream_agent] [depends: 4]

## Provision

6. Schedule the scaling action with an owner and a date. [kind: terminal] [depends: 4]

```text
Hope is not a capacity plan.
```

- [ ] Assumptions are listed.
- [ ] Scaling action has a date.
