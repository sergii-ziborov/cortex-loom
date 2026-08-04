# cortex-router

A fail-closed, model-free routing policy: given a task and its context, it
decides between deterministic tooling, repository analysis, a bounded local
model, or the upstream (strong) agent.

Model self-reported confidence is deliberately absent from the API. Routing
is a function of inspectable inputs only: task text, evidence state, schema
validity, token budgets, mutation authority, and local capability
availability.

## Why

"Should a small local model handle this?" is a safety question, not an
optimization question. This crate answers it with rules you can read, test,
and audit:

- **Fail-closed guards.** Missing or contradictory evidence, a schema
  failure, or a budget overrun always escalates upstream.
- **Mutation is never local.** Anything likely to mutate code or state is
  reserved for the upstream agent.
- **High-risk classes are named.** Security, authentication, concurrency,
  migration, release, deployment, and publication escalate by class.
- **Plans never exceed the caller's bound.** The returned `ContextPlan`
  reports at most the `max_input_tokens` the request declared.

```rust
use cortex_router::{route, EvidenceStatus, ExecutionTarget, RoutingRequest};

let mut request = RoutingRequest::new("Deploy the service to production");
request.evidence = EvidenceStatus::Verified;
let decision = route(&request);
assert_eq!(decision.target, ExecutionTarget::Upstream);
assert!(!decision.advisory_only);

let mut summary = RoutingRequest::new("Summarize the supplied evidence");
summary.evidence = EvidenceStatus::Verified;
assert_eq!(route(&summary).target, ExecutionTarget::Ollama);
```

## Scope and limits

Classification is lexical and currently tuned for English task text, which is
a deliberate trade: it is fast, deterministic, and inspectable, with no model
in the loop. Treat `classify` as a conservative first filter — it escalates
when uncertain — and layer richer signals above it if you need them.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
