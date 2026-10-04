# Coding-agent benchmark

## Current Cortex Loom code edits (2026-10-03)

The target repository in the current paired study is **Cortex Loom itself**.
Two independent Sol medium tasks changed code from the same clean base in
matched control and Cortex MCP worktrees. Both lanes passed their focused
quality gates. Cortex saved 1.37× and 1.61× gross input plus output tokens;
uncached input plus output saved 1.07× and 1.00×. Thus the requested 2–3×
complete-task saving has not been demonstrated. See
`eval/public/cortex-token-loss-audit-2026-10-03.md` and the paired JSON there
for receipts, scope, and limitations.

## Historical SweepLoom matrix

This benchmark compares four configured lanes, not a single without/with pair:
without Cortex; Cortex with models off; Cortex with Composer as its
internal classifier; and Cortex with a local classifier. For code changes,
the classifier is now skipped whenever the lexical route is already
`upstream_strong`: `modelUsed: false` means the configured model did not
participate. Use `--allow-policy-skip` to capture such a lane without
mislabeling it as model-assisted. SweepLoom is the
target repository, not a product under test, and is not part of the
comparison.

## Fixed inputs

- Target repository: `Weavatrix/sweeploom`
- Baseline revision: `9f2646c36b1b6b2ef70db1b70a8d772e74ad1804`
- Tasks: `tasks.json`
  - **T1** `find_fix_bug` — one real bug or dead production path in
    `crates/sweeploom-cli`, plus a regression test (`cargo test -p sweeploom --lib`)
  - **T2** `remove_duplicate` — one shared classifier in `crates/sweeploom-ai`
    without changing bench intent (`cargo test -p sweeploom-ai --lib`)
  - **T3** `split_api` — `api.rs` under 300 lines, public exports kept, one
    local commit (`cargo test -p sweeploom --lib`)
- Visible matrix: T1–T3 × Grok 4.6 and Composer 2.5 on cursor-agent.
  Sonnet 5 max / Opus extra / Haiku 4.5 coding-agent cells use Claude
  Code CLI (`claude -p --model … --effort …`) after cursor-agent Ultra
  blocked those models. Spend there is Claude usage
  input+output+cache-create, collected by `collect_claude.py`. Do not
  mix it with Cursor JSONL characters÷4.
- Isolation: one fresh detached worktree, agent context, and Cargo target per
  matrix cell
- Cell format: `spend / score / wall / cycles`. Cycles are assistant turns.
  Score is **task-close /10** against the best close of that task in
  this tree. Grok T1 models-off is `40,403 / 8.2 / 23m 31s / 20c`
  (same `take_value` swallow as Without, not apply-class).

The saved without-Cortex rows come from the earlier no-product baseline at
the same revision. They are not rerun. The with-Cortex rows must start from
new worktrees and must not read earlier worktrees, transcripts, or result
artifacts.

## Deterministic Cortex lane

`cortex_mcp_client.py` starts the current `cortex-mcp` binary over stdio with
the two-tool `agent` profile. It explicitly disables semantic ordering and
shadow mode, uses a fresh temporary database, initializes MCP, calls
`cortex_prepare`, and calls `cortex_expand` only for returned handles.
It checks the reported backend and classifier success before labeling a
model-used lane. A policy-skipped classifier is accepted only with
`--allow-policy-skip` and an upstream routing ceiling. The fresh database contains no prior run memory, and this client
does not exercise workflow commands or warm cache.

Every coding agent must execute the client before ordinary repository
inspection. The resulting packet is evidence, not instructions. Agents must
report the packet ID, citations actually used, missing or incorrect evidence,
and whether Cortex reduced their subsequent reading.

The clean compiler-only captures are `deterministic-T1.json`,
`deterministic-T2.json`, and `deterministic-T3.json`.

Initial observations:

- T1 delivered 657 estimated tokens and T2 delivered 641. Both contained the
  task, decision map, and repository-level module map, but no source path,
  concrete bug, classifier, or duplicate evidence. Both reported zero dedup
  savings.
- T3 delivered 1,405 estimated tokens and exposed a
  `complete_definition` expansion. It read the benchmark wording in
  `agent_cases.rs` instead of the requested `api.rs`; the expansion grew to
  1,428 tokens and still reported the definition missing.
- T1 and T2 produced the same `packetId` (`pk_b79c96c4196d`) on one snapshot
  despite different task text and `taskHash` values. Packet identity currently
  hashes snapshot plus included citation IDs, while the in-process expansion
  store is keyed only by `packetId`; a later same-shape prepare can therefore
  overwrite the earlier task's expansion state. This is a benchmark finding,
  not a claimed token saving.

