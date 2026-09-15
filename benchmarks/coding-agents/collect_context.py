#!/usr/bin/env python3
"""Reproduce transcript-based context estimates for the Cortex coding A/B.

Cursor subagent JSONL omits tool results and provider billing telemetry. This
collector reports only UTF-8-visible characters divided by four plus a separate
reconstructed tool-result estimate. It does not invent host tokens or label
repeated context rereads as provider spend.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any

RUN_ID_RE = re.compile(
    r"BENCH_RUN_ID:\s*(CORTEX_WITH_T[123]_(?:SOL_XHIGH|FABLE|GROK|OPUS|COMPOSER)_20260915)"
)
FINAL_ID_RE = re.compile(
    r"CORTEX_WITH_T[123]_(?:SOL_XHIGH|FABLE|GROK|OPUS|COMPOSER)_20260915"
)
MODEL_BY_KEY = {
    "SOL_XHIGH": "Sol 5.6 xhigh",
    "FABLE": "Fable 5.1 extra high",
    "GROK": "Grok 4.6 extra high",
    "OPUS": "Opus 5 extra high",
    "COMPOSER": "Composer 2.5",
}
def tokens(characters: int) -> int:
    return characters // 4


def json_size(value: Any) -> int:
    return len(json.dumps(value, ensure_ascii=False))


def message_size(record: dict[str, Any]) -> int:
    size = 0
    content = (record.get("message") or {}).get("content") or []
    for part in content:
        if not isinstance(part, dict):
            size += len(str(part))
        elif part.get("type") == "text":
            size += len(part.get("text") or "")
        elif part.get("type") == "tool_use":
            size += len(part.get("name") or "")
            size += json_size(part.get("input") or {})
        else:
            size += json_size(part)
    return size


def reconstructed_result_size(record: dict[str, Any]) -> int:
    """Estimate result characters because Cursor JSONL omits tool responses."""
    size = 0
    content = (record.get("message") or {}).get("content") or []
    for part in content:
        if not isinstance(part, dict) or part.get("type") != "tool_use":
            continue
        name = part.get("name") or ""
        arguments = part.get("input") or {}
        if name in {"Read", "ReadFile"}:
            path = Path(arguments.get("path") or "")
            if not path.is_file():
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            limit = arguments.get("limit")
            if isinstance(limit, int) and limit > 0:
                text = "".join(text.splitlines(keepends=True)[:limit])
            size += len(text)
        elif name == "GetDynamicTools":
            size += 8_000
        elif name == "CallDynamicTool":
            size += 3_000
        elif name in {"Grep", "rg"}:
            size += 1_500
        elif name == "Glob":
            size += 800
        elif name == "Shell":
            size += 400
        elif name in {"ApplyPatch", "StrReplace"}:
            size += 150
        else:
            size += 100
    return size


def first_user_text(path: Path) -> str:
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            record = json.loads(line)
            if record.get("role") != "user":
                continue
            return "".join(
                part.get("text", "")
                for part in (record.get("message") or {}).get("content") or []
                if isinstance(part, dict) and part.get("type") == "text"
            )
    return ""


def run_id(path: Path) -> str | None:
    match = RUN_ID_RE.search(first_user_text(path))
    return match.group(1) if match else None


def analyze(path: Path) -> dict[str, Any]:
    prior = 0
    generated = 0
    cumulative_read = 0
    peak = 0
    result_characters = 0
    assistant_turns = 0
    tool_calls = 0
    initial_request = 0
    user_characters = 0
    has_final_report = False

    with path.open(encoding="utf-8") as stream:
        for line in stream:
            record = json.loads(line)
            role = record.get("role")
            size = message_size(record)
            if role == "user":
                if initial_request == 0:
                    initial_request = size
                user_characters += size
                prior += size
                peak = max(peak, prior)
                continue
            if role != "assistant":
                continue

            assistant_turns += 1
            cumulative_read += prior
            generated += size
            content = (record.get("message") or {}).get("content") or []
            tool_calls += sum(
                isinstance(part, dict) and part.get("type") == "tool_use"
                for part in content
            )
            has_tool_call = any(
                isinstance(part, dict) and part.get("type") == "tool_use"
                for part in content
            )
            has_final_report = has_final_report or (
                not has_tool_call
                and any(
                isinstance(part, dict)
                and part.get("type") == "text"
                and FINAL_ID_RE.search(part.get("text") or "")
                for part in content
                )
            )
            result_size = reconstructed_result_size(record)
            result_characters += result_size
            prior += size + result_size
            peak = max(peak, prior)
            if has_final_report:
                break

    response_tokens = tokens(generated)
    return {
        "initial_request_tok": tokens(initial_request),
        "visible_request_tok": tokens(user_characters),
        "visible_response_tok": response_tokens,
        "visible_transcript_tok": tokens(user_characters + generated),
        "estimated_context_material_tok": tokens(peak),
        "assistant_turns": assistant_turns,
        "tool_calls": tool_calls,
        "reconstructed_result_tok": tokens(result_characters),
        "elapsed_seconds": round(path.stat().st_mtime - path.stat().st_ctime, 3),
        "has_final_report": has_final_report,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--transcripts", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()

    rows: list[dict[str, Any]] = []
    for path in sorted(arguments.transcripts.glob("*.jsonl")):
        identifier = run_id(path)
        if identifier is None:
            continue
        task = identifier.split("_")[2]
        model_key = identifier.removesuffix("_20260915").split("_", 3)[3]
        row = {
            "run_id": identifier,
            "task": task,
            "model": MODEL_BY_KEY[model_key],
            "agent_id": path.stem,
        }
        row.update(analyze(path))
        rows.append(row)

    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(
        json.dumps(rows, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {len(rows)} rows to {arguments.output}")


if __name__ == "__main__":
    main()
