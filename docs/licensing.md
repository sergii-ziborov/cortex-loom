# Licensing

Root `LICENSE-MIT` and `LICENSE-APACHE` apply **only** to the four published
crates. They do not license the rest of this workspace.

| Path | License |
|---|---|
| `crates/cortex-domain` | MIT OR Apache-2.0 |
| `crates/cortex-context` | MIT OR Apache-2.0 |
| `crates/cortex-router` | MIT OR Apache-2.0 |
| `crates/cortex-skills` | MIT OR Apache-2.0 |
| Everything else (`apps/`, `ui/`, remaining crates, `docs/` except this file) | Unlicensed. All rights reserved. |

A crate is published (`publish = true`) only when it carries both license
texts and `license = "MIT OR Apache-2.0"` in its manifest. CI fails if a
published crate is missing those files, or if an unpublished crate claims
that license.

Studio, MCP, eval, and Weavatrix adapter stay private until a separate
source-available or commercial grant is written.