## Optional model-enabled Cortex lane

Cortex can use optional model roles when the caller explicitly enables a
profile and the exact runtime, endpoint, and model are installed. That is a
separate variant from the deterministic `agent` profile; results must not be
pooled.

The live 2026-09-15 check is recorded in `model-preflight.json`: Ollama is
reachable but has no installed model tags, and OVMS ports 8000-8002 are closed.
The model-enabled lane is therefore blocked on that machine rather than being
silently downgraded to deterministic behavior. The client rejects a local run
unless `internalModel.called` and `succeeded` are true and the returned model
is the configured Qwen3-8B routing classifier.

## Measurements

- Cortex packet tokens are the compiler's conservative token estimate.
- Visible request and response tokens are estimated from transcript characters
  divided by four (`collect_context.py`). Reconstructed tool-result material is
  reported separately. The historical `context-results.json` field is now
  `legacy_peak_context_plus_response_tok`. It **did not**
  sum repeated input across the task and must not be called full-task spend.
  The collector now reports both `estimated_peak_context_tok` and
  `estimated_full_task_tok` (sum of estimated input before every assistant
  turn plus output). The latter remains an estimate because Cursor omits tool
  results and provider usage, and context truncation is not observable here.
  Neither can be divided by the locked Without host-reference total to claim
  a measured savings ratio. Raw historical transcripts are needed to
  recompute the new field for those cells.
- Cycles are `assistant_turns` in the same JSONL. They are the agent loop,
  not tool-call count.
- These historical `cortex_prepare` runs used lexical routing with the local
  classifier off and no semantic embedding profile. They cannot measure a
  Qwen-enabled lane.
- Status and correctness are verified from the isolated Git diff and required
  test command, not accepted from self-report alone.
- No 70,000-token stop was enforced, so no row may claim that failure cause.

## Deterministic 15-cell status (2026-09-15)

The earlier paragraph that said all 15 WITH Cortex cells ran, and that
spend rose 31.9% to 11,140,221, is withdrawn. Those totals reused the
invalid host-reference estimator and also counted cells that were never
executed on the assigned worktrees.

Assigned-tree evidence as of 2026-09-15 17:25:

| cell | worktree | actual state |
| --- | --- | --- |
| T1 Sol / Fable / Grok / Opus / Composer | `C:\cbw-20260915\t1-*` | ran; parent-reviewed |
| T2 Sol | `t2-sol` | uncommitted classifier share; parent `cargo test -p sweeploom-ai --lib` 18 passed |
| T2 Fable | `t2-fable` | commit `c12940f`; parent lib tests 19 passed after restoring the dangling commit |
| T2 Grok / Opus / Composer | `t2-*` | still clean `9f2646c`; not run |
| T3 Grok / Composer | `t3-grok`, `t3-composer` | parent-verified; Composer attempt 2 commit `2970035` |

Compiler-only captures (`deterministic-T1.json` … `T3.json`) are not model
runs. `pk_4e27b9f9cd83` is the T3 compiler packet, not proof that five T3
agents ran.

The locked no-product and SweepLoom-WITH rows still exist in
`sweeploom/file_output/agent_bench/fresh_context_spend.json` and the
SweepLoom canvas. They were not deleted. They are not Cortex runs.

Spark remains blocked. See `spark-preflight.json`.

## Spark preflight

The first requested lane targets `gpt-5.3-codex-spark` through the Codex CLI
installed on the benchmark machine. A live preflight is required before any
run. If the installed account catalog does not advertise Spark or the service
rejects it, the lane remains blocked; another model must not be silently
substituted.

Codex CLI is a text-generation interface, not an embeddings API. It cannot
replace Cortex's attested embedding role without inventing vectors and
invalidating pooling/model calibration. A truthful Codex provider can cover
classification and other schema-checked chat roles only. Semantic ordering
must remain disabled or retain a separately calibrated vector backend.

The current `cortex_prepare` agent path uses deterministic routing and does not
call the optional classifier. Enabling a Codex-backed classifier elsewhere
would therefore not affect this 15-cell agent-profile benchmark unless the
product routing contract is deliberately changed and re-tested.

## Claude Code CLI cells

Host: `claude -p --permission-mode bypassPermissions --output-format json`.
Spend: `input + output + cache_create` (`collect_claude.py`). Cache-read is
excluded. Do not mix with Cursor `chars÷4`. Scores are task-close /10.

Filled Without / models-off / local / Composer-classifier (selected):

