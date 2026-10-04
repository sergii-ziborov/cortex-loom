import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location(
    "collect_codex_usage", Path(__file__).with_name("collect_codex_usage.py")
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class CompleteUsageTests(unittest.TestCase):
    def test_sums_every_completed_turn_and_keeps_cached_input_separate(self) -> None:
        records = [
            {"type": "turn.completed", "usage": {
                "input_tokens": 100, "cached_input_tokens": 20, "output_tokens": 10}},
            {"type": "item.completed", "item": {
                "type": "command_execution", "aggregated_output": "abcd"}},
            {"type": "turn.completed", "usage": {
                "input_tokens": 200, "cached_input_tokens": 150, "output_tokens": 5}},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "run.jsonl"
            path.write_text("\n".join(json.dumps(row) for row in records) + "\n")
            row = MODULE.analyze(path)
        self.assertEqual(row["completedTurns"], 2)
        self.assertEqual(row["grossTokens"], 315)
        self.assertEqual(row["uncachedInputPlusOutputTokens"], 145)
        self.assertEqual(row["shellResultCharacters"], 4)

    def test_aborted_run_cannot_be_scored(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "aborted.jsonl"
            path.write_text('{"type":"turn.started"}\n')
            with self.assertRaisesRegex(ValueError, "no completed turn"):
                MODULE.analyze(path)


if __name__ == "__main__":
    unittest.main()
