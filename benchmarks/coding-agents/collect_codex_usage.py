#!/usr/bin/env python3
"""Summarize complete Codex CLI coding runs from provider usage receipts.

Each ``turn.completed`` is a real usage record. Cached input remains part of
gross input; the separate fresh view subtracts it once. Tool output characters
are diagnostics, never converted into provider tokens.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def analyze(path: Path) -> dict[str, Any]:
    totals = {"input_tokens": 0, "cached_input_tokens": 0, "output_tokens": 0}
    turns = shell_calls = mcp_calls = shell_chars = mcp_chars = 0
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            record = json.loads(line)
            if record.get("type") == "turn.completed":
                usage = record.get("usage") or {}
                for key in totals:
                    value = usage.get(key)
                    if not isinstance(value, int) or value < 0:
                        raise ValueError(f"{path}: invalid {key} in turn.completed")
                    totals[key] += value
                turns += 1
            if record.get("type") != "item.completed":
                continue
            item = record.get("item") or {}
            if item.get("type") == "command_execution":
                shell_calls += 1
                shell_chars += len(item.get("aggregated_output") or "")
            elif item.get("type") == "mcp_tool_call":
                mcp_calls += 1
                mcp_chars += len(json.dumps(item.get("result"), ensure_ascii=False))
    if turns == 0:
        raise ValueError(f"{path}: no completed turn; do not score an aborted run")
    if totals["cached_input_tokens"] > totals["input_tokens"]:
        raise ValueError(f"{path}: cached input exceeds total input")
    gross = totals["input_tokens"] + totals["output_tokens"]
    fresh = gross - totals["cached_input_tokens"]
    return {
        "path": str(path),
        "completedTurns": turns,
        "inputTokens": totals["input_tokens"],
        "cachedInputTokens": totals["cached_input_tokens"],
        "outputTokens": totals["output_tokens"],
        "grossTokens": gross,
        "uncachedInputPlusOutputTokens": fresh,
        "shellCalls": shell_calls,
        "mcpCalls": mcp_calls,
        "shellResultCharacters": shell_chars,
        "mcpResultCharacters": mcp_chars,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("logs", nargs="+", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    rows = [analyze(path) for path in arguments.logs]
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(
        json.dumps(rows, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
