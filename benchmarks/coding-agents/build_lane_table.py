#!/usr/bin/env python3
"""Build the requested five-lane spend table after new transcripts exist."""

from __future__ import annotations

import json
from pathlib import Path

from collect_context import MODEL_BY_KEY, analyze, run_id

WITHOUT = {
    ("Grok 4.6 extra high", "T1"): (809468, 9.4),
    ("Grok 4.6 extra high", "T2"): (302751, 8.5),
    ("Grok 4.6 extra high", "T3"): (292933, 9.2),
    ("Composer 2.5", "T1"): (453500, 8.2),
    ("Composer 2.5", "T2"): (304010, 8.8),
    ("Composer 2.5", "T3"): (264368, 8.4),
}

MODELS_OFF = {
    ("Grok 4.6 extra high", "T1"): (40403, 9.4),
    ("Grok 4.6 extra high", "T3"): (37580, 9.2),
    ("Composer 2.5", "T1"): (35083, 5.0),
    ("Composer 2.5", "T2"): (20152, 8.7),
    ("Composer 2.5", "T3"): (21602, 8.6),
}

COMPOSER_DIRECT = {
    "T1": (453500, 8.2),
    "T2": (304010, 8.8),
    "T3": (264368, 8.4),
}

LANE_PREFIX = {
    "OFF": "models_off",
    "ON": "models_on",
    "CMP": "cortex_composer_llm",
    "WITH": "models_off",
}

TRANSCRIPTS = Path(
    r"C:\Users\SergiiZiborov\.cursor\projects\c-Users-SergiiZiborov-Documents-GitHub-MyProjects-cortex-loom\agent-transcripts\d35f2884-d282-41ef-9dc1-009a22024fcb\subagents"
)


def collect_new() -> dict[tuple[str, str, str], dict]:
    rows: dict[tuple[str, str, str], dict] = {}
    if not TRANSCRIPTS.is_dir():
        return rows
    for path in TRANSCRIPTS.glob("*.jsonl"):
        identifier = run_id(path)
        if identifier is None:
            continue
        parts = identifier.split("_")
        lane = LANE_PREFIX.get(parts[1])
        if lane is None:
            continue
        task = parts[2]
        model_key = "_".join(parts[3:])
        for suffix in ("_20260915", "_20260916"):
            if model_key.endswith(suffix):
                model_key = model_key[: -len(suffix)]
                break
        model = MODEL_BY_KEY[model_key]
        measured = analyze(path)
        if not measured.get("has_final_report"):
            continue
        if measured.get("estimated_context_material_tok") is None:
            continue
        rows[(model, task, lane)] = measured
    return rows


def main() -> None:
    fresh = collect_new()
    out = []
    for model in ("Grok 4.6 extra high", "Composer 2.5"):
        for task in ("T1", "T2", "T3"):
            off = MODELS_OFF.get((model, task))
            if (model, task, "models_off") in fresh:
                item = fresh[(model, task, "models_off")]
                off = (item["total_agent_spend_tok"], None)
            on = None
            if (model, task, "models_on") in fresh:
                item = fresh[(model, task, "models_on")]
                on = item["total_agent_spend_tok"]
            cmp = None
            if (model, task, "cortex_composer_llm") in fresh:
                item = fresh[(model, task, "cortex_composer_llm")]
                cmp = item["total_agent_spend_tok"]
            out.append(
                {
                    "model": model,
                    "task": task,
                    "without_spend": WITHOUT[(model, task)][0],
                    "without_score": WITHOUT[(model, task)][1],
                    "models_off_spend": None if off is None else off[0],
                    "models_off_score": None if off is None else off[1],
                    "models_on_spend": on,
                    "cortex_composer_llm_spend": cmp,
                    "composer_direct_spend": COMPOSER_DIRECT[task][0],
                    "composer_direct_score": COMPOSER_DIRECT[task][1],
                }
            )
    path = Path(__file__).with_name("lane-table.json")
    path.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
    print(path)
    for row in out:
        print(
            f"{row['model']}|{row['task']}|{row['without_spend']}|{row['models_off_spend']}|{row['models_on_spend']}|{row['cortex_composer_llm_spend']}|{row['composer_direct_spend']}"
        )


if __name__ == "__main__":
    main()
