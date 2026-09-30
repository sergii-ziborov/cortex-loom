#!/usr/bin/env python3
"""Benchmark-only loopback proxy for cursor-agent classification.

Binds 127.0.0.1:8787 by default (`CORTEX_COMPOSER_PORT`). Used by
CORTEX_LLM_BACKEND=composer.
The request `model` field selects composer, sonnet-5, opus-5, or haiku.
"""

from __future__ import annotations

import json
import os
import hmac
import shutil
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HOST = "127.0.0.1"
PORT = int(os.environ.get("CORTEX_COMPOSER_PORT", "8787"))
AGENT = os.environ.get("CORTEX_COMPOSER_AGENT") or shutil.which("agent") or "agent"
API_KEY = os.environ.get("CORTEX_COMPOSER_API_KEY")
MAX_REQUEST_BYTES = 65_536
JOBS = threading.BoundedSemaphore(1)
LABELS = frozenset(("none", "local_small", "local_medium", "upstream_strong"))
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


class ClassifierFailure(Exception):
    """A CLI failure or invalid answer; the caller owns lexical fallback."""


def classify(prompt: str, agent_model: str) -> str:
    boxed = (
        "Reply with only one label and nothing else: "
        "none, local_small, local_medium, or upstream_strong.\n\n"
        + prompt
    )
    if not JOBS.acquire(blocking=False):
        raise ClassifierFailure("classifier busy")
    try:
        with tempfile.TemporaryDirectory(prefix="cortex-classifier-proxy-") as temporary:
            argv = [
                AGENT, "--print", "--mode", "ask", "--trust",
                "--model", agent_model, "--output-format", "text",
                "--workspace", str(Path(temporary)), boxed,
            ]
            completed = subprocess.run(
                argv, check=False, capture_output=True, text=True,
                encoding="utf-8", timeout=180,
            )
        if completed.returncode != 0:
            raise ClassifierFailure(f"classifier CLI exited with status {completed.returncode}")
        answer = (completed.stdout or "").strip().strip("`").strip()
        if answer not in LABELS:
            raise ClassifierFailure("classifier CLI returned no valid label")
        return answer
    finally:
        JOBS.release()


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
        if API_KEY and not hmac.compare_digest(
            self.headers.get("authorization", ""), f"Bearer {API_KEY}"
        ):
            self._send(401, {"error": {"message": "unauthorized"}})
            return
        try:
            length = int(self.headers.get("content-length", "0"))
        except ValueError:
            self._send(400, {"error": {"message": "invalid content length"}})
            return
        if not 0 < length <= MAX_REQUEST_BYTES:
            self._send(413, {"error": {"message": "request exceeds classifier limit"}})
            return
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
        # The CLI has no generation-token control. Only the fixed classifier
        # contract is accepted; this is an output-validation ceiling, not a
        # claim about the CLI's hidden token spend.
        if request.get("max_tokens") != 16:
            self._send(400, {"error": {"message": "only max_tokens=16 classifier requests are supported"}})
            return
        messages = request.get("messages") or []
        prompt = "\n\n".join(
            str(item.get("content", "")) for item in messages if isinstance(item, dict)
        )
        try:
            text = classify(prompt, agent_model)
        except ClassifierFailure as error:
            self._send(503 if str(error) == "classifier busy" else 502, {"error": {"message": str(error)}})
            return
        except subprocess.TimeoutExpired:
            self._send(504, {"error": {"message": "classifier CLI timed out"}})
            return
        except Exception as error:
            self._send(500, {"error": {"message": str(error)}})
            return
        self._send(
            200,
            {
                "id": "chatcmpl-cortex-classifier",
                "object": "chat.completion",
                "model": alias,
                "effectiveModel": agent_model,
                "usageKind": "unknown",
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": text},
                        "finish_reason": "stop",
                    }
                ],
            },
        )


def main() -> None:
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"classifier proxy listening on http://{HOST}:{PORT}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
