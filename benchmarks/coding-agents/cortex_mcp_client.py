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


def is_qwen3_8b(model: Any) -> bool:
    """Accept native Ollama and configured OVMS spellings of Qwen3 8B."""
    return isinstance(model, str) and model.lower().startswith("qwen3") and "8b" in model.lower()


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


def assert_effective_lane(
    prepared: dict[str, Any], backend: str, model: str, *, allow_policy_skip: bool = False,
) -> None:
    internal = prepared.get("internalModel") or {}
    draft = prepared.get("codingDraft") or {}
    effective = internal.get("mode")
    if effective != backend:
        raise RuntimeError(f"requested {backend} lane, Cortex reported {effective!r}")
    if backend == "off":
        if internal.get("called") or draft.get("called"):
            raise RuntimeError("models-off lane invoked a model")
        return
    if prepared.get("mutationLikely"):
        if draft.get("mode") != backend or not draft.get("called"):
            raise RuntimeError(
                f"{backend} coding model did not run: {draft.get('reason') or draft.get('status')}"
            )
        if backend == "local" and not is_qwen3_8b(draft.get("model")):
            raise RuntimeError("local coding lane did not use the configured Qwen3-8B")
        return
    if not internal.get("called") and allow_policy_skip:
        if (
            internal.get("skipReason") == "lexical_floor_upstream_strong"
            and prepared.get("routing", {}).get("modelTier") == "upstream_strong"
        ):
            return
        raise RuntimeError("classifier was skipped without an upstream routing ceiling")
    if not internal.get("called") or not internal.get("succeeded"):
        raise RuntimeError(
            f"{backend} classifier did not succeed: {internal.get('warning') or 'no result'}"
        )
    if backend == "local" and (
        internal.get("role") != "routing_classifier"
        or not is_qwen3_8b(internal.get("agentModel"))
    ):
        raise RuntimeError("local benchmark did not report the configured Qwen3-8B classifier")
    if backend == "composer" and internal.get("classifierModel") != model:
        raise RuntimeError("composer classifier alias differed from the requested model")


def model_tokens(prepared: dict[str, Any]) -> int | None:
    """Sum internal usage only when every invoked model reported it."""
    total = 0
    for field in ("internalModel", "codingDraft"):
        result = prepared.get(field) or {}
        if not result.get("called"):
            continue
        usage = result.get("totalTokens")
        if not isinstance(usage, int):
            return None
        total += usage
    return total


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
    parser.add_argument(
        "--llm-backend",
        choices=("off", "local", "composer"),
        default="off",
        help="Cortex internal model: off, local Qwen, or a Composer loopback proxy.",
    )
    parser.add_argument(
        "--classifier-model",
        choices=("composer", "sonnet-5", "opus-5", "haiku"),
        default="composer",
        help="Loopback proxy model when --llm-backend=composer.",
    )
    parser.add_argument(
        "--allow-policy-skip",
        action="store_true",
        help="Allow classifier policy skip for non-coding tasks; coding lanes still require an actual model call.",
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
    environment.pop("CORTEX_SEMANTIC", None)
    for key in list(environment):
        if key.startswith("CORTEX_SHADOW"):
            environment.pop(key)
    profiles = Path(__file__).resolve().parents[2] / "config" / "llm-profiles.json"
    if arguments.llm_backend == "off":
        environment.pop("CORTEX_LLM", None)
        environment["CORTEX_LLM_BACKEND"] = "off"
    elif arguments.llm_backend == "local":
        environment["CORTEX_LLM"] = "1"
        environment["CORTEX_LLM_BACKEND"] = "local"
        environment["CORTEX_LLM_PROFILES"] = str(profiles)
    else:
        environment.pop("CORTEX_LLM", None)
        environment["CORTEX_LLM_BACKEND"] = "composer"
        environment.setdefault("CORTEX_COMPOSER_BASE_URL", "http://127.0.0.1:8787")
        environment["CORTEX_CLASSIFIER_MODEL"] = arguments.classifier_model
        environment["CORTEX_COMPOSER_MODEL"] = arguments.classifier_model
    with tempfile.TemporaryDirectory(prefix="cortex-agent-mcp-") as temporary:
        environment["CORTEX_LOOM_DB"] = str(Path(temporary) / "cortex-loom.db")
        stderr_file = tempfile.TemporaryFile(mode="w+t", encoding="utf-8")
        try:
            process = subprocess.Popen(
                [str(binary), "--profile", "agent", "--workspace", str(repository)],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr_file,
                text=True, encoding="utf-8", bufsize=1, env=environment,
            )
        except Exception:
            stderr_file.close()
            raise
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
            prepare_arguments = {
                "repository": str(repository),
                "task": task,
                "runId": arguments.run_id,
                "budgetClass": arguments.budget_class,
            }
            if arguments.llm_backend == "composer":
                prepare_arguments["classifierModel"] = arguments.classifier_model
            prepared = call(
                process.stdin,
                process.stdout,
                2,
                "cortex_prepare",
                prepare_arguments,
            )
            assert_effective_lane(
                prepared, arguments.llm_backend, arguments.classifier_model,
                allow_policy_skip=arguments.allow_policy_skip,
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
                    "mode": f"cortex_{arguments.llm_backend}",
                    "effectiveBackend": prepared["internalModel"]["mode"],
                    "modelUsed": bool(
                        prepared["internalModel"]["called"]
                        or prepared.get("codingDraft", {}).get("called")
                    ),
                    "draftAccepted": bool(prepared.get("codingDraft", {}).get("accepted")),
                    "modelTokens": model_tokens(prepared),
                    "semanticEnabled": False,
                    "shadowEnabled": False,
                    "priorRunMemoryAvailable": False,
                    "prepare": prepared,
                    "expansions": expansions,
                },
                sys.stdout,
                ensure_ascii=False,
                indent=2,
            )
            sys.stdout.write("\n")
        except Exception as error:
            stderr_file.flush()
            stderr_file.seek(0)
            detail = stderr_file.read()[-4000:].strip()
            if detail:
                raise RuntimeError(f"{error}; Cortex stderr: {detail}") from error
            raise
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=5)
            stderr_file.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
