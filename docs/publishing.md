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

## Why 0.1.0 can ship now

The four crates deliberately omit `repository`: the workspace URL points at
this private repository and a link that 404s is worse than no link. Add it in
the next release once a public repository exists.

Everything else that gates a first upload is done and verified: metadata,
per-crate README rendered as compiled doctests, both license texts inside the
tarball, versioned path dependencies, no `unwrap`/`expect`/`panic!` in any
production path, and `cargo package` passing with full compile verification.

The list below is a **1.0** list, not a 0.1 list. In Rust, `0.x` is an
explicit statement that breaking changes are expected, and cargo enforces it:
every `0.x` bump is treated as incompatible, so each item can still be
changed with a normal minor release. Settle them before promising `1.0`.

## Decisions to settle before 1.0

1. **The version floor on `blazingly-json`.** `GraphNode::config` is
   `HashMap<String, blazingly_json::Value>`, so `cortex-domain` exposes that
   type publicly and consumers take the dependency too. That is deliberate —
   `blazingly-json` is a first-party crate and `mcport` already depends on it
   publicly, so this is the same stack rather than an outside risk. The one
   mechanical consequence: cargo treats `0.1 → 0.2` as incompatible, so
   `cortex-domain` cannot promise `1.0` while a `0.x` type sits in its API.
   Nothing to do for a `0.1.0` release; reaching `1.0` means releasing
   `blazingly-json 1.0` first. The dependency is declared by its real name
   rather than the workspace's `serde_json` alias so consumers see exactly
   what they take.
2. **How `cortex-router` is positioned.** Its API names first-party tools:
   `LocalAvailability { weavatrix, ollama }`, `ContextStrategy::WeavatrixEvidence`,
   `RoutingReason::{WeavatrixUnavailable, OllamaUnavailable}`, and
   `approves_local_model` matching `ExecutionTarget::Ollama`. This is a
   positioning choice, not a leak: as "the routing policy for the
   Weavatrix/Cortex stack" the names are correct and self-documenting; as "a
   general-purpose routing crate" a user of a different code-graph tool
   inherits names for tools they do not run. Whichever it is, the names are
   frozen at first publish. `cortex-domain` and `cortex-context` carry no
   such coupling either way.
3. **`cortex-domain::validate_execution` enforces policy, not only structure.**
   Every `GraphDocument::validate` call rejects mutation authority or
   high-risk work on any target other than `Upstream`/`Human`. That is now
   documented on `ExecutionPolicy` and in the README, and callers can opt out
   by leaving `execution` as `None` — but it remains an opinion shipped
   inside a schema crate. Moving it to `cortex-router` (where policy lives)
   is the alternative.
4. **`GRAPH_SCHEMA_VERSION` brands the wire format.** Documents must declare
   `"cortex-loom.graph.v1"`; an adopter's serialized graphs carry this
   product's name. Accepting any non-empty version string, with this value as
   the documented default, would remove that.
5. **`default_control_plane` ships an example topology.** Stale predecessor
   naming has been removed and it is documented as an example rather than a
   recommendation, but it is still product shape in a schema crate. Keeping
   it is defensible for first-run and tests; moving it to the application is
   the cleaner alternative.

## Also confirm

- **Path plus version.** `cortex-router` and `cortex-skills` declare
  `cortex-domain = { path = "...", version = "0.1.0" }`. The path builds
  locally; cargo substitutes the registry version when publishing. Both must
  move together on every future version bump.
- **Nothing private leaks.** No crate in this set depends on `cortex-run`,
  `cortex-store`, `cortex-shadow`, `cortex-ollama`, `cortex-weavatrix`,
  `cortex-eval`, `cortex-mcp`, or the server. Those stay `publish = false`
  *and* license-field-free, so an accidental publish fails twice.
- **Tarball contents.** Run `cargo package -p <crate> --list` and confirm no
  scratch files crept into the crate directory; `cargo package` includes
  everything under it.
