# Cortex Loom

Cortex Loom is a local, graph-native control plane in front of Codex and Claude. It selects evidence with deterministic tools and Weavatrix, delegates bounded low-risk transformations to local models, verifies their output, and leaves ambiguous or high-risk engineering decisions to the upstream coding agent.

The first milestone contains a Rust domain model, skill-to-graph compiler, hardware-aware local-model router, Weavatrix/Refactor adapter, bounded MCP server, and a browser-based editable graph extracted from AI Dev System's custom SVG workflow editor.

This repository is private. Candidate reusable crates remain in the workspace until their APIs, tests, security boundaries, and licensing are reviewed for separate public release.

Design notes: [architecture](docs/architecture.md), [research](docs/research.md), and [evaluation gates](docs/evaluation.md).

