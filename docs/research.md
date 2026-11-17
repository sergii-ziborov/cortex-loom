# Research baseline — 2026-08-02

## Superpowers

[obra/superpowers v6.2.0](https://github.com/obra/superpowers/releases/tag/v6.2.0) is an MIT-licensed collection of portable Markdown skills and thin harness integrations. Its [skill-writing guide](https://raw.githubusercontent.com/obra/superpowers/main/skills/writing-skills/SKILL.md) uses YAML frontmatter, Markdown instructions, optional resources, and scenario-based evaluation. It is not an MCP server, scheduler, persistent graph, or agent runtime.

Cortex Loom borrowed the portable `SKILL.md` *shape* and a checklist of
mechanic names. The shipped sequences are original typed graphs with our own
step scripts, evidence gates, and Weavatrix/upstream edges — not a Superpowers
fork and not a 1:1 copy of those files. Training corpora under `corpora/` are
Cortex-original for the same reason. Any future redistribution of copied
upstream material must retain the [MIT notice](https://raw.githubusercontent.com/obra/superpowers/main/LICENSE).

## Weavatrix refactor boundary

The backend uses first-party Rust crates only: `weavatrix-rust` 2.17.17 for read-only repository evidence, `weavatrix-rust-refactor` 0.1.13 for native refactor plan production, `weavatrix-refactor-plan` 0.1.1 for bounded parsing/validation/fingerprints, and `weavatrix-edit` 0.1.7 for exact in-memory rendering. 2.16.1+ is taken for session `Arc` sharing, cached `graph_stats`, `expected_repository` drift checks, `git_read_blob`, and graph `inspect_symbol`. 2.16.2 keeps n8n/Dify/Agent/Mermaid/Web3 out of this planner. 2.16.3 is taken so `coverage_map` is documented as ingest-only (Quality may publish `.weavatrix/coverage/lcov.info`; a missing report is unmeasured, not 0%). 2.17.4 is taken for `context_bundle` caller/callee quotas and for `find_references` on blast-radius questions. 2.17.17 is taken for the graph and evidence fixes listed in [CHANGELOG.md](../CHANGELOG.md); of its new arguments only the `git_history` `path` is planned, for a history question that names one file. `ci_restrictions` is not planned: a workflow file is not a code fact. Weavatrix Quality (`@weavatrix/wvq` / `wvq-*` 0.1.0-alpha.5) stays a sibling PROVE product: Cortex does not depend on it, execute its runners, or copy `quality_run` / obligation proof. Workflow/Web3/Agent/diagram operations from 2.11–2.17 are not planned here. `weavatrix-rust-refactor` 0.1.11 pinned `weavatrix-rust = 2.17.12`, which cannot share a build with 2.17.17. 0.1.13 pins 2.17.17, so it shares this build and graph type, and Cortex takes it for plan production only. Its `rename_symbol`, `rename_related_symbols`, `change_signature`, `move_file`, `move_symbol`, `extract_duplicate`, `extract_window`, and `share_method_body` producers prove sites from the graph; before, an upstream agent had to write every coordinate and hash of such a plan by hand. `weavatrix_refactor_plan` converts the native `weavatrix.edit-plan.v1` envelope into `weavatrix.refactor-plan.v1` and sends it through the same validation and preview as an upstream-authored plan. The crate also owns transactional apply and rollback (`apply_edit_plan`, `rollback_last_apply`, and `mode: "apply"` on the rename tools). Cortex runs a read-only session, refuses `mode` and `confirm_token`, drops the planner's confirm token, and never applies; the MCP host `weavatrix-refactor` (1.0.17 on the same engine) is the product that applies. Planning takes that host's shared reader lock, which creates an empty `.weavatrix/refactor-write.lock` when it is missing. `edit_symbol`, `bulk_replace`, `organize_imports`, `remove_duplicates`, and `delete_closure` are not called; those plans stay upstream-authored. A native `COMPLETE` arrives without the typed `completenessProof` the plan validator requires, so Cortex records it as `PARTIAL` with a `NATIVE_COMPLETE_WITHOUT_PROOF` warning instead of inventing a proof. A native `move_file` into a directory that does not exist yet fails the Cortex preview, which requires the destination's parent.

Version 2.17.5 clips long budgeted `search_code` lines around their matches,
so one generated line cannot erase later short hits. Cortex also searches
without a broad positive glob when possible, preserving repository ignore
rules before the native result limit is applied.

From 2.17.6 `search_code` reads one resident content index per process,
stored under `~/.weavatrix/search` (or `WEAVATRIX_SEARCH_HOME`) with a file
watcher. From 2.17.9 a glob filters that index, and its matcher has no
`{a,b}` alternation: Cortex's multi-language glob matched nothing. Cortex
asks each alternative separately; the index answers in path order, so the
merged, re-budgeted prefix is the one a brace-aware call would keep.

## MCP

Codex supports local stdio and Streamable HTTP servers, shared host configuration, server instructions, allowlists, approvals, and bounded startup/tool timeouts in the current [Codex MCP documentation](https://learn.chatgpt.com/docs/extend/mcp). Claude Code supports local/remote scopes, tools, resources, prompts, and plugin-bundled MCP configuration in its [official MCP guide](https://code.claude.com/docs/en/mcp).

The implementation targets stable `2025-11-25` first. The [`2026-07-28` release candidate](https://github.com/modelcontextprotocol/modelcontextprotocol/releases/tag/2026-07-28-RC) remains behind negotiated compatibility. Security follows the protocol’s [security guidance](https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices), and verification should include the official [conformance suite](https://github.com/modelcontextprotocol/conformance) plus adversarial subprocess tests.

## Local inference

Ollama provides structured chat, tools, embeddings, model details, residency, and operational metrics through its [chat API](https://docs.ollama.com/api/chat), [structured outputs](https://docs.ollama.com/capabilities/structured-outputs), [embedding API](https://docs.ollama.com/api/embed), and [`/api/ps`](https://docs.ollama.com/api/ps).

Initial profiles to evaluate rather than blindly install:

- `embeddinggemma` for cheap short embeddings;
- `qwen3-embedding:0.6b` for code-heavy retrieval;
- `qwen3.5:4b` or `phi4-mini` for constrained extraction/classification;
- `qwen3.5:9b` for stronger summarization;
- `qwen3-coder:30b` for code-specialist critique when hardware and latency allow.

Model digest, quantization, schema/prompt version, context, and device must be pinned in an evaluated profile.

## Local runtimes

Ollama’s [hardware documentation](https://docs.ollama.com/gpu) documents GPU/CPU paths, not a Windows or Linux NPU backend. An NPU path needs a separate adapter such as [OpenVINO GenAI](https://github.com/openvinotoolkit/openvino.genai) or [Foundry Local](https://github.com/microsoft/foundry-local) where that stack applies.

Models-off is a valid product path. A missing local model is not an
installation error. Domain-specific SQL models are not accepted as
general coding fallbacks. Pin digest, quantization, schema/prompt
version, and device in an evaluated profile before treating a local
model as advisory.

## Agent Finder scan

GitHub Agent Finder was queried on 2026-08-03 for MCP construction, editable workflow skills, code-graph/refactor integrations, context compression, and Ollama orchestration. The most relevant results were:

| Resource | Type | Relevance |
| --- | --- | ---: |
| [MCP Server Dev](https://github.com/anthropics/claude-plugins-public/blob/main/plugins/mcp-server-dev) | Copilot/Claude plugin | 80 |
| [MCP Builder](https://github.com/anthropics/skills/blob/main/skills/mcp-builder/SKILL.md) | AI skill | 80 |
| [Serena](https://github.com/anthropics/claude-plugins-public/blob/main/external_plugins/serena) | Copilot/Claude plugin | 75 |
| [Designing Workflow Skills](https://github.com/trailofbits/skills/blob/main/plugins/workflow-skill-design/skills/designing-workflow-skills/SKILL.md) | AI skill | 50 |
| [MCP Apps](https://github.com/modelcontextprotocol/ext-apps/blob/main/plugins/mcp-apps) | Copilot/Claude plugin | 40 |

These scores measure query relevance, not trust, safety, quality, or architectural fit. Nothing was installed automatically. Cortex Loom already owns its Rust transport, graph editor, and Weavatrix boundary; the skill-design resources are useful evaluation references, while Serena is primarily a competitor/reference rather than a dependency.
