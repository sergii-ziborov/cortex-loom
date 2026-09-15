#!/usr/bin/env python3
"""Call the two-tool Cortex agent profile over stdio for coding benchmarks."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, TextIO

PROTOCOL_VERSION = "2025-11-25"


def send(stream: TextIO, message: dict[str, Any]) -> None:
    stream.write(json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n")
    stream.flush()


def receive(stream: TextIO, request_id: int) -> dict[str, Any]:
    while line := stream.readline():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("id") == request_id:
            return message
    raise RuntimeError(f"Cortex MCP closed before replying to request {request_id}")


def result_text(message: dict[str, Any]) -> dict[str, Any]:
    if "error" in message:
        raise RuntimeError(json.dumps(message["error"], ensure_ascii=False))
    result = message.get("result", {})
    if result.get("isError"):
        content = result.get("content", [])
        detail = content[0].get("text", "unknown Cortex MCP error") if content else result
        raise RuntimeError(str(detail))
    content = result.get("content", [])
    if not content or not isinstance(content[0].get("text"), str):
        raise RuntimeError("Cortex MCP returned no text content")
    return json.loads(content[0]["text"])


def call(
    stdin: TextIO,
    stdout: TextIO,
    request_id: int,
    name: str,
    arguments: dict[str, Any],
) -> dict[str, Any]:
    send(
        stdin,
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        },
    )
    return result_text(receive(stdout, request_id))


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--repository", type=Path, required=True)
    task_source = parser.add_mutually_exclusive_group(required=True)
    task_source.add_argument("--task-file", type=Path)
    task_source.add_argument("--task-id")
    parser.add_argument(
        "--tasks-file",
        type=Path,
        default=Path(__file__).with_name("tasks.json"),
    )
    parser.add_argument("--run-id", required=True)
    parser.add_argument(
        "--budget-class",
        choices=("auto", "tight", "normal", "wide"),
        default="normal",
    )
    parser.add_argument(
        "--expand-missing",
        action="store_true",
        help="Call cortex_expand for every handle returned by cortex_prepare.",
    )
    arguments = parser.parse_args()

    binary = arguments.binary.resolve(strict=True)
    repository = arguments.repository.resolve(strict=True)
    if arguments.task_file is not None:
        task = arguments.task_file.read_text(encoding="utf-8").strip()
    else:
        manifest = json.loads(arguments.tasks_file.read_text(encoding="utf-8"))
        task = next(
            (
                item["text"].strip()
                for item in manifest["tasks"]
                if item["id"] == arguments.task_id
            ),
            "",
        )
        if not task:
            raise ValueError(f"unknown task id: {arguments.task_id}")
    if not task:
        raise ValueError("task file is empty")

    environment = os.environ.copy()
    environment.pop("CORTEX_LLM", None)
    environment.pop("CORTEX_SEMANTIC", None)
    with tempfile.TemporaryDirectory(prefix="cortex-agent-mcp-") as temporary:
        environment["CORTEX_LOOM_DB"] = str(Path(temporary) / "cortex-loom.db")
        process = subprocess.Popen(
            [
                str(binary),
                "--profile",
                "agent",
                "--workspace",
                str(repository),
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
            bufsize=1,
            env=environment,
        )
        if process.stdin is None or process.stdout is None:
            raise RuntimeError("failed to open Cortex MCP stdio")
        try:
            send(
                process.stdin,
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": PROTOCOL_VERSION,
                        "capabilities": {},
                        "clientInfo": {
                            "name": "cortex-coding-agent-bench",
                            "version": "1",
                        },
                    },
                },
            )
            initialized = receive(process.stdout, 1)
            if "error" in initialized:
                raise RuntimeError(json.dumps(initialized["error"], ensure_ascii=False))
            send(
                process.stdin,
                {
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {},
                },
            )
            prepared = call(
                process.stdin,
                process.stdout,
                2,
                "cortex_prepare",
                {
                    "repository": str(repository),
                    "task": task,
                    "runId": arguments.run_id,
                    "budgetClass": arguments.budget_class,
                },
            )
            expansions = []
            if arguments.expand_missing:
                handles = prepared.get("expansionHandles", [])
                for index, handle in enumerate(handles, start=3):
                    facet = handle.get("facet")
                    if not isinstance(facet, str):
                        continue
                    expansions.append(
                        {
                            "facet": facet,
                            "result": call(
                                process.stdin,
                                process.stdout,
                                index,
                                "cortex_expand",
                                {
                                    "packetId": prepared["packetId"],
                                    "facet": facet,
                                },
                            ),
                        }
                    )
            json.dump(
                {
                    "mode": "deterministic_no_models",
                    "prepare": prepared,
                    "expansions": expansions,
                },
                sys.stdout,
                ensure_ascii=False,
                indent=2,
            )
            sys.stdout.write("\n")
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=5)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
