# Public eval

Gold fixtures live in `crates/cortex-eval/fixtures/`.

They are the public gate. They must never be copied into `corpora/train/`.
The corpus writer refuses to emit a train file that hashes, MinHash-overlaps,
or copies a gold fixture family from `fixtures/`.

Release numbers that should stay honest:

- leave-one-repository-out
- leave-one-language-out
- leave-one-task-family-out

`cortex-eval` implements those folds in `holdout.rs`. A reported accuracy
must name the held-out axis. Training on generated `corpora/train` rows of
the same *family* is allowed; training on the gold rows is not.

The private suite under `eval/private/` is not used when writing heuristics.
