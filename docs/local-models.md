# Local models: which device does which job

Measured on the development machine 2026-08-05.

| | |
| --- | --- |
| CPU | Intel Core Ultra 7 255U (Arrow Lake-U), 2P + 10E cores |
| NPU | Intel AI Boost, driver **32.0.100.4724** (2026-03-19) |
| GPU | Intel Graphics, **4 Xe-LPG cores / 64 EU @ 2.1 GHz** |
| Memory | 47.5 GB, shared with the iGPU |
| Platform AI | **24 peak TOPS INT8 — CPU, GPU and NPU combined** |

Current configured roles (checked 2026-08-09):

| profile | authority | gate/runtime state |
| --- | --- | --- |
| `gpu-embedding` | within-band evidence ordering only | gate passed for Qwen3-Embedding-0.6B INT8 on OVMS/GPU; endpoint `:8001` was not running during the final check, so deterministic order remains active |
| `npu-classifier` | may only escalate above the lexical routing floor | gate passed for Qwen3-8B INT4 on OVMS/NPU; endpoint `:8000` was healthy |
| `gpu-digest` | future off-path, per-revision digest cache only | `qwen3.5:9b` candidate, gate false; no hot-path authority |
| `npu-micro-extract-qwen3-0.6b` | future verified-input literal extraction only | endpoint `:8002` absent and gate false; not selectable |

The editor already exposes each node's execution target, risk, input/output
budgets, exact `modelProfile`, evidence requirement, upstream review, and
mutation flag. Local-model mutation is rejected, and Ollama nodes cannot turn
off upstream review. Automatic weak-model assignment therefore remains a
calibration problem, not a missing UI control.

Two facts set the whole policy. The platform total is 24 TOPS, so this is not
a 40-TOPS Copilot+ part and the NPU alone is a fraction of that. And the iGPU
is four Xe cores — it can *hold* a 9B model in shared memory but will run it
slowly.

## The three tiers

### NPU — hot path, small, low latency

Runs on every request. Latency is a user's patience.

| role | model | why this one |
| --- | --- | --- |
| `embedding` | `Qwen3-Embedding-0.6B` | Named as NPU-supported by OpenVINO GenAI, and the model already passed this project's retrieval gate at Recall@k 1.00 / nDCG 0.96. Gated semantic ordering is already wired behind `CORTEX_SEMANTIC=1`, reorders only within a priority band, and falls back deterministically — so moving it to the NPU changes cost, not trust. |
| `classification` | `Qwen3-8B-int4-cw-ov` | The 1.5B candidate missed two required escalations. This 8B NPU profile is the smallest measured routing profile with zero misses, so it remains the routing floor. |

### A sub-1B lane, but not a smaller router

The useful place for a 0.3-0.6B model is the gated `micro_extract` role, not
`classification`. Routing decides whether work may stay local; a smaller model
must not make that trust decision after the measured 1.5B profile already
missed two escalations.

The deterministic router may offer work to `micro_extract` only when all of
these are true:

1. the task is read-only and the input is already verified evidence;
2. the output is a closed JSON schema or vocabulary with a small output cap;
3. every field is mechanically checked against the supplied evidence;
4. invalid, unknown, unsupported, or timed-out output falls back without
   changing the lexical routing floor.

Good jobs are identifier extraction, evidence tagging, metadata normalization,
and drafting `PlanHints` that deterministic code validates. Bad jobs are
`route_work`, sufficiency judgment, source compression, change planning, code
review, or any mutation.

The first shadow candidate is `Qwen3-0.6B`: it is multilingual, has a
32K context, and uses the same family already deployed here. `Gemma-3-270m-it`
and `SmolLM2-360M-Instruct` are useful controls, not trusted defaults. The
candidate is present in `config/llm-profiles.json` with `gatePassed: false`;
that is configuration for measurement, not permission to run trusted work. The exact
*(model, quantization, device, runtime)* tuple must pass the micro-extraction
gate: schema validity 1.00, field precision and recall >= 0.95, exact match >=
0.90, zero unsupported fields, zero routing/mutation output, and p95 <= 1.5 s.

The Rust contract makes this lane narrower than the older `local_small`
extraction suite. `MicroExtractRequest` cannot be constructed without verified
input and a non-empty closed field list. The provider submits a strict dynamic
JSON schema, then rejects unknown fields, duplicate values, free-form shapes,
and every returned string that does not occur literally in the verified input.
It never enters `cortex-router`, so passing this gate still grants no routing,
sufficiency, compression, planning, completion, or mutation authority.

