"""Turn the Cortex train split into chat records for a micro_extract LoRA.

Training-only. Nothing here runs on a Cortex request path.

Two invariants this script exists to enforce:

1. **The prompt is the serving prompt.** The system text is read out of
   `crates/cortex-eval/src/prompts.rs` rather than copied, and the user turn is
   rendered exactly as `micro_extraction_request` renders it. A model tuned on
   a paraphrase of its own serving prompt is a different model at serving time.

2. **The holdout stays out.** Rows are refused unless they are
   `targetRole=micro_extract` and `split=train`, and any row whose verified
   input appears in `fixtures/micro-extraction.json` aborts the run. The gate
   is only worth measuring if the model has not seen the answers.

Usage:
    python prepare.py [--repo DIR] [--out DIR] [--dev-fraction 0.1]
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import re
import sys
from pathlib import Path

CORPUS = "corpora/micro-extract-train.jsonl"
HOLDOUT = "crates/cortex-eval/fixtures/micro-extraction.json"
PROMPTS = "crates/cortex-eval/src/prompts.rs"

EXTRACTION_TASK = "micro-extraction"
REJECT_TASK = "micro-extraction-reject"

# Superpowers is a measured baseline, never a training source. `Done means` in
# docs/fine-tune.md asks for this to be provably absent from training files.
FORBIDDEN = ("using-superpowers", "obra/superpowers")


def read_serving_system(repo: Path) -> str:
    """Extract MICRO_EXTRACTION_SYSTEM from prompts.rs so it cannot drift."""
    source = (repo / PROMPTS).read_text(encoding="utf-8")
    match = re.search(
        r'pub const MICRO_EXTRACTION_SYSTEM: &str = "(?P<text>(?:[^"\\]|\\.)*)";',
        source,
    )
    if not match:
        raise SystemExit(f"MICRO_EXTRACTION_SYSTEM not found in {PROMPTS}")
    return (
        match.group("text")
        .replace('\\"', '"')
        .replace("\\n", "\n")
        .replace("\\\\", "\\")
    )


def read_jsonl(path: Path) -> list[dict]:
    with io.open(path, encoding="utf-8") as handle:
        return [json.loads(line) for line in handle if line.strip()]


def split_corpus_input(text: str) -> tuple[str, str]:
    """Parse `allowedFields: a, b\\nverifiedInput:\\n...` into its two parts."""
    head, _, rest = text.partition("\n")
    if not head.startswith("allowedFields: "):
        raise SystemExit(f"unexpected corpus input head: {head!r}")
    marker, _, verified = rest.partition("\n")
    if marker != "verifiedInput:":
        raise SystemExit(f"unexpected corpus input marker: {marker!r}")
    return head[len("allowedFields: ") :], verified


def serving_user_turn(allowed: str, verified: str) -> str:
    """Byte-identical to `micro_extraction_request`'s user message."""
    return f"Allowed fields: {allowed}\n\nVerified evidence:\n{verified}"


def to_messages(record: dict, serving_system: str) -> dict:
    if record["task"] == EXTRACTION_TASK:
        allowed, verified = split_corpus_input(record["input"])
        system, user = serving_system, serving_user_turn(allowed, verified)
    elif record["task"] == REJECT_TASK:
        # Judge rows keep their own system turn. The serving prompt never asks
        # for a reject verdict, so it must never be paired with one.
        system, user = record["instruction"], record["input"]
    else:
        raise SystemExit(f"{record['id']}: unexpected task {record['task']}")
    return {
        "id": record["id"],
        "task": record["task"],
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
            {"role": "assistant", "content": record["output"]},
        ],
    }


def guard(records: list[dict], holdout: list[dict]) -> None:
    holdout_inputs = [fixture["verifiedInput"] for fixture in holdout]
    for record in records:
        if record.get("split") != "train":
            raise SystemExit(f"{record['id']}: split is {record.get('split')!r}, not train")
        if record.get("targetRole") != "micro_extract":
            raise SystemExit(f"{record['id']}: role is {record.get('targetRole')!r}")
        if record.get("trainingSource") != "cortex-original":
            raise SystemExit(f"{record['id']}: trainingSource is not cortex-original")
        blob = f"{record['instruction']}\n{record['input']}\n{record['output']}"
        lowered = blob.lower()
        for marker in FORBIDDEN:
            if marker in lowered:
                raise SystemExit(f"{record['id']}: upstream skill text leaked ({marker})")
        for verified in holdout_inputs:
            if verified in record["input"]:
                raise SystemExit(f"{record['id']}: carries a holdout verified input")


def is_dev(identifier: str, fraction: float) -> bool:
    """Deterministic split: the same id lands in the same half every run."""
    digest = hashlib.sha256(identifier.encode("utf-8")).digest()
    return (int.from_bytes(digest[:4], "big") % 10_000) < fraction * 10_000


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with io.open(path, "w", encoding="utf-8", newline="\n") as handle:
        for row in rows:
            handle.write(json.dumps(row, ensure_ascii=False) + "\n")


def main() -> int:
    repo_default = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=repo_default)
    parser.add_argument("--out", type=Path, default=None)
    parser.add_argument("--dev-fraction", type=float, default=0.1)
    args = parser.parse_args()

    repo: Path = args.repo.resolve()
    out: Path = (args.out or repo / ".cortex-loom/fine-tune/data").resolve()

    corpus_path = repo / CORPUS
    if not corpus_path.exists():
        raise SystemExit(
            f"{CORPUS} is missing; run `cargo run -p cortex-eval -- corpus` first"
        )
    records = read_jsonl(corpus_path)
    holdout = json.loads((repo / HOLDOUT).read_text(encoding="utf-8"))
    guard(records, holdout)

    serving_system = read_serving_system(repo)
    rows = [to_messages(record, serving_system) for record in records]
    dev = [row for row in rows if is_dev(row["id"], args.dev_fraction)]
    train = [row for row in rows if not is_dev(row["id"], args.dev_fraction)]

    write_jsonl(out / "train.jsonl", train)
    write_jsonl(out / "dev.jsonl", dev)
    write_jsonl(
        out / "holdout-preview.jsonl",
        [
            {
                "id": fixture["id"],
                "messages": [
                    {"role": "system", "content": serving_system},
                    {
                        "role": "user",
                        "content": serving_user_turn(
                            ", ".join(sorted(fixture["allowedFields"])),
                            fixture["verifiedInput"],
                        ),
                    },
                    {
                        "role": "assistant",
                        "content": json.dumps(fixture["gold"], ensure_ascii=False, separators=(",", ":")),
                    },
                ],
            }
            for fixture in holdout
        ],
    )

    counts = {task: sum(1 for row in rows if row["task"] == task) for task in {r["task"] for r in rows}}
    print(f"prepared {len(train)} train / {len(dev)} dev rows -> {out}")
    print(f"by task: {counts}")
    print(f"holdout kept out: {len(holdout)} fixtures (preview written, never trained)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
