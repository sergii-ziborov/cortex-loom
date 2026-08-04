# Shadow mode — milestone 3, iteration 1 (implemented)

Wire evaluated Ollama profiles into the MCP host behind explicit runtime
configuration and shadow-mode metrics, with zero workflow influence.

## Runtime configuration

Shadow mode starts only when `cortex-mcp` sees explicit configuration:

```powershell
$env:CORTEX_SHADOW = "1"                    # required master switch
$env:CORTEX_SHADOW_SMALL = "qwen3.5:4b"     # exact tag for route_classification
$env:CORTEX_SHADOW_MEDIUM = "qwen3.5:9b"    # exact tag for context_compression
$env:CORTEX_SHADOW_TIMEOUT_MS = "30000"     # optional, default 30000
$env:CORTEX_SHADOW_QUEUE = "64"             # optional, default 64
$env:CORTEX_SHADOW_MAX_COMPRESSION_TOKENS = "2048"  # optional payload cap
cargo run -p cortex-mcp
```

Compression observations whose estimated input exceeds the cap are skipped
and counted (`oversizeSkipped` in `shadow_metrics_read`) instead of queued:
a dogfood run showed that a real 7.5k-token packet times out on CPU where
200-token calibration fixtures succeed, and a timed-out sample is not a
comparable measurement.

Without `CORTEX_SHADOW=1` plus at least one model tag there is no thread, no
queue, and no samples. An operation whose model tag is unset is ignored
entirely. A broken shadow configuration only prints a warning; the MCP host
starts normally without shadow mode.

## Invariants

1. Off by default. Enabled only by explicit runtime configuration
   (`CORTEX_SHADOW=1` plus profile configuration). No hidden model download or
   downgrade; existing cortex-ollama rules apply unchanged.
2. Observation only. The deterministic result is computed and returned exactly
   as today. The shadow runner receives a copy of inputs and the already-final
   deterministic outcome; it cannot modify routing, context, citations, or
   `requiresUpstream`.
3. Never blocks, never fails the hot path. Shadow work runs on a background
   worker with a bounded queue; overflow drops the sample and increments a drop
   counter. Timeouts and errors become failed samples.
4. Fail-closed semantics untouched. Shadow agreement can never relax an
   escalation. A shadow suggesting a lower tier than the deterministic decision
   is recorded as `missed_escalation` — the key safety metric (research gate:
   zero missed escalations).
5. Append-only, read-only exposure. Samples are immutable; MCP and HTTP
   surfaces are bounded reads and aggregates only.

## Calibration prerequisite

A shadow profile must reference a `cortex-eval` calibration record for the
exact model tag. A profile without a passing calibration run may still be
shadowed for measurement, but its report is expected to carry the failing
verdict alongside every aggregate so nobody mistakes observation for approval.

## Shadowed operations (iteration 1)

- `route_classification` — the local model re-classifies the task; compare the
  tier against the deterministic classifier (`cortex-router/src/classifier.rs`).
- `context_compression` — the local model produces a citation-preserving
  compression draft next to the deterministic evidence selection; compare
  citation preservation and token estimates.

## Components

### New crate `crates/cortex-shadow`

- `ShadowConfig { enabled, small_profile, medium_profile, timeout_ms,
  queue_capacity }`, parsed from explicit runtime configuration with
  `enabled = false` by default.
- `ShadowTask::{RouteClassification { .. }, ContextCompression { .. }}` —
  typed, self-contained payloads (evidence IDs plus a snapshot of the
  deterministic outcome).
- `ShadowRunner` — a dedicated `std::thread` worker owning the Ollama client
  and the store handle, fed through a bounded `std::sync::mpsc::sync_channel`.
  `try_send` gives drop-on-full without blocking. Tokio is deliberately not
  used: `cortex-ollama` (ureq) and `cortex-store` (rusqlite) are blocking, and
  `mcport` handlers are synchronous, so an async runtime would add a dependency
  without removing a single block point.
- Comparators are reused from `cortex-eval::comparators`
  (`classification_outcome`, `citation_metrics`, `token_delta`) and stay pure
  and unit-tested without a model. Shadow inference goes through
  `OllamaClient::structured_chat`, which pins the exact model tag and bypasses
  nothing else.

### Hook points

Both hooks live at the MCP call sites in `cortex-mcp`, after the deterministic
reply is finalized: the `route_work` handler and the `weavatrix_context_compile`
handler call `shadow.observe(task_snapshot)` fire-and-forget. `cortex-router`
itself stays model-free — model awareness in the deterministic policy crate is
out of bounds.

### `cortex-store` — append-only `shadow_samples`

```sql
CREATE TABLE IF NOT EXISTS shadow_samples (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at INTEGER NOT NULL,          -- unix seconds, matching other tables
  operation TEXT NOT NULL CHECK (operation IN
    ('route_classification','context_compression')),
  model_tag TEXT NOT NULL,
  device TEXT,
  latency_ms INTEGER,
  input_digest TEXT NOT NULL,
  deterministic_summary TEXT NOT NULL,
  shadow_summary TEXT,
  schema_valid INTEGER,
  agreement INTEGER,
  missed_escalation INTEGER NOT NULL DEFAULT 0,
  citation_preserved_ratio REAL,
  hallucinated_citations INTEGER,
  token_estimate_delta INTEGER,
  error TEXT
);
```

Inserts only; the store exposes no update or delete surface. The list query is
bounded by `LIMIT <= 100`; the aggregate query covers the most recent 1000
samples per query and returns per `(operation, model_tag)`: sample count,
schema-valid rate, agreement rate, missed-escalation count, hallucinated
citations, mean preserved ratio, latency p50/p95, and device breakdown.
`input_digest` is a SHA-256 of the canonical input — raw payloads are not
persisted. For compression, `must_cite` is the intersection of the
deterministic `includedIds` with the actual evidence fragment IDs, so the
synthetic `TASK` citation is never demanded from the draft, and
`token_estimate_delta` is the shadow draft estimate minus the deterministic
packet's `selectedEstimatedTokens`. The drop counter is per-process runtime
state: it appears in the MCP tool response, not in HTTP aggregates.

### Read-only surfaces

- `cortex-mcp`: one bounded tool, `shadow_metrics_read { operation?, model?,
  limit <= 100 }` → enabled flag, configured models, per-process dropped
  counter, aggregates, and (only when `limit` is passed) recent samples. No
  write or apply surface.
- `cortex-server`: `GET /api/shadow/metrics?operation=&model=` and
  `GET /api/shadow/samples?operation=&model=&limit=`, served through the store
  handle only — the server takes no dependency on the shadow runner.

## Testing (no live Ollama in CI)

Covered by `cortex-shadow/src/tests.rs` and `cortex-store` shadow tests:

- Scripted backend behind the same trait used by `cortex-eval`.
- Disabled-by-default: no samples, no queue, no thread; enabling requires both
  the switch and a model tag.
- Agreement and missed-escalation matrix; citation preservation and
  hallucination counting against the deterministic packet.
- A backend error or timeout produces a failed sample; the hot path only ever
  executes a non-blocking `try_send`.
- Queue overflow drops without blocking; the drop counter increments.
- Samples are append-only with monotonic IDs and bounded, filterable reads.
- Full gates as usual: `cargo fmt --check`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -D warnings`, UI build.

## Out of scope (iteration 1)

- Any influence of shadow output on routing or compilation.
- UI overlays for shadow metrics.
- Embeddings and semantic selection (roadmap gate 4).
- NPU claims (research gate: requires OpenVINO/Foundry calibration).
