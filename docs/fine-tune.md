# Fine-tune brief for Claude Opus (max effort)

This file is the entire task. Read the repo against it. Do not invent a
different product.

Repo: `C:\Users\SergiiZiborov\Documents\GitHub\MyProjects\cortex-loom`
HEAD when this brief was written: `6280742`
Operator language: Russian is fine. Identifiers stay English.

## One-sentence job

Stand up a **local LoRA/QLoRA pipeline** that can teach **Qwen3-0.6B** the
Cortex `micro_extract` contract, measure it with `cortex-eval`, and leave
`gatePassed: false` until the real gate passes on a **held-out** fixture set.

You said small models are in scope. This is that job. It is not "make Cortex
smarter" and not "port Superpowers".

## What Cortex is

Cortex Loom compiles typed repository evidence and editable sequences so an
upstream coding agent spends fewer tokens. Local models may **draft or
extract**. They may not route risk downward, mutate, or apply Weavatrix
Refactor.

Product authority is only `config/llm-profiles.json` + `gatePassed`.
`config/model-inventory.json` is a map, not permission.

## Hardware (this machine)

| | |
| --- | --- |
| CPU | Intel Core Ultra 7 255U (Arrow Lake-U), 2P+10E |
| NPU | Intel AI Boost — inference, not a training backend |
| iGPU | 4 Xe-LPG cores / 64 EU, shared 47.5 GB RAM |
| TOPS | 24 INT8 platform total — not Copilot+ 40 |

Realistic train: **0.6B LoRA/QLoRA**. 8B/9B full or long SFT on this box is
the wrong first move. If you decide local train is impossible, **stop and
report**. Do not rent a GPU, do not upload weights, do not pull a 30B.

## What to train (priority)

1. **Primary — `micro_extract` on Qwen3-0.6B.**
   Profile: `npu-micro-extract-qwen3-0.6b` (`llm-profiles.json`).
   Base: official `Qwen/Qwen3-0.6B` (or the smallest honest instruct
   checkpoint of that family). The OVMS NPU IR is **not installed**. Training
   artifact and serving artifact are different tuples.
   Role: closed-schema literal extraction from **already verified** input.
   **Never a router, never a planner, never a compressor.**

2. **Do not start** classification LoRA on the 8B until 0.6B has a written
   eval report. The 8B NPU classifier already passed its gate.

3. **Do not start** 9B digest SFT in this session.

4. **Do not train** `qwen3.5:4b`, `phi4-mini`, embedding models, or
   XiYanSQL 7B/3B. The SQL 7B is **not** the Cortex 7B. The Cortex 7B-class
   product model is `Qwen3-8B-int4-cw-ov` on OVMS/NPU.

## Corpus (already in the repo)

```
cargo run -p cortex-eval -- corpus
```

writes `corpora/sft.jsonl` (138 records, `trainingSource: cortex-original`).

| `targetRole` | n | use |
| --- | ---: | --- |
| `micro_extract` | 18 | train seed for 0.6B |
| `classification` | 38 | not this session |
| `digest` | 82 | not this session |

**18 rows is a seed, not a dataset.** Expand it. Rules:

- Every new row must be Cortex-original: same contract as
  `crates/cortex-eval/fixtures/micro-extraction.json`.
- **Hold out** the existing 8 fixtures. Do not train on them and then claim
  the gate. Generate a larger train split; evaluate on the shipped fixtures
  (and any extra holdout you add under `crates/cortex-eval/fixtures/`).
- Cover: identifiers, env keys, file paths, unicode/multilingual literals,
  empty fields, instruction-as-data, duplicates, extra fields, route/authority
  leakage.
- `trainingSource` stays `cortex-original`. License MIT OR Apache-2.0.
- **Do not** train on Superpowers `SKILL.md` bodies, `using-superpowers`,
  or any upstream skill text. Superpowers is a measured baseline. We rewrote
  13 mechanic *names* as seven typed sequences. This is not a fork and not a
  1:1 port. Sequence templates live in `crates/cortex-sequences/templates/`.

Regenerate the seed after you extend `corpus.rs` / fixtures:

```
cargo run -p cortex-eval -- corpus
```

## Gate you must beat (0.6B `micro_extract`)

From `llm-profiles.json` and `judge_micro_extract`:

