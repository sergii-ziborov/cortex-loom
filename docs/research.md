# Research baseline — 2026-08-02

## Superpowers

[obra/superpowers v6.2.0](https://github.com/obra/superpowers/releases/tag/v6.2.0) is an MIT-licensed collection of portable Markdown skills and thin harness integrations. Its [skill-writing guide](https://raw.githubusercontent.com/obra/superpowers/main/skills/writing-skills/SKILL.md) uses YAML frontmatter, Markdown instructions, optional resources, and scenario-based evaluation. It is not an MCP server, scheduler, persistent graph, or agent runtime.

Cortex Loom imports the format and models the methodology as typed workflow semantics. It does not copy the full prose. Any future redistribution of copied upstream material must retain the [MIT notice](https://raw.githubusercontent.com/obra/superpowers/main/LICENSE).

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

## GPU and NPU

Ollama’s [hardware documentation](https://docs.ollama.com/gpu) documents GPU/CPU paths, not a Windows or Linux NPU backend. NPU execution therefore needs a separate pluggable adapter such as [OpenVINO GenAI](https://github.com/openvinotoolkit/openvino.genai) for Intel hardware or [Foundry Local](https://github.com/microsoft/foundry-local) where its supported device stack applies.

The current development machine has an Intel Core Ultra 7 255U, Intel Graphics, Intel AI Boost NPU, about 51 GB RAM, and no NVIDIA runtime. Ollama 0.32.5 is installed, but its two present XiYanSQL models are domain-specific and are not accepted as general coding fallbacks.

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