### GPU — quality, off the hot path, waiting is free

Four Xe cores will not serve an interactive 9B. That is not a problem when
**nothing is waiting**, which is the point of the `digest` role: structural
evidence (`WX-MODULES`, `WX-GRAPH`) is stable for a whole repository revision
and was exactly the low-value material the token budget kept dropping.
Compute a compact digest **once per revision** on the GPU, cache it against
the revision hash, and serve it for nothing on every later compile.

That is a real token saving with no hot-path cost, and it is the opposite of
the thing that already failed twice: hot-path *compression*. Shadow
compression of a real 7.5k-token packet timed out on CPU, and
[benchmark.md](benchmark.md) showed the 71 % reduction comes from
priority-ordered budgeting across operations, not from a model. A bigger local
model on the request path buys latency and a trust problem, not tokens.

`qwen3.5:9b` is the candidate: it already passed the `local_medium` gate with
perfect citation preservation, and citation preservation is exactly what a
digest must not lose.

### CPU — excluded by policy

`DevicePolicy::default()` permits the NPU and the GPU and **not** the CPU. On
this machine the CPU is busy with the compiler, the test suite and the editor;
a local model competing for it costs more than it saves. Excluding it in one
place removes every CPU-bound profile from every call site at once, instead of
each site being expected to remember.

Opting back in is explicit: `DevicePolicy::new([Device::Npu, Device::Gpu,
Device::Cpu])`.

## What the code guarantees

* **A device is never claimed, only confirmed.** [`Placement`] separates the
  device a profile *declared* from the one a runtime *observed*. An
  unreported placement renders as `npu (unconfirmed)`, and a silent fallback
  renders as `npu requested, cpu used`. Nothing may claim acceleration unless
  `Placement::is_confirmed()`.
* **A model that has not passed its gate is never selected**, however well
  placed. `gatePassed` is a claim about a *(model, device, runtime)* triple —
  a profile that passed on Ollama/CPU has not passed on NPU, because the
  quantisation and the runtime both changed.
* **Loopback is enforced in one place.** Every provider builds its address
  through `LoopbackUrl`, which cannot be constructed from a remote host. There
  is no flag and no environment variable to bypass it, and
  `http://127.0.0.1@evil.example` is rejected as malformed rather than parsed
  as loopback.
* **A forbidden device and a failed gate are different errors**, because they
  have different fixes.

## Measured 2026-08-05: first deployment attempt

No Python anywhere in this. Both models were taken as **pre-converted
OpenVINO IR** from the OpenVINO organisation on Hugging Face
(`Qwen3-Embedding-0.6B-int8-ov`, `Qwen2.5-1.5B-Instruct-int4-ov`, 1.47 GB
total), and OVMS was taken as the official **`python_off`** Windows build
(2026.3.0, 103.9 MB, SHA-256 verified; 484 files, 288.8 MB extracted, no
Python binaries in the bundle). `optimum-cli`, torch and NNCF are not needed
and were not installed.

OpenVINO reported `Available devices: CPU, GPU, NPU`.

### The embeddings were silently wrong

OVMS logged one warning that turned out to matter more than anything else:

```text
Pooling mode was not specified and could not be inferred. Defaulting to CLS pooling.
```

Qwen3-Embedding needs **last-token** pooling. With the default:

| pair | CLS (default) | `--pooling LAST` |
| --- | ---: | ---: |
| unrelated sentences | **0.9603** | **0.2417** |
| paraphrase of the same fact | — | **0.6635** |

A cosine of 0.96 between unrelated text is an embedding that cannot
discriminate at all. Had this been wired in and the retrieval gate re-run, the
collapse would plausibly have been blamed on the device or the quantisation.
**Always pass `--pooling LAST` for this model.** This is precisely why
`gatePassed` is a claim about a *(model, device, runtime)* triple.

### The NPU works — for text generation, not for embeddings

The first attempt put the **embeddings** task on the NPU. It never became
usable: still initialising after **424 s**, 6.5 GB resident, 448 s of CPU
burned, `/v3/models` empty. The same configuration on the GPU was `AVAILABLE`
in under 20 s, which isolates the failure to the device rather than the
config.

Re-run with `--cache_dir` and the roles as designed:

| servable | device | task | outcome |
| --- | --- | --- | --- |
| `qwen25-1.5b` | **NPU** | `text_generation` | `AVAILABLE` in **under 15 s** |
| `qwen3-embed` | GPU | `embeddings` | `AVAILABLE` in **under 15 s** |

