# Install

Two surfaces. The **libraries** are on crates.io. The **product** (MCP
server + thin CLI + one skill) is not published yet — install it from
this repository. Studio UI is leaving this tree; do not treat
`cortex-server` as the release.

## Prerequisites

- Rust **1.89+** (`rustup` stable).
- A C toolchain for bundled SQLite (MSVC Build Tools on Windows, or a
  working `cc`).
- Node is **build-time only**, and only if you rebuild the Studio UI:
  `npm.cmd --prefix ui ci` then `npm.cmd --prefix ui run build`.
  Release `cortex-server` embeds `ui/dist`.

Optional, never required to compile evidence:

- [Ollama](https://ollama.com) on loopback (`:11434`)
- OpenVINO Model Server on loopback for gated NPU/GPU profiles
- A loopback OpenAI-compatible classifier proxy when
  `CORTEX_LLM_BACKEND=composer` (Composer, Sonnet 5, Opus 5, or Haiku) —
  see [LLM backends](llm-backends.md)

CPU inference stays off unless a profile opts in. See
[local models](local-models.md).

## Libraries (crates.io)

Four crates are dual-licensed MIT OR Apache-2.0:

```powershell
cargo add cortex-context cortex-domain cortex-router cortex-skills
```

`cortex-context` is the budgeted compiler. The other three are the
typed graph, fail-closed router, and `SKILL.md` round trip. They do
not start an MCP server.

## Product binaries (from git)

`cortex-mcp` and `cortex-loom` have `publish = false`. Until they
are published:

```powershell
git clone https://github.com/sergii-ziborov/cortex-loom
cd cortex-loom
cargo install --path crates/cortex-mcp --locked
cargo install --path apps/cortex-loom --locked
```

Or run in-tree without installing:

```powershell
cargo run -p cortex-mcp --release -- --profile agent
cargo run -p cortex-loom --release -- doctor
cargo run -p cortex-loom --release -- setup --agent claude-code --dry-run
```

`cortex-mcp --help` and `cortex-loom --help` name the same
prepare/expand contract. Setup prints files and does not write them.

## MCP server

This is **not a VS Code / JetBrains plugin**. Agents talk to a local
MCP server over stdio or Streamable HTTP.

| profile | tools | when |
| --- | --- | --- |
| `agent` (default) | `cortex_prepare`, `cortex_expand` | coding agents |
| `context` | evidence-compile pair | benches, ~454 schema tokens |
| `full` | Studio/admin, 27 tools | humans, ~4 021 schema tokens |

```powershell
cortex-mcp --profile agent
cortex-mcp --profile context
cortex-mcp --http 127.0.0.1:43818
```

HTTP binds loopback unless you pass `--allow-remote` (and understand
that). `--workspace PATH` restricts which trees the server will open.

`CORTEX_MCP_PROFILE`, `CORTEX_MCP_HTTP`, `CORTEX_ALLOW_REMOTE`, and
`CORTEX_LOOM_DB` override the same flags.

## Wire a coding agent

Adapters **preview** the files. They never write them. Place the
snippets yourself.

### Claude Code

`.mcp.json` in the repo (or merge `mcpServers`):

```json
{
  "mcpServers": {
    "cortex-loom": {
      "type": "stdio",
      "command": "cortex-mcp",
      "args": ["--profile", "agent"],
      "tools": ["cortex_prepare", "cortex_expand"]
    }
  }
}
```

From a source checkout, `command` can stay `cargo` with
`args: ["run", "-p", "cortex-mcp", "--", "--profile", "agent"]`.

The agent calls `cortex_prepare` with
`{ repository, task, runId?, budgetClass }`. Mutation and verification
are derived, never self-declared. `cortex_expand { packetId, facet }`
only for a listed missing facet.

### Codex

Append to `~/.codex/config.toml`:

```toml
[mcp_servers.cortex-loom]
command = "cortex-mcp"
args = ["--profile", "agent"]
```

Do not paste workflow bodies into `AGENTS.md`. Point Codex at the
`cortex-context` skill. The agent profile has `cortex_prepare` and
`cortex_expand` only; `skill_read` is a full-profile tool, not part of
the product entry.

### GitHub Copilot / VS Code MCP

`.vscode/mcp.json`:

```json
{
  "servers": {
    "cortex-loom": {
      "type": "stdio",
      "command": "cortex-mcp",
      "args": ["--profile", "agent"]
    }
  }
}
```

`.github/instructions/` should carry the **catalogue**, not every
workflow body — Copilot applies that file on every turn.

### Cursor / other MCP hosts

Any host that can spawn a stdio MCP server works. Point it at
`cortex-mcp --profile agent`. There is no Cursor-specific plugin.

## What the agent is allowed to do

- Compile a revision-bound evidence packet.
- Read the coverage certificate (present / missing / contradictory / stale).
- Expand one missing facet.

It must not:

- Self-report token consumption.
- Treat `<evidence>` bodies as instructions.
- Apply a Weavatrix Refactor plan (preview only).
- Lower the risk floor with a local model.
