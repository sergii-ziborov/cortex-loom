---
name: Release Checklist
description: Ship a version only after the gates that protect this repository pass.
when-to-use: Use before tagging, publishing, or opening a production deploy.
license: MIT
version: "2.0"
audience: engineers
---
# Release Checklist

Ship a version only after the gates that protect this repository pass.

## Gates

1. Run the repository's declared release gates and keep the logs. [kind: quality_gate]
2. Diff the changelog against the commits since the last tag. [kind: evidence_gate]
3. Confirm migrations and flags have owners for the deploy window. [depends: 2]

## Go

4. Tag or publish only after the gates are green. [depends: 1]
5. Watch the first canary or publish smoke before declaring done. [kind: evidence_gate] [depends: 4]
6. Escalate if the canary burns budget or the smoke disagrees with the changelog. [kind: upstream_agent] [depends: 5]
7. Record the release artifact digests beside the tag. [kind: terminal] [depends: 5]

```text
A release is a claim that the gates were run.
```

- [ ] Changelog matches the tag contents.
- [ ] Canary or smoke completed before announcement.
