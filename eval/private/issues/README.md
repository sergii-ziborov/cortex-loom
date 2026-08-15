# Hidden issue / PR suite

Reserved. Do not read these cases when writing heuristics, prompts, or
training data. A release claim that cites only `eval/public` or
`crates/cortex-eval/fixtures/` is a development number.

Each hidden task is a real issue or PR at a pinned commit, with tests
that development must not look at.

Shape: `id`, `repository`, `commit`, `prompt`, `hiddenTests[]`.
The `hiddenTests` field is the oracle. It is not a training target.
