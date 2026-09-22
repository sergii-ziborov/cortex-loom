#!/usr/bin/env python3
"""Run empty Claude coding-agent cells on Claude Code CLI, not cursor-agent.

Host is labeled separately: spend comes from Claude JSON usage + num_turns,
not Cursor JSONL characters÷4. Existing Grok/Composer/Opus-Without cells stay.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

from launch_gap_matrix import BASE, GAPS, ensure_worktree, prompt_for

LOGS = BASE / "logs"
PROMPTS = BASE / "prompts"

# Coding-agent lanes only. Classifier-on-cursor-agent cells stay Ultra-blocked.
LANES = {"wo", "off", "on", "cmp"}
CLAUDE_MODELS = {
    "claude-sonnet-5-thinking-max": ("claude-sonnet-5", "max"),
    "claude-haiku-4-5": ("claude-haiku-4-5", ""),
    "claude-opus-5-thinking-xhigh": ("claude-opus-5", "xhigh"),
}


def claude_bin() -> str:
    found = shutil.which("claude")
    if not found:
        raise SystemExit("claude not on PATH")
    return found


def cells() -> list[dict[str, str]]:
    selected = []
    for cell in GAPS:
        if cell["lane"] not in LANES:
            continue
        if cell["model"] not in CLAUDE_MODELS:
            continue
        selected.append(cell)
    return selected


def already_done(log_path: Path) -> bool:
    if not log_path.is_file() or log_path.stat().st_size < 20:
        return False
    try:
        payload = json.loads(log_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return False
    return payload.get("type") == "result" and not payload.get("is_error")


def launch(cell: dict[str, str], binary: str) -> subprocess.Popen[str]:
    tree = BASE / cell["tree"]
    prompt_path = PROMPTS / f"{cell['tree']}.txt"
    log_path = LOGS / f"cc-{cell['tree']}.json"
    err_path = LOGS / f"cc-{cell['tree']}.err"
    model, effort = CLAUDE_MODELS[cell["model"]]
    prompt = prompt_path.read_text(encoding="utf-8")
    command = [
        binary,
        "-p",
        prompt,
        "--model",
        model,
        "--permission-mode",
        "bypassPermissions",
        "--output-format",
        "json",
    ]
    if effort:
        command.extend(["--effort", effort])
    stdout = log_path.open("w", encoding="utf-8")
    stderr = err_path.open("w", encoding="utf-8")
    env = os.environ.copy()
    env["CLAUDE_CODE_DISABLE_TERMINAL_TITLE"] = "1"
    print(f"LAUNCH {cell['run_id']} {model} {effort or 'default'} -> {log_path}", flush=True)
    return subprocess.Popen(
        command,
        cwd=str(tree),
        stdout=stdout,
        stderr=stderr,
        text=True,
        encoding="utf-8",
        env=env,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wave", type=int, default=3)
    parser.add_argument("--offset", type=int, default=0)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument(
        "--trees",
        default="",
        help="comma-separated worktree names; empty means the full Claude queue",
    )
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    BASE.mkdir(parents=True, exist_ok=True)
    LOGS.mkdir(exist_ok=True)
    PROMPTS.mkdir(exist_ok=True)
    selected = cells()
    wanted = {name.strip() for name in args.trees.split(",") if name.strip()}
    if wanted:
        selected = [cell for cell in selected if cell["tree"] in wanted]
    if args.limit:
        selected = selected[args.offset : args.offset + args.limit]
    else:
        selected = selected[args.offset :]
    for cell in selected:
        status = ensure_worktree(cell["tree"])
        prompt_path = PROMPTS / f"{cell['tree']}.txt"
        prompt_path.write_text(prompt_for(cell), encoding="utf-8", newline="\n")
        print(f"{status} {cell['run_id']}", flush=True)
    (BASE / "claude-matrix.json").write_text(
        json.dumps(selected, indent=2) + "\n", encoding="utf-8"
    )
    if args.prepare_only:
        print(f"prepared {len(selected)} Claude Code cells")
        return 0
    binary = claude_bin()
    running: list[subprocess.Popen[str]] = []
    launched: list[subprocess.Popen[str]] = []
    queued = [
        cell
        for cell in selected
        if not already_done(LOGS / f"cc-{cell['tree']}.json")
    ]
    print(f"queue {len(queued)} / {len(selected)} wave={args.wave}", flush=True)
    for cell in queued:
        while len(running) >= args.wave:
            running = [proc for proc in running if proc.poll() is None]
            time.sleep(5)
        proc = launch(cell, binary)
        running.append(proc)
        launched.append(proc)
    codes = [proc.wait() for proc in launched]
    failed = sum(code != 0 for code in codes)
    print(f"wave finished failed={failed} launched={len(launched)}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
