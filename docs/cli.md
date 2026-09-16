# CLI

`cortex-loom` is a thin front for the same prepare/expand compiler as
`cortex-mcp --profile agent`. It does not edit code, run tests, or start
a second coding agent.

```powershell
cortex-loom --help
cortex-loom doctor --repo .
cortex-loom prepare --repo . --task-file task.md --budget 6000 --format json
cortex-loom expand --packet <packet-id> --facet callers --format json
cortex-loom setup --agent claude-code --dry-run
cortex-loom report --last
```

## Task input

Use exactly one of `--task`, `--task-file`, or `--task-stdin`. Passing
two sources is an argument error. `--budget 2000|6000|16000` maps to
`tight|normal|wide`. `--budget-class` is not a separate flag in this
release.

## Output

This release prints JSON on stdout. Logs and setup notes go to stderr.
`--format json` is accepted; other formats are rejected.

Exit `0` means the process finished and wrote JSON. A packet can still
be `sufficient: false`. Do not treat a zero exit as proof that every
declared requirement was covered.

## State

Prepare writes `<repo>/.cortex-loom/cli/last.json` and
`<packetId>.json` so a later process can expand the same packet. That
directory is local cache, not a project memory plane. Setup is
preview-only and refuses `--write`.

`serve`, `config`, and `cache` are not commands yet. MCP is still
`cortex-mcp --profile agent`.
