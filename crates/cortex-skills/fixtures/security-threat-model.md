---
name: Security Threat Model
description: Name who can hurt the system before writing the next control.
when-to-use: Use before exposing a new trust boundary, secret, or privilege.
license: MIT
version: "2.0"
audience: engineers
---
# Security Threat Model

Name who can hurt the system before writing the next control.

## Scope

1. Draw the trust boundaries the change crosses.
2. List assets that become reachable across each boundary. [depends: 1]

## Threats

3. For each asset, name one concrete abuse case. [depends: 2]
4. Mark which abuses the current controls already stop. [kind: evidence_gate] [depends: 3]
5. Escalate residual critical abuses instead of hoping. [kind: upstream_agent] [depends: 4]

## Controls

6. Add the smallest control that closes each open abuse case. [depends: 4]
7. Verify the control with a test or a reviewed proof. [kind: test_gate] [depends: 6]
8. Record residual risk where the next reviewer will look. [kind: terminal] [depends: 7]

```text
A threat you will not write down is a threat you will not fund.
```

- [ ] Every new secret has a rotation owner.
- [ ] Residual risk is explicit, not implied.