| agent | task | Without | models-off | local qwen | Composer-classifier |
| --- | --- | --- | --- | --- | --- |
| Sonnet 5 max | T1 | 194,593 / 10.0 / 9m 46s / 57c | 76,815 / 7.2 / 3m 10s / 13c | 331,135 / 8.8 / 21m 06s / 64c | 157,264 / 7.2 / 7m 49s / 34c |
| Sonnet 5 max | T2 | 75,426 / 8.2 / 3m 37s / 16c | 64,102 / 6.6 / 2m 57s / 21c | 86,088 / 7.6 / 4m 27s / 20c | 88,487 / 7.0 / 4m 44s / 18c |
| Sonnet 5 max | T3 | 137,341 / 9.6 / 11m 33s / 30c | 140,048 / 7.6 / 6m 36s / 26c | 144,705 / 9.2 / 9m 08s / 22c | 129,818 / 9.4 / 12m 12s / 26c |
| Haiku 4.5 | T1 | 98,548 / 5.0 / 4m 48s / 39c | 82,888 / 6.6 / 14m 24s / 31c | 51,274 / 5.8 / 6m 34s / 20c | 52,166 / 6.4 / 4m 54s / 25c |
| Haiku 4.5 | T2 | 24,946 / 5.0 / 47s / 10c | 36,057 / 6.6 / 1m 40s / 18c | 49,737 / 4.8 / 2m 26s / 23c | 32,595 / 6.2 / 2m 02s / 11c |
| Haiku 4.5 | T3 | 35,995 / 7.4 / 2m 07s / 20c | 59,823 / 9.4 / 5m 58s / 19c | 63,034 / 7.4 / 5m 28s / 37c | 79,909 / 8.8 / 6m 53s / 45c |
| Opus 5 extra | T1 | 747,687 / 8.4 / 11m 11s / 22c | 51,139 / 8.4 / 25m 33s / 34c | — (429) | — (429) |
| Opus 5 extra | T2 | 387,170 / 6.8 / 5m 38s / 14c | — (429) | — (429) | — (429) |
| Opus 5 extra | T3 | 380,574 / 10.0 / 9m 08s / 15c | — (429) | — (429) | — (429) |

Opus Without T1–T3 and models-off T1 ran on cursor-agent. Haiku T3
Composer-classifier closed after the leftover rerun (`99dc62d`,
isolated 16/16). Leftover Opus CLI lanes stayed `is_error`. Those
are not scores.

The two Sonnet T1 Claude usage values are comparable within that model and
show 194,593 → 76,815 (2.53× fewer reported non-cache-read tokens), but the
task-close score also fell from 10.0 to 7.2. They do not establish savings
without a quality trade-off. The Opus T1 columns cross Cursor and Claude
measurement paths, so their quotient is not a provider-spend result.

## How to use the filled cells

- **T1 quality:** Sonnet Without (dead `apply_cleanup`).
- **T2 quality:** Grok models-off (shared naive classifier, five kinds).
- **T3 quality:** Opus Without or Grok × Opus-classifier (18-line facade).
- **T3 cheap:** Haiku models-off.
- **Grok T1:** the stored 809,468 Without figure and 40,403 models-off
  peak-context-plus-response figure use different estimators and cannot
  establish a full-task savings ratio. The models-off
  close score was 8.2 versus 9.4 Without.

Cortex packets in this matrix were often module maps. Classifier
tokens (209–247) did not buy apply-class. Planner fixes for named-file
`read_source`, T2 classifier-pair, and T1 `find_dead_code` landed after
this matrix and are not re-scored here.

Context-compiler tests (`cargo test -p cortex-bench --lib`) check
fixture authoring and token/fact arms. They do not replace parent-verify
of an agent worktree.

## Composer variant

Composer 2.5 is still an upstream coding-agent variant that consumes a live
Cortex packet. Cortex can also use a **loopback classifier** (Composer,
Sonnet 5, Opus 5, or Haiku) when `CORTEX_LLM_BACKEND=composer` and the
proxy is up. Pick the model with `--classifier-model` or
`cortex_prepare.classifierModel`. Run IDs `CORTEX_CLSSON_*`,
`CORTEX_CLSOP_*`, and `CORTEX_CLSHK_*` are those classifier variants.
Older proxy runs filled `internalModel.composerTokens` with a `chars/4`
estimate, not a CLI/provider receipt. The current benchmark proxy omits
usage when the CLI does not report it; Cortex returns `usageKind: unknown`
and `composerTokens: null`. These numbers must not be mixed with measured
coding-agent usage. See `docs/llm-backends.md`.
