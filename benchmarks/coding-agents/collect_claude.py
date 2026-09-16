#!/usr/bin/env python3
"""Collect Claude Code CLI print-mode JSON. Do not mix with Cursor chars÷4."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def summarize(path: Path) -> dict[str, object]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    usage = payload.get("usage") or {}
    input_tokens = int(usage.get("input_tokens") or 0)
    output_tokens = int(usage.get("output_tokens") or 0)
    cache_read = int(usage.get("cache_read_input_tokens") or 0)
    cache_create = int(usage.get("cache_creation_input_tokens") or 0)
    # Cache reads are replay, not new tokens. Do not add them to spend.
    spend = input_tokens + output_tokens + cache_create
    return {
        "host": "claude-code-cli",
        "file": str(path),
        "ok": payload.get("type") == "result" and not payload.get("is_error"),
        "spend": spend,
        "inputTokens": input_tokens,
        "outputTokens": output_tokens,
        "cacheReadTokens": cache_read,
        "cacheCreateTokens": cache_create,
        "cycles": payload.get("num_turns"),
        "wallMs": payload.get("duration_ms"),
        "costUsd": payload.get("total_cost_usd"),
        "sessionId": payload.get("session_id"),
        "resultHead": str(payload.get("result") or "")[:240],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("logs", nargs="+", type=Path)
    args = parser.parse_args()
    rows = [summarize(path) for path in args.logs if path.is_file()]
    print(json.dumps(rows, indent=2))


if __name__ == "__main__":
    main()