So the NPU is fine — Intel documents `text_generation` on NPU for Arrow Lake,
and that is exactly what works. What hung was the OVMS **embeddings**
pipeline on NPU. The embedding role therefore stays on the GPU until that is
understood, which costs nothing: it answers in about 100 ms warm.

### Why the embeddings pipeline hangs on the NPU

**The NPU executes static shapes only.** OpenVINO supports dynamic shapes on
CPU and GPU; the NPU compiler needs fixed dimensions to build its execution
graph. Embedding models are the worst case for that — a batch of N inputs of
varying token length is dynamic in two dimensions at once — and there is an
open OpenVINO issue where the NPU compiler hits a `Gather` node with dynamic
bounds and raises *"to_shape was called on a dynamic shape"* for exactly this
class of model.

That matches what was observed: `text_generation` compiled in under 15 s,
while `embeddings` sat in graph initialisation past 424 s and 6.5 GB. It is a
shape problem, not a capability problem.

The way through, if the NPU is wanted for embeddings, is to reshape the model
to a static batch and sequence length before serving it, and accept padding
waste. That is worth doing only if the GPU's ~100 ms becomes a bottleneck,
which it currently is not.

### Measured through the provider

`cargo run -p cortex-llm --example probe`, against both live endpoints:

| call | device | latency | result |
| --- | --- | ---: | --- |
| 3 embeddings, dim 1024 | GPU | **102 ms** | cosine 0.6635 related / 0.2417 unrelated |
| classify | NPU | **2 736 ms** | returned a valid label |
| classify | NPU | **2 602 ms** | returned a valid label |

Both classifications answered `deterministic`, including for *"change the
retention policy for audited production run evidence"* — which is a high-risk
mutating change that must go upstream. The plumbing is right and the model is
not yet trustworthy. A differently-worded prompt got `upstream` correct
earlier, so this is prompt sensitivity, which is precisely what the
calibration harness measures and precisely why `gatePassed` stays `false`.

### The gates, run against the deployment

`cortex-eval` learned a second backend (`--runtime openai`) so the same
fixtures, comparators and pinned prompts (`eval-prompts-v3`) could be pointed
at the accelerator instead of at Ollama. Two runs, because the two servables
sit on two devices and therefore two ports:

```powershell
cargo run -p cortex-eval -- --config config/eval-profiles-ovms.json `
  --runtime openai --base-url http://127.0.0.1:8001 --suite retrieval
cargo run -p cortex-eval -- --config config/eval-profiles-ovms.json `
  --runtime openai --base-url http://127.0.0.1:8000 --suite classification
```

**Retrieval — `Qwen3-Embedding-0.6B-int8-ov`, GPU: PASS**

| mode | recall@3 | recall@5 | nDCG@5 | MRR |
| --- | ---: | ---: | ---: | ---: |
| embedding | 0.79 | 0.92 | 0.87 | 0.96 |
| hybrid | 0.83 | 0.92 | 0.90 | 1.00 |
| **hybrid_graph** | **0.96** | **1.00** | **0.96** | **1.00** |

Latency p50/p95 593/658 ms over 4 batches. The INT8 OpenVINO conversion did
not cost retrieval quality: `hybrid_graph` matches the 1.00/0.96 the same
weights reached on Ollama. **`gatePassed: true`** for this triple.

**Classification — `Qwen2.5-1.5B-Instruct-int4-ov`, NPU: FAIL**

28/28 replies were schema-valid — OVMS accepted `response_format:
json_schema`, so structured output works on the NPU — but:

| metric | measured | required |
| --- | ---: | ---: |
| accuracy | **0.71** | ≥ 0.80 |
| missed escalations | **2** | **0** |

Latency p50/p95 4 009 / 5 346 ms over 28 calls. Two missed escalations is the
one failure this project treats as disqualifying: the model sent work
downward that should have gone upstream. **`gatePassed` stays `false`.** The
plumbing is correct and the model is not trusted to route.

**Classification — `Qwen3-8B-int4-cw-ov`, NPU: PASS (2026-08-06)**

No `OpenVINO/Qwen3-7B-int4-ov` on Hugging Face; the NPU-oriented channel-wise
IR is `Qwen3-8B-int4-cw-ov`. Served with OVMS 2026.3.0 `--target_device NPU`.
Eval used pinned `eval-prompts-v5` and
`chat_template_kwargs.enable_thinking=false` (without it, extraction truncated
mid-JSON). Report: `.cortex-loom/eval/eval-1786013596.json`.

