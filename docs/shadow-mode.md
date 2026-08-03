# Shadow mode design — milestone 3, iteration 1

Wire evaluated Ollama profiles into the MCP host behind explicit runtime
configuration and shadow-mode metrics, with zero workflow influence.

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
  created_at TEXT NOT NULL,
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

Inserts only. The list query is bounded by `LIMIT`; the aggregate query returns
per `(operation, model_tag)`: sample count, schema-valid rate, agreement rate,
missed-escalation count, latency p50/p95, device breakdown, and dropped count.
`input_digest` is a SHA-256 of the canonical input — raw payloads are not
persisted.

### Read-only surfaces

- `cortex-mcp`: one bounded tool, `shadow_metrics_read { operation?, model?,
  limit <= 100 }` → aggregates plus optional recent samples. No write or apply
  surface.
- `cortex-server`: `GET /api/shadow/metrics` and
  `GET /api/shadow/samples?limit=`, served through the store handle only — the
  server takes no dependency on the shadow runner.

## Testing (no live Ollama in CI)

- Scripted backend behind the same trait used by `cortex-eval`.
- Disabled-by-default: no samples, no queue, no thread.
- Agreement and missed-escalation matrix; citation preservation and
  hallucination counting.
- Timeout produces a failed sample and leaves hot-path latency unaffected.
- Queue overflow drops without blocking; the drop counter increments.
- Samples are append-only.
- Full gates as usual: `cargo fmt --check`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -D warnings`, UI build.

## Out of scope (iteration 1)

- Any influence of shadow output on routing or compilation.
- UI overlays for shadow metrics.
- Embeddings and semantic selection (roadmap gate 4).
- NPU claims (research gate: requires OpenVINO/Foundry calibration).
