"""LoRA fine-tune of Qwen3-0.6B on the Cortex micro_extract train split.

Training-only. Nothing here runs on a Cortex request path, and none of it is a
Cargo or OVMS dependency.

This is LoRA, not QLoRA: Windows PyPI torch wheels are CPU-only, this box has
no CUDA device, and 4-bit bitsandbytes is CUDA-only. Base weights stay frozen
in fp32; only the adapter trains. At 0.6B with ~200-token samples that fits
comfortably in RAM, and the CPU is the honest device to report.

Loss is masked to the assistant turn: the model is graded on the JSON it must
emit, never on reciting the system prompt back.

Usage:
    python train_lora.py [--epochs 3] [--rank 16] [--max-steps N]
"""

from __future__ import annotations

import argparse
import contextlib
import io
import json
import math
import os
import random
import sys
import time
from pathlib import Path

import torch
from peft import LoraConfig, get_peft_model
from transformers import AutoModelForCausalLM, AutoTokenizer

# Every projection, which is what makes a rank-16 adapter on a 0.6B model
# strong enough to change output format rather than only output style.
TARGET_MODULES = [
    "q_proj",
    "k_proj",
    "v_proj",
    "o_proj",
    "gate_proj",
    "up_proj",
    "down_proj",
]


def read_jsonl(path: Path) -> list[dict]:
    with io.open(path, encoding="utf-8") as handle:
        return [json.loads(line) for line in handle if line.strip()]


def encode(tokenizer, row: dict, max_len: int) -> dict | None:
    """Prompt tokens masked out, assistant tokens supervised.

    `enable_thinking=False` matches the serving path exactly: `cortex-ollama`
    sends `think: false` on every structured call, so a model trained behind
    the thinking prefix would meet a different prefix in production.
    """
    messages = row["messages"]
    prompt = tokenizer.apply_chat_template(
        messages[:-1],
        tokenize=False,
        add_generation_prompt=True,
        enable_thinking=False,
    )
    answer = messages[-1]["content"] + tokenizer.eos_token
    prompt_ids = tokenizer(prompt, add_special_tokens=False)["input_ids"]
    answer_ids = tokenizer(answer, add_special_tokens=False)["input_ids"]
    input_ids = prompt_ids + answer_ids
    if len(input_ids) > max_len:
        return None
    return {
        "input_ids": input_ids,
        "labels": [-100] * len(prompt_ids) + answer_ids,
    }


def collate(batch: list[dict], pad_id: int) -> dict:
    width = max(len(row["input_ids"]) for row in batch)
    input_ids, labels, attention = [], [], []
    for row in batch:
        padding = width - len(row["input_ids"])
        input_ids.append(row["input_ids"] + [pad_id] * padding)
        labels.append(row["labels"] + [-100] * padding)
        attention.append([1] * len(row["input_ids"]) + [0] * padding)
    return {
        "input_ids": torch.tensor(input_ids, dtype=torch.long),
        "labels": torch.tensor(labels, dtype=torch.long),
        "attention_mask": torch.tensor(attention, dtype=torch.long),
    }


def batches(rows: list[dict], size: int, pad_id: int) -> list[dict]:
    # Length-sorted so a batch pads to something close to its own longest row;
    # on CPU the padding waste is real wall-clock, not just memory.
    ordered = sorted(rows, key=lambda row: len(row["input_ids"]))
    return [
        collate(ordered[start : start + size], pad_id)
        for start in range(0, len(ordered), size)
    ]


def autocast(amp: str):
    """bf16 compute with fp32 master weights.

    Plain bf16 weights would put the LoRA parameters and the Adam moments in
    bf16 too; autocast keeps them fp32 and only casts the matmuls, which is the
    part the CPU is slow at.
    """
    if amp == "bf16":
        return torch.autocast(device_type="cpu", dtype=torch.bfloat16)
    return contextlib.nullcontext()


@torch.no_grad()
def evaluate(model, dev_batches: list[dict], amp: str) -> float:
    model.eval()
    total, count = 0.0, 0
    for batch in dev_batches:
        with autocast(amp):
            total += float(model(**batch).loss)
        count += 1
    model.train()
    return total / max(count, 1)


