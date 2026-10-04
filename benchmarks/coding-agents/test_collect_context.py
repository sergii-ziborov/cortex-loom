"""Regression for full-task versus one-context transcript estimates."""

import json
import tempfile
import unittest
from pathlib import Path

from collect_context import analyze, tokens


class CollectContextTests(unittest.TestCase):
    def test_repeated_input_is_counted_for_every_assistant_turn(self):
        final = "CORTEX_WITH_T1_GROK_20260915 finished"
        records = [
            {"role": "user", "message": {"content": [{"type": "text", "text": "x" * 400}]}},
            {"role": "assistant", "message": {"content": [{"type": "text", "text": "p" * 80}]}},
            {"role": "assistant", "message": {"content": [{"type": "text", "text": final}]}},
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "transcript.jsonl"
            path.write_text("".join(json.dumps(record) + "\n" for record in records))
            result = analyze(path)

        output = tokens(80 + len(final))
        self.assertEqual(result["estimated_peak_context_tok"], tokens(400 + 80 + len(final)))
        self.assertEqual(result["estimated_cumulative_input_tok"], tokens(400 + 480))
        self.assertEqual(result["estimated_full_task_tok"], tokens(400 + 480) + output)
        self.assertFalse(result["provider_usage_available"])
        self.assertNotIn("total_agent_spend_tok", result)

    def test_a_report_marker_does_not_drop_later_turns(self):
        records = [
            {"role": "user", "message": {"content": [{"type": "text", "text": "task"}]}},
            {
                "role": "assistant",
                "message": {"content": [{"type": "text", "text": "CORTEX_WITH_T1_GROK_20260915"}]},
            },
            {"role": "assistant", "message": {"content": [{"type": "text", "text": "checked result"}]}},
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "transcript.jsonl"
            path.write_text("".join(json.dumps(record) + "\n" for record in records))
            result = analyze(path)

        self.assertTrue(result["has_final_report"])
        self.assertEqual(result["assistant_turns"], 2)


if __name__ == "__main__":
    unittest.main()
