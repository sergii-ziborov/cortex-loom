---
name: Contract Testing
description: Lock consumer and provider expectations so integrations fail in CI, not in prod.
when-to-use: Use when two services or packages share a schema or API.
license: MIT
version: "2.0"
audience: engineers
---
# Contract Testing

Lock consumer and provider expectations so integrations fail in CI, not in prod.

## Capture

1. Record the consumer's expected requests and responses as fixtures.
2. Generate or hand-write the provider verification from those fixtures. [depends: 1]

## Verify

3. Run consumer tests in the consumer pipeline. [kind: test_gate] [depends: 2]
4. Run provider verification in the provider pipeline. [kind: test_gate] [depends: 2]
5. Break the build on contract drift. [kind: quality_gate] [depends: 3]

## Evolve

6. Escalate when consumer and provider disagree on whether a drift is intentional. [kind: upstream_agent] [depends: 5]
7. Version the contract when a breaking change is intentional. [kind: terminal] [depends: 5]

```text
A handshake that is only tested in production is not a handshake.
```

- [ ] Both sides run in CI.
- [ ] Drift fails the build.
