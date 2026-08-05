---
name: Performance Investigation
description: Measure the real workload before optimizing anything.
when-to-use: Use when a workload is slower than its target and the cause is unmeasured.
license: MIT
version: "2.0"
audience: engineers
---
# Performance Investigation

Intuition about where time goes is reliably wrong, and a synthetic benchmark
is a different program than the one that is slow.

## Frame

1. State the goal as a number attached to a workload.
2. Reproduce the slowness on the workload that motivated the complaint.
3. Record the baseline with its variance, not a single run. [kind: evidence_gate] [depends: 2]

```text
Measured on a fixture, deployed against production sizes: that gap has
already bitten this project once.
```

## Locate

4. Profile before forming an opinion about the hot path. [depends: 3]
5. Separate time spent from time waited: compute, allocation, lock contention, and input-output are different problems. [depends: 4]
6. Confirm the suspect by changing only it and watching the number move. [kind: evidence_gate] [depends: 5]
7. Escalate when the cost is architectural rather than local. [kind: upstream_agent] [depends: 5]

## Improve

8. Choose between the cheap fix, the structural fix, and doing nothing. [kind: branch] [depends: 6]
9. Re-run the correctness suite before believing any number. [kind: test_gate] [depends: 8]
10. Re-measure against the original workload and keep the comparison in the repository. [depends: 9]
11. Accept the change only if the measured gain is worth the complexity added. [kind: review_gate] [depends: 10]
12. Land the optimization with its before and after recorded. [kind: terminal] [depends: 11]

- An optimization without a before and an after is a refactor with a story.
- [ ] The regression guard covers the workload, not the microbenchmark.
- [ ] The complexity added is worth the milliseconds removed.
