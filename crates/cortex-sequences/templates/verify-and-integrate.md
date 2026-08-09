---
name: Verify and Integrate
description: Prove completion, preserve repository state, and integrate by explicit authority.
version: "1.0.0"
mechanics: verification-before-completion, finishing-a-development-branch, using-git-worktrees
---
# Verify and Integrate

Use this sequence after implementation is complete and before commit, merge, push, or release claims.

## Protect state

1. Inspect branch, worktree, ownership, dirty files, and integration authority. [kind: deterministic]
2. Isolate work when repository state or parallel activity could overlap. [kind: deterministic] [depends: 1]
3. Stop rather than overwrite unrelated or unauthorized changes. [kind: human_gate] [depends: 2]

## Prove

4. Map every completion claim to the command or artifact that proves it. [kind: agent_task] [depends: 3]
5. Run the full required format, test, lint, build, and product-specific gates. [kind: test_gate] [depends: 4]
6. Check fresh outputs, the final diff, file bounds, and cited evidence. [kind: evidence_gate] [depends: 5]
7. Allow one bounded correction for a concrete gate failure, then re-run the full gate. [kind: retry] [depends: 6]
8. Hand unresolved failures or new authority needs to the upstream agent or user. [kind: handoff] [depends: 7]

## Integrate

9. Present only integration actions allowed by the current authority. [kind: upstream_agent] [depends: 6]
10. Record commit identity, exact checks, and any action deliberately not taken. [kind: review_gate] [depends: 9]
11. Finish only after fresh evidence supports every reported claim. [kind: terminal] [depends: 10]