| metric | required |
| --- | ---: |
| schema-valid | **1.00** |
| field precision / recall | **≥ 0.95** |
| exact match | **≥ 0.90** |
| unsupported fields | **0** |
| authority / routing output | **0** |
| p95 latency | **≤ 1500 ms** on the *serving* tuple |

The provider already rejects unknown fields and any string that is not a
literal substring of the verified input (`MicroExtractRequest::validate_output`).
The model still has to emit valid closed JSON so validation has something
legal to accept.

`gatePassed` is a claim about a **(model, quant, device, runtime)** triple.
A GGUF/LoRA that passes on Ollama/CPU has **not** passed on OVMS/NPU INT4.
Do **not** flip `gatePassed` to true in this session unless you actually ran
`cortex-eval` against that exact serving tuple and the judge passed. If you
only train, leave the flag false and write the report.

## Runtime policy — do not "unify"

- OVMS = OpenVINO IR, NPU/GPU, loopback `:8000` classifier / `:8001` embed /
  `:8002` future micro.
- Ollama = GGUF, iGPU, `:11434`, eval + future digest.
- Same `LlmProfile` schema, **two servers**. Do not merge them. Do not
  dual-serve the product embedder.
- CPU is forbidden for *product* inference (`DevicePolicy`). Training may use
  CPU/iGPU; say so in the report.
- Loopback only. No remote trainer, no remote endpoint.

Today: Ollama is up (7 tags). OVMS `:8000/:8001/:8002` were down on 2026-08-14.

## Engineering constraints (Agents.md)

- Protocol-independent graph / router / skill compiler stay free of MCP and UI.
- Never auto-apply Weavatrix Refactor. Preview + confirm only.
- Source files **< 500 lines**.
- Training Python is allowed under `scripts/fine-tune/` only. It is **not**
  a product runtime. Do not add torch/unsloth/transformers to Cargo or to the
  OVMS bundle. The product path stays Rust + prebuilt OVMS `python_off`.
- Do not pull models as a side effect of `cortex-eval`. The harness never
  pulls; keep that.
- Do not commit multi-GB weights. Adapter configs, scripts, reports, and
  small LoRA adapters if they are small enough; otherwise `.gitignore` the
  blobs and document the path under `%LOCALAPPDATA%\cortex-loom\`.
- `cargo fmt --all -- --check`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` before you call
  Rust work done. UI build only if you touch `ui/`.
- Do not publish crates. Do not `git push`. Do not set `gatePassed: true`
  without a real report. Ask before any destructive git.

## Suggested order

1. Read `docs/local-models.md`, `config/model-inventory.json`,
   `config/llm-profiles.json`, `crates/cortex-eval/src/corpus.rs`,
   `crates/cortex-eval/fixtures/micro-extraction.json`,
   `crates/cortex-llm` micro-extract types, `crates/cortex-eval/src/prompts.rs`
   (`micro_extraction_request`).
2. Expand a **train** split (hundreds of rows, same contract). Keep shipped
   fixtures as holdout. Wire `cargo run -p cortex-eval -- corpus` so the
   train file is reproducible.
3. Add `scripts/fine-tune/` : prepare JSONL → LoRA train → export GGUF or
   a serving-ready adapter. Pin versions. One README in that folder, short.
4. Train **only** `targetRole=micro_extract` (+ reject rows).
5. Serve the result on loopback (Ollama custom tag is fine for the first
   measurement). Run the existing micro-extract fixtures through
   `cortex-eval` / the typed validator. Write
   `.cortex-loom/eval/micro-extract-lora-<date>.md` (that dir is gitignored)
   **and** a short checked-in note under `docs/local-models.md` Status.
6. If the holdout gate fails: diagnose (invention, extra fields, language
   folding, instruction-following). Iterate on data or rank, not on prompt
   folklore.
7. Stop. Do not convert to NPU IR, do not flip `gatePassed`, do not start
   8B/9B, unless the holdout report is already a pass and you have leftover
   time — then propose the next tuple, do not silently start it.

## Done means

- Reproducible train split + script.
- A trained 0.6B adapter **or** a written blocker (OOM, missing toolchain)
  with the exact command that failed.
- Numeric holdout report against the micro_extract judge.
- Product profiles unchanged unless a gate actually passed.
- No Superpowers text in any training file (`rg -i 'using-superpowers|obra/superpowers'`
  over `corpora/` and `scripts/fine-tune/` is empty of bodies).

Start now. Do not wait for a second prompt.
