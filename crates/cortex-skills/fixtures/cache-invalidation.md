---
name: Cache Invalidation
description: Define what becomes stale and when it must be gone.
when-to-use: Use when introducing or changing a cache that can serve wrong data.
license: MIT
version: "2.0"
audience: engineers
---
# Cache Invalidation

Define what becomes stale and when it must be gone.

## Keys

1. Name the cache key and the authoritative source it mirrors.
2. List every write that must invalidate or update that key. [kind: weavatrix] [depends: 1]

## Policy

3. Choose TTL, explicit invalidate, or versioned keys — and write why. [depends: 2]
4. Prove a write is visible to the next read under that policy. [kind: test_gate] [depends: 3]
5. Escalate if the policy cannot be tested. [kind: upstream_agent] [depends: 3]

## Operate

6. Expose a metric for hit rate and stale-serve if detectable. [depends: 4]
7. Document the purge procedure beside the cache. [kind: terminal] [depends: 5]

```text
The hard part is knowing what you promised to forget.
```

- [ ] Every write path invalidates or versions.
- [ ] A purge procedure exists.
