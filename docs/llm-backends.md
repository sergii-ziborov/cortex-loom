# Cortex LLM backends

`cortex_prepare` now reports `internalModel` so a caller can see which
backend ran and how many tokens it spent. Three backends, one at a time:

| `CORTEX_LLM_BACKEND` | What runs | Token fields |
| --- | --- | --- |
| `off` (default) | Lexical `route()` only | all zeros |
| `local` | Gated classifier from `config/llm-profiles.json` (OVMS qwen3-8b when live) | `localTokens` |
| `composer` | Loopback cursor-agent proxy (`classifierModel`) | `composerTokens` |

`CORTEX_LLM=1` with no backend flag still means `local`. `CORTEX_LLM_BACKEND=off`
wins over `CORTEX_LLM=1`.

The model may only escalate the lexical floor. Failures keep the lexical
decision and still record an attempt. High-risk or unverified work stays
with the upstream coding agent. Cortex does not call Composer as a coding
agent and does not send local-model traffic off this machine.

## Loopback classifier proxy

Cursor does not give Cortex a cloud chat URL. Set a loopback proxy and pick
the classifier with CLI, env, or `cortex_prepare.classifierModel`:

```
CORTEX_LLM_BACKEND=composer
CORTEX_COMPOSER_BASE_URL=http://127.0.0.1:8787
CORTEX_CLASSIFIER_MODEL=sonnet-5   # composer | sonnet-5 | opus-5 | haiku
CORTEX_COMPOSER_MODEL=sonnet-5     # same aliases; kept for older scripts
CORTEX_COMPOSER_API_KEY=           # or CURSOR_API_KEY
```

The CLI has no `--llm-backend` flag. `cortex-loom prepare` follows
`CORTEX_LLM_BACKEND` (`off`, `local`, or `composer`) and accepts
`--classifier-model sonnet-5` for the loopback aliases. MCP:
`cortex_prepare({ repository, task, classifierModel: "opus-5" })`.

Aliases map to cursor-agent ids (`composer-2.5`,
`claude-sonnet-5-thinking-max`, `claude-opus-5-thinking-high`,
`claude-haiku-4-5`). Sonnet is max, not high. Opus stays high, not
ultra. Haiku 4.5 answers on this account even though `--list-models`
omits it; `[effort=high]` is rejected by cursor-agent.

Default URL is `http://127.0.0.1:8787` (`/v1/chat/completions`). A remote
host is refused. If the proxy is down, prepare stays lexical and
`internalModel.warning` explains why; `composerTokens` stays 0.

## Packet field

```json
{
  "mode": "composer",
  "called": true,
  "profile": "cursor-sonnet-5-classifier",
  "classifierModel": "sonnet-5",
  "agentModel": "claude-sonnet-5-thinking-max",
  "promptTokens": 80,
  "completionTokens": 2,
  "totalTokens": 82,
  "composerTokens": 82,
  "localTokens": 0,
  "warning": null
}
```
