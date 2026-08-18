# micro_extract LoRA — training-only Python

Not a product runtime. Nothing here is a Cargo dependency, nothing here is in
the OVMS `python_off` bundle, and nothing here runs on a Cortex request path.
The product path stays Rust plus prebuilt OVMS.

Trains `Qwen/Qwen3-0.6B` on the Cortex-original `micro_extract` split so it can
emit the closed JSON the typed provider will accept. It is **LoRA, not QLoRA**:
Windows PyPI torch wheels are CPU-only, this box has no CUDA device, and 4-bit
bitsandbytes is CUDA-only. Base weights stay frozen in fp32; training runs on
**CPU**. That is allowed — `DevicePolicy` forbids the CPU for *product
inference*, not for training.

Two defaults are measurements, not taste. A 1024×3072×1024 sgemm on this box
reaches ~350 GFLOPS at 10 threads and drops to ~300 at 12, because the E-cores
oversubscribe — hence `--threads 10`. And Arrow Lake-U client cores have no
bf16 hardware path, so `--amp bf16` is emulated and markedly *slower* than
fp32 — hence `--amp off`. Both flags are still there; the defaults are just the
side that won.

## Install

```powershell
uv venv --python 3.12 "$env:LOCALAPPDATA\cortex-loom\fine-tune\venv"
uv pip install --python "$env:LOCALAPPDATA\cortex-loom\fine-tune\venv\Scripts\python.exe" -r scripts/fine-tune/requirements.txt
```

`export_ollama.py` also needs llama.cpp's HF→GGUF converter, which is not on
PyPI. Fetch one pinned release once — the tag below is what this pipeline was
run against — and keep it out of the repository:

```powershell
$tag = "b10434"   # zip sha256 FA35776705931C118BCCAD62BD79260451FE18913E87C09B5D89E1C2C35B69FC
$root = "$env:LOCALAPPDATA\cortex-loom\fine-tune"
Invoke-WebRequest "https://github.com/ggml-org/llama.cpp/archive/refs/tags/$tag.zip" -OutFile "$root\llama-$tag.zip"
Expand-Archive "$root\llama-$tag.zip" "$root\stage" -Force
Move-Item "$root\stage\llama.cpp-$tag" "$root\llama.cpp-$tag"
```

`export_ollama.py --llama-cpp` defaults to `%LOCALAPPDATA%\cortex-loom\fine-tune\llama.cpp-b10434`.
This step exists because **Ollama 0.32.9 cannot import `Qwen3ForCausalLM`
safetensors** — `ollama create` fails with `unsupported architecture`. It reads
the resulting GGUF happily.

## Run

```powershell
$py  = "$env:LOCALAPPDATA\cortex-loom\fine-tune\venv\Scripts\python.exe"
$env:HF_HOME = "$env:LOCALAPPDATA\cortex-loom\fine-tune\hf"

cargo run -p cortex-eval -- corpus              # regenerate corpora/
& $py scripts/fine-tune/prepare.py              # -> .cortex-loom/fine-tune/data
& $py scripts/fine-tune/train_lora.py           # -> .cortex-loom/fine-tune/runs/.../adapter
& $py scripts/fine-tune/export_ollama.py --tag cortex-micro-extract-0.6b:v1
& $py scripts/fine-tune/export_ollama.py --no-adapter --tag cortex-micro-extract-0.6b:base

cargo run -p cortex-eval -- --config config/eval-profiles-micro.json --suite micro
```

The last line scores both tags on `crates/cortex-eval/fixtures/micro-extraction.json`
through `judge_micro_extract`. The `tier` in that config is inert: `--suite micro`
never calls `run_profile`, and the `micro_extract` gate is judged on its own.

## Where the bytes live

| what | where | committed? |
| --- | --- | --- |
| scripts, pins, this file | `scripts/fine-tune/` | yes |
| train/dev JSONL | `.cortex-loom/fine-tune/data/` | no — gitignored, regenerate |
| adapter, GGUF, Modelfile | `.cortex-loom/fine-tune/runs/<name>/` | no — gitignored |
| base model cache | `%LOCALAPPDATA%\cortex-loom\fine-tune\hf` | no |
| venv, pinned llama.cpp | `%LOCALAPPDATA%\cortex-loom\fine-tune\` | no |

The f16 GGUF is ~1.2 GB per tag and Ollama keeps its own copy in the blob
store, so budget ~2.5 GB per tag while `ollama create` runs. `export_ollama.py`
deletes the intermediate safetensors copy unless you pass `--keep-merged`.
Nothing multi-GB is committed.

## The two rules the scripts enforce for you

**The prompt is the serving prompt.** `prepare.py` reads
`MICRO_EXTRACTION_SYSTEM` out of `crates/cortex-eval/src/prompts.rs` instead of
copying it, renders the user turn exactly as `micro_extraction_request` does,
and builds the generation prefix with `enable_thinking=False` because
`cortex-ollama` sends `think: false` on every structured call. Judge rows keep
their own system turn so the serving prompt is never paired with a verdict.

**The holdout stays out.** `corpora/micro-extract-train.jsonl` only contains
`split=train`; `prepare.py` re-checks the split, the role, `trainingSource`,
and aborts if any row carries a verified input that appears in the shipped
fixtures. Superpowers markers abort the run too — it is a measured baseline,
never a training source.

## What passing here does and does not mean

`gatePassed` is a claim about a *(model, quantisation, device, runtime)* tuple.
This pipeline produces a GGUF served by Ollama on the iGPU. The profile
`npu-micro-extract-qwen3-0.6b` names OVMS/NPU INT4 on `:8002`. **They are
different tuples.** A pass here is evidence to convert and re-measure, not
permission to flip `gatePassed`.