def main() -> int:
    repo_default = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=repo_default)
    parser.add_argument("--data", type=Path, default=None)
    parser.add_argument("--out", type=Path, default=None)
    parser.add_argument("--base", default="Qwen/Qwen3-0.6B")
    parser.add_argument("--epochs", type=int, default=3)
    parser.add_argument("--lr", type=float, default=2e-4)
    parser.add_argument("--rank", type=int, default=16)
    parser.add_argument("--alpha", type=int, default=32)
    parser.add_argument("--dropout", type=float, default=0.05)
    parser.add_argument("--batch-size", type=int, default=8)
    parser.add_argument("--grad-accum", type=int, default=1)
    parser.add_argument("--max-len", type=int, default=768)
    parser.add_argument("--seed", type=int, default=20260814)
    parser.add_argument("--max-steps", type=int, default=0, help="0 = full run")
    # Measured on this box (1024x3072x1024 sgemm): fp32 reaches ~350 GFLOPS at
    # 10 threads and ~300 at 12 — the E-cores oversubscribe. bf16 has no
    # hardware path on Arrow Lake-U client cores and is emulated, so autocast
    # is a large slowdown here, not a speed-up. Both defaults are measurements.
    parser.add_argument("--threads", type=int, default=10)
    parser.add_argument("--amp", choices=("off", "bf16"), default="off")
    args = parser.parse_args()

    repo: Path = args.repo.resolve()
    data: Path = (args.data or repo / ".cortex-loom/fine-tune/data").resolve()
    out: Path = (args.out or repo / ".cortex-loom/fine-tune/runs/micro-extract-lora").resolve()
    out.mkdir(parents=True, exist_ok=True)

    torch.set_num_threads(args.threads or os.cpu_count() or 4)
    torch.manual_seed(args.seed)
    random.seed(args.seed)

    tokenizer = AutoTokenizer.from_pretrained(args.base)
    train_rows = read_jsonl(data / "train.jsonl")
    dev_rows = read_jsonl(data / "dev.jsonl")
    encoded_train = [encode(tokenizer, row, args.max_len) for row in train_rows]
    encoded_dev = [encode(tokenizer, row, args.max_len) for row in dev_rows]
    dropped = sum(1 for row in encoded_train + encoded_dev if row is None)
    if dropped:
        raise SystemExit(f"{dropped} rows exceed --max-len {args.max_len}; raise it")
    encoded_train = [row for row in encoded_train if row]
    encoded_dev = [row for row in encoded_dev if row]
    longest = max(len(row["input_ids"]) for row in encoded_train + encoded_dev)
    print(f"train {len(encoded_train)} / dev {len(encoded_dev)} rows, longest {longest} tokens")

    model = AutoModelForCausalLM.from_pretrained(args.base, dtype=torch.float32)
    model.config.use_cache = False
    model = get_peft_model(
        model,
        LoraConfig(
            r=args.rank,
            lora_alpha=args.alpha,
            lora_dropout=args.dropout,
            bias="none",
            task_type="CAUSAL_LM",
            target_modules=TARGET_MODULES,
        ),
    )
    model.print_trainable_parameters()
    model.train()

    pad_id = tokenizer.pad_token_id or tokenizer.eos_token_id
    dev_batches = batches(encoded_dev, args.batch_size, pad_id)
    train_batches = batches(encoded_train, args.batch_size, pad_id)
    steps_per_epoch = math.ceil(len(train_batches) / args.grad_accum)
    total_steps = steps_per_epoch * args.epochs
    if args.max_steps:
        total_steps = min(total_steps, args.max_steps)
    warmup = max(1, total_steps // 10)

    parameters = [p for p in model.parameters() if p.requires_grad]
    optimizer = torch.optim.AdamW(parameters, lr=args.lr, weight_decay=0.0)

    def learning_rate(step: int) -> float:
        if step < warmup:
            return (step + 1) / warmup
        progress = (step - warmup) / max(1, total_steps - warmup)
        return 0.5 * (1.0 + math.cos(math.pi * min(1.0, progress)))

    scheduler = torch.optim.lr_scheduler.LambdaLR(optimizer, learning_rate)
    history: list[dict] = []
    started = time.time()
    step = 0
    stop = False

    for epoch in range(args.epochs):
        order = list(range(len(train_batches)))
        random.Random(args.seed + epoch).shuffle(order)
        running, seen = 0.0, 0
        for index, position in enumerate(order):
            with autocast(args.amp):
                loss = model(**train_batches[position]).loss
            (loss / args.grad_accum).backward()
            running += loss.detach().item()
            seen += 1
            if (index + 1) % args.grad_accum == 0 or index + 1 == len(order):
                torch.nn.utils.clip_grad_norm_(parameters, 1.0)
                optimizer.step()
                scheduler.step()
                optimizer.zero_grad(set_to_none=True)
                step += 1
                if step % 10 == 0 or step == 1:
                    elapsed = time.time() - started
                    print(
                        f"epoch {epoch + 1} step {step}/{total_steps} "
                        f"loss {running / max(seen, 1):.4f} "
                        f"lr {scheduler.get_last_lr()[0]:.2e} "
                        f"{elapsed / step:.1f}s/step",
                        flush=True,
                    )
                if args.max_steps and step >= args.max_steps:
                    stop = True
                    break
        dev_loss = evaluate(model, dev_batches, args.amp)
        # Checkpoint every epoch. On a two-hour CPU run, "train again with one
        # fewer epoch" is not a real option, so the alternative has to already
        # be on disk when the holdout says the last epoch overfit.
        model.save_pretrained(out / f"adapter-epoch{epoch + 1}")
        entry = {
            "epoch": epoch + 1,
            "step": step,
            "trainLoss": running / max(seen, 1),
            "devLoss": dev_loss,
            "elapsedSeconds": round(time.time() - started, 1),
        }
        history.append(entry)
        print(f"== {entry}", flush=True)
        if stop:
            break

    model.save_pretrained(out / "adapter")
    tokenizer.save_pretrained(out / "adapter")
    summary = {
        "base": args.base,
        "device": "cpu",
        "precision": "fp32 master weights" + (", bf16 autocast" if args.amp == "bf16" else ""),
        "threads": torch.get_num_threads(),
        "method": "lora",
        "rank": args.rank,
        "alpha": args.alpha,
        "dropout": args.dropout,
        "learningRate": args.lr,
        "epochs": args.epochs,
        "batchSize": args.batch_size,
        "gradAccum": args.grad_accum,
        "maxLen": args.max_len,
        "seed": args.seed,
        "trainRows": len(encoded_train),
        "devRows": len(encoded_dev),
        "steps": step,
        "targetModules": TARGET_MODULES,
        "history": history,
        "wallClockSeconds": round(time.time() - started, 1),
    }
    (out / "train-summary.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    print(f"adapter -> {out / 'adapter'}")
    print(f"summary -> {out / 'train-summary.json'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
