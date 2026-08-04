# Publishing the public crates

Four crates are prepared for crates.io. Everything else in the workspace
stays private (`publish = false`) and carries no license field.

| Crate | Depends on | What it is |
| --- | --- | --- |
| `cortex-domain` | — | Typed process-graph schema and validation invariants |
| `cortex-context` | — | Budget-bounded evidence selection and retrieval ranking |
| `cortex-router` | `cortex-domain` | Fail-closed, model-free routing policy |
| `cortex-skills` | `cortex-domain` | `SKILL.md` ↔ typed graph round trip |

All four names were free on crates.io as of 2026-08-04.

## Licensing

Dual **MIT OR Apache-2.0**, the Rust ecosystem convention. Each crate carries
its own copy of `LICENSE-MIT` and `LICENSE-APACHE`, because `cargo package`
only includes files under the crate directory — workspace-root license files
would not reach the published tarball.

## Order

`cortex-domain` and `cortex-context` publish independently. `cortex-router`
and `cortex-skills` depend on `cortex-domain`, so they can only be packaged
once it exists on the registry; until then `cargo package` fails with
`no matching package named 'cortex-domain' found`, which is the expected
gate rather than a defect.

```powershell
cargo package -p cortex-domain --no-verify   # sanity check the tarball
cargo publish -p cortex-domain
cargo publish -p cortex-context
# after cortex-domain is live on the registry:
cargo publish -p cortex-router
cargo publish -p cortex-skills
```

## Before the first publish

Publishing is irreversible: a version can be yanked but never removed, and
the name is claimed permanently. Confirm each item:

- **Dependency honesty.** `cortex-domain` and `cortex-skills` depend on
  [`blazingly-json`](https://crates.io/crates/blazingly-json) `0.1`, declared
  under its real name rather than the workspace's internal `serde_json`
  alias, so a consumer's manifest and rustdoc agree with reality.
  `cortex-domain` exposes `blazingly_json::Value` in `GraphNode::config`,
  which means public consumers take that dependency too. If the goal is
  maximum ecosystem reach instead, switch that one field to real
  `serde_json::Value` before the first publish — afterwards it is a breaking
  change.
- **Version floor.** Everything is `0.1.0`. A `0.1` dependency on
  `blazingly-json` keeps these crates below a comfortable `1.0` promise;
  reaching `1.0` means either `blazingly-json 1.0` or dropping that type from
  the public API.
- **Path plus version.** `cortex-router` and `cortex-skills` declare
  `cortex-domain = { path = "...", version = "0.1.0" }`. The path builds
  locally; cargo substitutes the registry version when publishing. Both must
  move together on every future version bump.
- **Nothing private leaks.** No crate in this set depends on
  `cortex-run`, `cortex-store`, `cortex-shadow`, `cortex-ollama`,
  `cortex-weavatrix`, `cortex-eval`, `cortex-mcp`, or the server.
