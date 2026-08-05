# Local models: which device does which job

Measured on the development machine 2026-08-05.

| | |
| --- | --- |
| CPU | Intel Core Ultra 7 255U (Arrow Lake-U), 2P + 10E cores |
| NPU | Intel AI Boost, driver **32.0.100.4724** (2026-03-19) |
| GPU | Intel Graphics, **4 Xe-LPG cores / 64 EU @ 2.1 GHz** |
| Memory | 47.5 GB, shared with the iGPU |
| Platform AI | **24 peak TOPS INT8 — CPU, GPU and NPU combined** |

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
| `classification` | `Qwen2.5-1.5B-Instruct` | Named as NPU-supported. Bounded input, closed label set, fail-closed on anything outside it. |

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

### GPU works; NPU did not come up

| device | outcome |
| --- | --- |
| GPU | servable `AVAILABLE` in ~10–20 s; embeddings answered in **1 447 ms** for 2 inputs, dim 1024 |
| NPU | graph still initialising after **424 s**, 6.5 GB resident, 448 s of CPU burned, `/v3/models` still empty |

The NPU path is not refuted — first-load compilation is expected to be slow
and OVMS has a compilation cache — but on this part it did not become usable
in seven minutes, and it consumed the one resource this deployment cannot
spare: a CPU core, continuously. Next attempt should set a cache directory and
measure a warm start before any conclusion is drawn.

Nothing here permits setting `gatePassed`. The GPU embedding endpoint is
merely *serving*; it has not been through `cortex-eval`.

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

# Text generation for the classifier role.
& $ovms --model_path "$env:LOCALAPPDATA\cortex-loom\models\qwen2.5-1.5b-instruct-int4-ov" `
        --model_name qwen25-1.5b --task text_generation `
        --target_device NPU --rest_port 8000 --rest_bind_address 127.0.0.1
```

Ollama stays supported as a runtime and is what is installed today
(`qwen3-embedding:0.6b`, `qwen3.5:9b`, `qwen3.5:4b` are already pulled), but
Ollama on this box means CPU or iGPU — it has no NPU path.

## Status

`cortex-llm` ships the device policy, the profile registry, the loopback
endpoint and the provider contract, with tests. It is **not yet wired into
`cortex-mcp`**, and no profile has passed a gate on an accelerator, so nothing
in this document is a claim that the NPU has run anything here. The remaining
work, in order:

1. Convert both NPU models to OpenVINO IR and serve them with OVMS.
2. Implement the OpenAI-compatible provider against that endpoint, reading the
   device the server reports into `Placement::observed`.
3. Re-run `cortex-eval` against the deployed endpoints and only then set
   `gatePassed`.
4. Add the `digest` role's cache, keyed by repository revision, and measure it
   as a sixth benchmark arm.

## Sources

- [Intel Core Ultra 7 255U — product specifications](https://www.intel.com/content/www/us/en/products/sku/241860/intel-core-ultra-7-processor-255u-12m-cache-up-to-5-20-ghz/specifications.html)
- [OpenVINO GenAI on NPU](https://docs.openvino.ai/2026/openvino-workflow-generative/inference-with-genai/inference-with-genai-on-npu.html)
- [Text generation serving with NPU acceleration — OpenVINO Model Server](https://docs.openvino.ai/2025/model-server/ovms_demos_llm_npu.html)
- [Running your GenAI App locally on Intel GPU and NPU with OpenVINO Model Server](https://medium.com/openvino-toolkit/running-your-genai-app-locally-on-intel-gpu-and-npu-with-openvino-model-server-eb590af29dbc)
