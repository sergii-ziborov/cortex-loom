# Evaluation gates

The “90% less upstream work” idea is a measurable product target, not a release claim.

## Shadow phase

Local inference runs without changing workflow state. Every candidate result is compared with deterministic evidence and the upstream agent’s accepted outcome.

Promotion requires:

- 100% schema-valid structured output after at most one bounded repair;
- no missed escalation for security, authentication, concurrency, migrations, releases, deployment, publication, or destructive changes;
- valid source IDs or file/line anchors for every factual claim;
- measured extraction precision/recall and classification macro F1 on a repository-specific fixture set;
- retrieval Recall@k and nDCG on known questions;
- bounded latency, queue time, memory, context, and output size on the actual device;
- zero unauthorized mutations or hidden model fallback.

## Token accounting

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

