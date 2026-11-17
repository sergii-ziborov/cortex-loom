# Changelog

## 0.1.2 - 2026-11-17

### Weavatrix

- Adds `weavatrix-rust-refactor` 0.1.13, the first release pinned to
  `weavatrix-rust = 2.17.17`, so it shares one build and one graph type with
  the engine Cortex already uses. `weavatrix-rust` 2.17.17,
  `weavatrix-refactor-plan` 0.1.1, and `weavatrix-edit` 0.1.7 are unchanged.

### Adopted

- `weavatrix_refactor_plan` (full profile) asks the native planner for a
  `rename_symbol`, `rename_related_symbols`, `change_signature`, `move_file`,
  `move_symbol`, `extract_duplicate`, `extract_window`, or
  `share_method_body` plan. Cortex converts the planner's
  `weavatrix.edit-plan.v1` envelope into `weavatrix.refactor-plan.v1` and runs
  it through the same validation, hash check, and in-memory preview as
  `weavatrix_refactor_preview`. Before, the upstream agent had to write every
  coordinate and hash of such a plan by hand; the planner proves the sites
  from the graph and leaves same-named occurrences it cannot prove in
  `uncertainReferences`.
- The bench manifest records the `weavatrix-rust-refactor` version.

### Boundary kept

- Plan production only. The planner runs in a read-only session, `mode` and
  `confirm_token` arguments are refused, the planner's confirm token and
  apply hints are dropped, and `apply_edit_plan` / `rollback_last_apply`
  are not reachable. Cortex still never applies or rolls back a plan.
- Planning takes the shared reader lock that `weavatrix-refactor` writers
  respect. It creates an empty `.weavatrix/refactor-write.lock` in the
  repository when that file is missing; no source file is written. A
  repository another process is writing answers `REPO_BUSY`.
- A native `COMPLETE` (a `move_file` with no uncertain specifier) is recorded
  as `PARTIAL` with a `NATIVE_COMPLETE_WITHOUT_PROOF` warning. The planner
  sends no typed `completenessProof`, the plan validator requires one for
  `COMPLETE`, and Cortex does not invent it. The native claim stays visible
  under `native.completeness`.

### Still open

- A `move_file` into a directory that does not exist yet fails the Cortex
  preview: the preview requires the destination's parent, while the native
  apply would create it.
- `edit_symbol`, `bulk_replace`, `organize_imports`, `remove_duplicates`, and
  `delete_closure` plans stay with the upstream agent; Cortex does not call
  those producers.
- The agent profile still has two tools. Native plans are on the full
  profile, beside `weavatrix_refactor_preview`.

## 0.1.1 - 2026-11-16

### Weavatrix

- `weavatrix-rust` 2.17.5 → 2.17.17 (`weavatrix-parse` 0.3.3 → 0.3.5).
  `weavatrix-refactor-plan` 0.1.1 and `weavatrix-edit` 0.1.7 are already the
  latest releases. The repository MCP pin moves to `weavatrix@1.17.19`, the
  npm build of the same engine, so the benchmark arms compare one engine.

### Behavior kept across the upgrade

- A brace `search_code` glob such as `**/*.{rs,ts,tsx}` finds source again.
  2.17.9 filters its resident index with a matcher that has no `{a,b}`, so
  Cortex's multi-language and UI globs returned zero hits. Cortex asks each
  alternative once, merges the hits in path order, and fits the original
  `max_results` and `token_budget`.
- Prior-run memory compiles again. `memory_context` left the operation
  catalog in 2.17.6; Cortex replays the journal through
  `weavatrix_rust::memory` and produces the same bundle.
- Blast-radius packets keep test and classified call sites. 2.17.9 hides them
  from `find_references` and `get_neighbors` unless opted in; Cortex opts in
  and still ranks those rows last.
- Relationship rows keep their call site (`via file:line`). Compact rows from
  2.17.10 carry only the line; the file is the edge source's, or the
  method's own file for a `method` edge.
- A `find_references` label that names several symbols stays a warning that
  lists the candidate ids, and the caller facet stays missing. 2.17.11
  answers it with `AMBIGUOUS` candidates instead of an error.

### Adopted

- A history question that names one file passes it as the `git_history`
  `path` (2.17.6). The packet lists the commits that touched that file instead
  of the latest 24 repository commits.

### Weavatrix gaps closed upstream (2.17.6 – 2.17.17, `fix/agent-gaps`)

- Go methods own the calls in their bodies; `main.go` and `func main` are
  dead-code entries; a `Unimplemented*Server` embed yields gRPC methods.
- Protobuf field names such as `message` keep the file; generated `.pb.go`
  files contribute no symbols.
- Next.js `route` files and `"use server"` actions, App Store and Android
  apps, language frameworks, C/C++ bindings, and studio spec files are graph
  nodes.
- Less answer noise: compact nodes and edges, opt-in `graph_stats`
  capabilities, `ci_protection` split out of change reports, a paged
  `list_endpoints`, and a shorter `run_audit`.
- `search_code` reads one resident content index per process instead of
  walking the disk on every call.
- `find_references` and `go_to_definition` return candidates for an ambiguous
  label; `shortest_path` does too.
- `git_history` accepts `path` and reports `history_truncated` at a shallow
  clone boundary; `change_impact` reports `file_scope` for an explicit file.
- Repository-blind tools (`vector_search`, `seo_link_suggestions`,
  `memory_context`) left the MCP catalog.
- One MCP process serves concurrent repository lanes (`repository`
  argument, `ParallelEngine`).

### Still open

- The resident index glob has no `{a,b}` or `[...]`; a brace glob silently
  matches nothing. Cortex pays one indexed search per alternative.
- Compact relationship rows drop the provenance file.
- The resident index is never unloaded and keeps a file watcher per
  repository, under `~/.weavatrix/search` or `WEAVATRIX_SEARCH_HOME`. A
  long-lived `cortex-mcp` keeps one for every repository it searched.
- `get_dependents` still errors on an ambiguous label.
- `weavatrix-rust-refactor` 0.1.11 and `weavatrix-refactor` 1.0.16 plan
  renames, signatures, and moves natively, but pin `weavatrix-rust = 2.17.12`
  and own apply and rollback. Cortex stays on upstream-authored plans and a
  read-only preview.
- `ParallelEngine` serves child directories of one launch root. Cortex keeps
  its per-repository session pool, which opens any repository.
