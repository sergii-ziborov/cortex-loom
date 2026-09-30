"""Offline contract tests for the benchmark classifier proxy."""

import io
import json
import subprocess
import unittest
from pathlib import Path
from unittest import mock

import composer_proxy as proxy


class ProxyTests(unittest.TestCase):
    def post(self, request: dict, *, key: str | None = None) -> tuple[int, dict]:
        raw = json.dumps(request).encode()
        handler = proxy.Handler.__new__(proxy.Handler)
        handler.path = "/v1/chat/completions"
        handler.headers = {"content-length": str(len(raw))}
        if key is not None:
            handler.headers["authorization"] = f"Bearer {key}"
        handler.rfile = io.BytesIO(raw)
        response = []
        handler._send = lambda status, payload: response.append((status, payload))
        handler.do_POST()
        return response[0]

    @staticmethod
    def request(max_tokens: int = 16) -> dict:
        return {
            "model": "composer",
            "max_tokens": max_tokens,
            "messages": [{"role": "user", "content": "classify this"}],
        }

    def test_nonzero_exit_never_becomes_a_successful_label(self) -> None:
        for stdout, stderr in [("", "429 rate limit upstream_strong"), ("none", "failed")]:
            with self.subTest(stdout=stdout), mock.patch.object(
                proxy.subprocess, "run",
                return_value=subprocess.CompletedProcess([], 1, stdout, stderr),
            ):
                with self.assertRaises(proxy.ClassifierFailure):
                    proxy.classify("task", "composer-2.5")

    def test_success_omits_unknown_usage_and_cleans_workspace(self) -> None:
        workspaces = []

        def fake_run(argv: list[str], **_: object) -> subprocess.CompletedProcess:
            workspaces.append(Path(argv[argv.index("--workspace") + 1]))
            self.assertTrue(workspaces[-1].exists())
            self.assertIn("--mode", argv)
            return subprocess.CompletedProcess(argv, 0, "none\n", "")

        with mock.patch.object(proxy.subprocess, "run", side_effect=fake_run):
            status, payload = self.post(self.request())
        self.assertEqual(status, 200)
        self.assertEqual(payload["choices"][0]["message"]["content"], "none")
        self.assertEqual(payload["usageKind"], "unknown")
        self.assertNotIn("usage", payload)
        self.assertFalse(workspaces[0].exists())

    def test_unsupported_output_limit_does_not_call_model(self) -> None:
        with mock.patch.object(proxy, "classify") as classify:
            for limit in (1, 1000):
                self.assertEqual(self.post(self.request(limit))[0], 400)
            classify.assert_not_called()

    def test_configured_key_is_checked(self) -> None:
        with mock.patch.object(proxy, "API_KEY", "secret"), mock.patch.object(
            proxy, "classify", return_value="none"
        ) as classify:
            self.assertEqual(self.post(self.request())[0], 401)
            self.assertEqual(self.post(self.request(), key="wrong")[0], 401)
            self.assertEqual(self.post(self.request(), key="secret")[0], 200)
            classify.assert_called_once()


if __name__ == "__main__":
    unittest.main()
