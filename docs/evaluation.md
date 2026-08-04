# Evaluation gates

The “90% less upstream work” idea is a measurable product target, not a release claim.

## Shadow phase

Local inference runs without changing workflow state. Every candidate result is compared with deterministic evidence and the upstream agent’s accepted outcome. Shadow mode is implemented behind explicit `CORTEX_SHADOW=1` configuration ([shadow-mode.md](shadow-mode.md)); it turns real MCP traffic into the same metrics the offline harness produces.

Promotion requires:

- 100% schema-valid structured output after at most one bounded repair;
- no missed escalation for security, authentication, concurrency, migrations, releases, deployment, publication, or destructive changes;
- valid source IDs or file/line anchors for every factual claim;
- measured extraction precision/recall and classification macro F1 on a repository-specific fixture set;
- retrieval Recall@k and nDCG on known questions;
- bounded latency, queue time, memory, context, and output size on the actual device;
- zero unauthorized mutations or hidden model fallback.

## Token accounting

The `usage_samples` ledger records every routing decision and every context
compilation (budget, raw/selected/saved tokens, `requiresUpstream`, latency);
read it via the `usage_read` MCP tool or `GET /api/usage/summary`. First
on-repo measurement: the dogfood task packet compressed from 7479 raw
evidence tokens to 1461 under a 4k budget (80.5% saved, omission reported,
fail-closed escalation unchanged), against roughly 9k tokens of naive file
reading.

Record, per workflow:

- raw evidence bytes and estimated tokens;
- compact evidence sent upstream;
- local input/output tokens and latency;
- upstream input/output tokens;
- rejected local drafts and fallback reason;
- correctness verdict from tests and review.

Report savings only for quality-equivalent accepted runs. A smaller prompt that causes extra repair loops is not a saving.

## Escalation

Escalate when evidence is missing or contradictory, validation fails, a budget is exceeded, a task is repository-wide or high-risk, or the user asks for the stronger model. Silent fallback to a smaller local model is forbidden.

## Calibration harness

`cortex-eval` measures candidate profiles offline against typed fixtures. It never pulls a model: absent models are reported as `model_absent` and skipped.

```powershell
cargo run -p cortex-eval -- --discover
cargo run -p cortex-eval -- --profile local-small
cargo run -p cortex-eval -- --suite classification --limit 5
```

Profiles live in `config/eval-profiles.json` (exact tags only). Reports land in `.cortex-loom/eval/` as JSON plus a Markdown summary on stdout, pinned to prompt/schema versions, with the model digest and CPU/GPU placement recorded.

Encoded gates per profile: schema-valid rate ≥ 0.95 per gated suite, zero missed escalations, classification accuracy ≥ 0.8, extraction action accuracy ≥ 0.8 and exact-match ≥ 0.6, minimum citation-preservation ≥ 0.9, zero hallucinated citations, and a negative mean token delta (the draft must actually compress).

Verdicts are role-aware: a profile is calibrated for the tier it would be granted. `local_small` is gated on classification and extraction; `local_medium` on citation-preserving compression; any other tier on the full matrix. Suites outside the role are still measured and reported, but they never gate the verdict, because routing never assigns that work to the profile. A failing verdict is calibration data, not a build failure; shadow-mode profiles must reference a calibration record for the exact model tag and role.

