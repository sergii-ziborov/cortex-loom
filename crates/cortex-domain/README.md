# cortex-domain

A transport-independent, typed process-graph schema with structural
validation invariants.

The graph is the canonical artifact: nodes carry a kind (human gate, test
gate, retry controller, local model, upstream agent, and more), edges carry a
kind (sequence, success, failure, fallback, approval, and more), and
`GraphDocument::validate` enforces the structural rules before anything
executes. Markdown, diagrams, and UI layouts are views of this graph, never
the source of truth.

## Why

If you orchestrate agents or humans through a workflow, the workflow itself
deserves a type. This crate gives you one that is:

- **Serializable and stable** — `serde` in, `serde` out, camelCase on the
  wire.
- **Validated** — duplicate ids, dangling edges, empty labels, and size
  limits are rejected up front.
- **Vendor-neutral** — no transport, no model, no runtime; just the schema
  and its invariants.

```rust
use cortex_domain::{default_control_plane, GraphDocument, NodeKind};

let graph: GraphDocument = default_control_plane();
assert!(graph.validate().is_ok());

let gates = graph
    .nodes
    .iter()
    .filter(|node| matches!(node.kind, NodeKind::QualityGate | NodeKind::HumanGate))
    .count();
assert!(gates > 0);
```

## Execution policy is enforced, not just described

`GraphDocument::validate` checks structure *and* two authority rules on any
node that carries an `ExecutionPolicy`: mutation authority is reserved for
`Upstream` or `Human` targets, and so is any target at `High` risk or above.
A graph that grants mutation to a local model is rejected. That is a
deliberate safety default; leave `execution` as `None` if you want to enforce
your own policy above the graph instead.

## Node configuration

`GraphNode::config` is a `HashMap<String, blazingly_json::Value>` used for
kind-specific settings, for example a retry controller's `targetNodeId` and
`maxAttempts`. The JSON engine is
[`blazingly-json`](https://crates.io/crates/blazingly-json); it is named
explicitly here rather than aliased, so what you depend on is exactly what
the manifest says.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
