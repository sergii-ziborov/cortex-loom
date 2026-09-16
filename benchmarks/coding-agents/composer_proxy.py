#!/usr/bin/env python3
"""Loopback OpenAI-compatible proxy: cursor-agent classifiers.

Binds 127.0.0.1:8787 by default (`CORTEX_COMPOSER_PORT`). Used by
CORTEX_LLM_BACKEND=composer.
The request `model` field selects composer, sonnet-5, opus-5, or haiku.
"""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HOST = "127.0.0.1"
PORT = int(os.environ.get("CORTEX_COMPOSER_PORT", "8787"))
AGENT = os.environ.get(
    "CORTEX_COMPOSER_AGENT",
    r"C:\Users\SergiiZiborov\AppData\Local\cursor-agent\agent.cmd",
)
ALIASES = {
    "composer": "composer-2.5",
    "composer-2.5": "composer-2.5",
    "sonnet-5": "claude-sonnet-5-thinking-max",
    "sonnet": "claude-sonnet-5-thinking-max",
    "opus-5": "claude-opus-5-thinking-high",
    "opus": "claude-opus-5-thinking-high",
    "haiku": "claude-haiku-4-5",
}
REMAP = {
    "claude-sonnet-5-thinking-high": "claude-sonnet-5-thinking-max",
    "claude-sonnet-5-thinking-xhigh": "claude-sonnet-5-thinking-max",
    "claude-haiku-4-5[effort=high]": "claude-haiku-4-5",
}


def resolve_model(requested: str | None) -> tuple[str, str]:
    raw = (requested or os.environ.get("CORTEX_CLASSIFIER_MODEL") or "composer").strip()
    if not raw:
        raw = "composer"
    key = raw.lower()
    if key in ALIASES:
        return key, ALIASES[key]
    agent = REMAP.get(raw, raw)
    if raw.startswith("claude-") or raw.startswith("composer-"):
        return raw, agent
    raise ValueError(f"unknown classifier model: {raw}")


def classify(prompt: str, agent_model: str) -> tuple[str, int, int]:
    workspace = Path(tempfile.mkdtemp(prefix="cortex-classifier-proxy-"))
    boxed = (
        "Reply with only one label and nothing else: "
        "none, local_small, local_medium, or upstream_strong.\n\n"
        + prompt
    )
    argv = [
        AGENT,
        "--print",
        "--trust",
        "--model",
        agent_model,
        "--output-format",
        "text",
        "--workspace",
        str(workspace),
        boxed,
    ]
    if "haiku" not in agent_model:
        argv[2:2] = ["--mode", "ask"]
    completed = subprocess.run(
        argv,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=180,
    )
    text = (completed.stdout or "").strip() or (completed.stderr or "").strip()
    if completed.returncode != 0 and not text:
        text = f"upstream_strong\nproxy error: {agent_model} exit {completed.returncode}"
    if not text:
        text = "upstream_strong"
    prompt_tokens = max(1, len(prompt) // 4)
    completion_tokens = max(1, len(text) // 4)
    labels = ("none", "local_small", "local_medium", "upstream_strong")
    if not any(token in text.replace("`", " ").split() for token in labels):
        text = "upstream_strong"
    return text, prompt_tokens, completion_tokens


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: object) -> None:
        return

    def _send(self, status: int, payload: dict) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        if self.path.rstrip("/") in {"/v1/models", "/models"}:
            self._send(
                200,
                {
                    "object": "list",
                    "data": [
                        {"id": alias, "object": "model", "owned_by": "cursor-agent"}
                        for alias in ("composer", "sonnet-5", "opus-5", "haiku")
                    ],
                },
            )
            return
        self._send(404, {"error": {"message": "not found"}})

    def do_POST(self) -> None:
        if self.path.rstrip("/") not in {"/v1/chat/completions", "/chat/completions"}:
            self._send(404, {"error": {"message": "not found"}})
            return
        length = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(length)
        try:
            request = json.loads(raw.decode("utf-8"))
        except json.JSONDecodeError:
            self._send(400, {"error": {"message": "invalid json"}})
            return
        try:
            alias, agent_model = resolve_model(
                request.get("model") if isinstance(request.get("model"), str) else None
            )
        except ValueError as error:
            self._send(400, {"error": {"message": str(error)}})
            return
        messages = request.get("messages") or []
        prompt = "\n\n".join(
            str(item.get("content", "")) for item in messages if isinstance(item, dict)
        )
        try:
            text, prompt_tokens, completion_tokens = classify(prompt, agent_model)
        except Exception as error:
            self._send(500, {"error": {"message": str(error)}})
            return
        self._send(
            200,
            {
                "id": "chatcmpl-cortex-classifier",
                "object": "chat.completion",
                "model": alias,
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": text},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "total_tokens": prompt_tokens + completion_tokens,
                },
            },
        )


def main() -> None:
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"classifier proxy listening on http://{HOST}:{PORT}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
