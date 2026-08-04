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

## Blocking: the repository link 404s

All four crates inherit `repository = "https://github.com/sergii-ziborov/cortex-loom"`,
which is **private**. Every crates.io and docs.rs visitor would follow that
link to a 404. Before the first upload, either make the repository public or
override `repository` per crate to point somewhere real.

## Decisions that are one-way doors

Publishing is irreversible: a version can be yanked but never removed, the
name is claimed permanently, and the public API becomes a compatibility
promise. Each item below is cheap to change now and breaking afterwards.

1. **The JSON value type in `cortex-domain`'s public API.**
   `GraphNode::config` is `HashMap<String, blazingly_json::Value>`. Three
   consequences: consumers must take `blazingly-json 0.1` themselves; cargo
   treats `0.1 → 0.2` as incompatible, so any release of that crate is
   automatically a breaking change here; and `cortex-domain 1.0` is not
   reachable while a `0.x` type sits in its API. Options, in increasing
   independence: keep it (fine if these crates stay `0.x`), switch to real
   `serde_json` 1.0 (two lines here plus the alias in `cortex-skills` and
   `cortex-run`), or define a crate-owned `#[serde(untagged)] enum
   ConfigValue` with the same wire format and drop the dependency entirely.
2. **`cortex-router` freezes vendor names into a public contract.**
   `LocalAvailability { weavatrix, ollama }`, `ContextStrategy::WeavatrixEvidence`,
   `RoutingReason::{WeavatrixUnavailable, OllamaUnavailable}`,
   `approves_local_model` matching `ExecutionTarget::Ollama`, and `"weavatrix"`
   as a classifier keyword. Nothing secret leaks — both are public products —
   but a user of a different code-graph tool or local runtime inherits names
   for tools they do not run, permanently. Either rename to generic terms
   (`code_graph`, `local_model`) first, or publish `cortex-router` in a later
   batch once the naming is settled. `cortex-domain` and `cortex-context`
   have no such coupling.
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
