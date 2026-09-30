"""Offline checks that benchmark lane labels reflect actual MCP results."""

import unittest

from cortex_mcp_client import assert_effective_lane


class LaneTests(unittest.TestCase):
    def test_fallback_is_not_scored_as_a_successful_model_lane(self) -> None:
        for internal in (
            {"mode": "off", "called": False},
            {"mode": "composer", "called": True, "succeeded": False, "warning": "429"},
        ):
            with self.subTest(internal=internal), self.assertRaises(RuntimeError):
                assert_effective_lane({"internalModel": internal}, "composer", "composer")

    def test_explicit_success_and_models_off(self) -> None:
        assert_effective_lane({"internalModel": {"mode": "off", "called": False}}, "off", "composer")
        assert_effective_lane(
            {"internalModel": {
                "mode": "composer", "called": True, "succeeded": True,
                "classifierModel": "sonnet-5",
            }},
            "composer", "sonnet-5",
        )


if __name__ == "__main__":
    unittest.main()
