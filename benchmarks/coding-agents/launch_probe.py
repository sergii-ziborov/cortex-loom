#!/usr/bin/env python3
"""Probe Cortex MCP launch modes. Does not implement coding-agent tasks."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, TextIO

PROTOCOL_VERSION = "2025-11-25"
REPO = Path(r"C:\cbw-20260915\t2-composer")
BINARY = Path(r"C:\Users\SergiiZiborov\AppData\Local\CortexLoom\bin\cortex-mcp.exe")
PROFILES = Path(r"C:\Users\SergiiZiborov\Documents\GitHub\MyProjects\cortex-loom\config\llm-profiles.json")
ROOT = Path(__file__).resolve().parent


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
    raise RuntimeError("Cortex MCP closed before replying")


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
        return result
    try:
        return json.loads(content[0]["text"])
    except json.JSONDecodeError:
        return {"text": content[0]["text"]}


def handshake(stdin: TextIO, stdout: TextIO, name: str) -> None:
    send(
        stdin,
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": name, "version": "1"},
            },
        },
    )
    initialized = receive(stdout, 1)
    if "error" in initialized:
        raise RuntimeError(json.dumps(initialized["error"], ensure_ascii=False))
    send(stdin, {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})


def call(stdin: TextIO, stdout: TextIO, request_id: int, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
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


def run_server(profile: str, enable_llm: bool) -> dict[str, Any]:
    environment = os.environ.copy()
    if enable_llm:
        environment["CORTEX_LLM"] = "1"
        environment["CORTEX_LLM_PROFILES"] = str(PROFILES)
        environment.pop("CORTEX_SEMANTIC", None)
    else:
        environment.pop("CORTEX_LLM", None)
        environment.pop("CORTEX_SEMANTIC", None)
    with tempfile.TemporaryDirectory(prefix="cortex-launch-") as temporary:
        environment["CORTEX_LOOM_DB"] = str(Path(temporary) / "cortex-loom.db")
        stderr_path = Path(temporary) / "stderr.txt"
        with stderr_path.open("w", encoding="utf-8") as stderr:
            process = subprocess.Popen(
                [str(BINARY), "--profile", profile, "--workspace", str(REPO)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=stderr,
                text=True,
                encoding="utf-8",
                bufsize=1,
                env=environment,
                cwd=str(ROOT.parents[1]),
            )
        assert process.stdin is not None and process.stdout is not None
        try:
            handshake(process.stdin, process.stdout, f"cortex-launch-{profile}")
            payload: dict[str, Any] = {"profile": profile, "cortexLlm": enable_llm}
            if profile == "agent":
                payload["prepare"] = call(
                    process.stdin,
                    process.stdout,
                    2,
                    "cortex_prepare",
                    {
                        "repository": str(REPO),
                        "task": "Explain how sweeploom-ai classifies cache versus secret files. Do not edit files.",
                        "runId": "CORTEX_LAUNCH_AGENT_20260915",
                        "budgetClass": "normal",
                    },
                )
            else:
                common = {
                    "evidence": "not_required",
                    "schemaValid": True,
                    "budget": {
                        "estimatedInputTokens": 200,
                        "estimatedOutputTokens": 40,
                        "maxInputTokens": 8192,
                        "maxOutputTokens": 256,
                    },
                    "mutation": "none",
                    "availability": {"weavatrix": True, "ollama": False},
                }
                payload["routeReadOnly"] = call(
                    process.stdin,
                    process.stdout,
                    2,
                    "route_work",
                    {
                        **common,
                        "task": "Explain how sweeploom-ai classifies cache versus secret files.",
                        "runId": "CORTEX_LAUNCH_ROUTE_RO_20260915",
                    },
                )
                payload["routeMutation"] = call(
                    process.stdin,
                    process.stdout,
                    3,
                    "route_work",
                    {
                        **common,
                        "task": "Find, verify, and eliminate duplicate classifier logic in crates/sweeploom-ai.",
                        "mutation": "approval_required",
                        "runId": "CORTEX_LAUNCH_ROUTE_MUT_20260915",
                    },
                )
            payload["stderr"] = stderr_path.read_text(encoding="utf-8", errors="replace")
            return payload
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=8)


def main() -> int:
    report = {
        "binary": str(BINARY),
        "profiles": str(PROFILES),
        "agentDeterministic": run_server("agent", False),
        "agentWithLlmFlag": run_server("agent", True),
        "fullWithLlmFlag": run_server("full", True),
    }
    out = ROOT / "launch-preflight.json"
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