| metric | measured | required |
| --- | ---: | ---: |
| classification accuracy | **0.86** | ≥ 0.80 |
| missed escalations | **0** | **0** |
| under-called | **0** | — |
| extraction schema-valid | **1.00** | ≥ 0.95 |
| extraction action accuracy | **1.00** | ≥ 0.80 |
| extraction exact-match | **0.70** | ≥ 0.60 |

Latency p50/p95 7 289 / 12 893 ms over 38 calls. The remaining disagreements
are fail-closed over-calls on repository-analysis fixtures (`none` →
`upstream_strong`). Release/version-tag disambiguation closed the prior
`cls-rel-2` miss. **`gatePassed: true`** for this triple.

The report also prints `device: unknown`, because OVMS does not say. That is
the honest value, not a defect.

## Deploying it

The NPU and GPU tiers need OpenVINO Model Server, which exposes an
OpenAI-compatible endpoint and takes an explicit device target. Its text
generation on NPU is documented as tested on Arrow Lake under Windows 11.

Bind explicitly. OVMS defaults to `0.0.0.0`, which would put a model endpoint
on every interface and quietly falsify the local-first claim — our client
enforces loopback, but the client is not what an attacker talks to.

```powershell
$ovms = "$env:LOCALAPPDATA\cortex-loom\ovms\ovms.exe"

# Embeddings. --pooling LAST is not optional for this model.
& $ovms --model_path "$env:LOCALAPPDATA\cortex-loom\models\qwen3-embedding-0.6b-int8-ov" `
        --model_name qwen3-embed --task embeddings --pooling LAST `
        --target_device GPU --rest_port 8001 --rest_bind_address 127.0.0.1

# Text generation for the classifier role (Qwen3-8B NPU IR; no 7B OV IR published).
& $ovms --model_path "$env:LOCALAPPDATA\cortex-loom\models\qwen3-8b-int4-cw-ov" `
        --model_name qwen3-8b --task text_generation `
        --target_device NPU --rest_port 8000 --rest_bind_address 127.0.0.1 `
        --cache_dir "$env:LOCALAPPDATA\cortex-loom\cache\qwen3-8b-npu"
```

Ollama stays supported as a runtime and is what is installed today
(`qwen3-embedding:0.6b`, `qwen3.5:9b`, `qwen3.5:4b` are already pulled), but
Ollama on this box means CPU or iGPU — it has no NPU path.

## Status

`cortex-llm` ships the device policy, the profile registry, the loopback
endpoint and the OpenAI-compatible provider, with tests. Embedding
(`gatePassed: true` on GPU) and classification (`gatePassed: true` on NPU with
Qwen3-8B) are measured. **`route_work` uses the gated classifier when
`CORTEX_LLM=1`** (profiles from `CORTEX_LLM_PROFILES`, default
`config/llm-profiles.json`): the model may only escalate above the lexical
floor; endpoint failure, unknown labels, and under-calls keep the lexical
decision. Remaining work, in order:

1. Prefer reading the device OVMS reports into `Placement::observed` when the
   server exposes it; until then `device: unknown` stays the honest value.
2. Optional accuracy polish: repository-analysis fixtures still over-call to
   `upstream_strong` (fail-closed, does not block the gate).
3. Measure the disabled `micro_extract` candidate only when its exact local
   artifact is already installed; never download one from the evaluation path.
4. Add the `digest` role's cache, keyed by repository revision, and measure it
   as a sixth benchmark arm.

## Sources

- [Intel Core Ultra 7 255U — product specifications](https://www.intel.com/content/www/us/en/products/sku/241860/intel-core-ultra-7-processor-255u-12m-cache-up-to-5-20-ghz/specifications.html)
- [OpenVINO GenAI on NPU](https://docs.openvino.ai/2026/openvino-workflow-generative/inference-with-genai/inference-with-genai-on-npu.html)
- [Text generation serving with NPU acceleration — OpenVINO Model Server](https://docs.openvino.ai/2025/model-server/ovms_demos_llm_npu.html)
- [Running your GenAI App locally on Intel GPU and NPU with OpenVINO Model Server](https://medium.com/openvino-toolkit/running-your-genai-app-locally-on-intel-gpu-and-npu-with-openvino-model-server-eb590af29dbc)
- [Qwen3-0.6B official model card](https://huggingface.co/Qwen/Qwen3-0.6B)
- [Gemma 3 270M IT official model card](https://huggingface.co/google/gemma-3-270m-it)
- [SmolLM2-360M-Instruct official model card](https://huggingface.co/HuggingFaceTB/SmolLM2-360M-Instruct)
