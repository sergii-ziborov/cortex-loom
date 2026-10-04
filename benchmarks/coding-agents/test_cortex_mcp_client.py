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

    def test_local_lane_requires_the_qwen_classifier(self) -> None:
        internal = {
            "mode": "local", "called": True, "succeeded": True,
            "role": "routing_classifier", "agentModel": "qwen3-8b",
        }
        assert_effective_lane({"internalModel": internal}, "local", "composer")
        with self.assertRaises(RuntimeError):
            assert_effective_lane(
                {"internalModel": {**internal, "agentModel": "other"}},
                "local", "composer",
            )

    def test_coding_route_may_skip_a_configured_model_without_claiming_use(self) -> None:
        skipped = {
            "internalModel": {"mode": "local", "called": False,
                              "skipReason": "lexical_floor_upstream_strong"},
            "routing": {"modelTier": "upstream_strong"},
        }
        with self.assertRaises(RuntimeError):
            assert_effective_lane(skipped, "local", "composer")
        assert_effective_lane(skipped, "local", "composer", allow_policy_skip=True)
        skipped["routing"]["modelTier"] = "local_medium"
        with self.assertRaises(RuntimeError):
            assert_effective_lane(skipped, "local", "composer", allow_policy_skip=True)


if __name__ == "__main__":
    unittest.main()
