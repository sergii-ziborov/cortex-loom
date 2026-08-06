---
name: Schema Evolution
description: Evolve a persisted schema without trapping old readers or writers.
when-to-use: Use when protobuf, JSON, SQL, or file formats change on disk or wire.
license: MIT
version: "2.0"
audience: engineers
---
# Schema Evolution

Evolve a persisted schema without trapping old readers or writers.

## Compat

1. Classify the change as backward compatible, forward compatible, or neither.
2. Reject neither unless a coordinated deploy is funded. [kind: quality_gate]

## Roll

3. Deploy readers that accept both shapes before writers emit the new one. [depends: 1]
4. Only then deploy writers. [depends: 3]
5. Keep a decoder for the old shape until retention expires. [kind: evidence_gate] [depends: 4]

## Close

6. Remove the old decoder after retention and a final scan. [kind: terminal] [depends: 5]

```text
Writers first is how you strand your readers.
```

- [ ] Readers landed before writers.
- [ ] Retention expiry is dated.
