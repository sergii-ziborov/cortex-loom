---
name: Error Budget Review
description: Spend reliability budget on purpose, or stop the burn.
when-to-use: Use when SLOs exist and a change or incident consumes them.
license: MIT
version: "2.0"
audience: engineers
---
# Error Budget Review

Spend reliability budget on purpose, or stop the burn.

## Measure

1. Read the current error-budget burn for the affected SLO. [kind: evidence_gate]
2. Attribute the burn to deploy, dependency, or demand. [depends: 1]

## Act

3. If budget remains, proceed with explicit spend limits. [kind: branch] [depends: 2]
4. If budget is exhausted, freeze risky deploys and fix reliability. [kind: quality_gate] [depends: 2]
5. Escalate when product pressure demands burning a depleted budget. [kind: upstream_agent] [depends: 4]

## Learn

6. Escalate when leadership demands spend after a freeze. [kind: upstream_agent] [depends: 5]
7. Update the SLO or the remediation backlog from what burned. [kind: terminal] [depends: 5]

```text
An SLO without a budget policy is a vanity metric.
```

- [ ] Burn attribution is written.
- [ ] Freeze or spend was an explicit choice.
