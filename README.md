# Cortex Loom

**Task-aware repository evidence for coding agents. Bounded context. Visible gaps.**

Cortex Loom prepares a compact evidence packet for the task your coding
agent is working on. It asks
[Weavatrix](https://github.com/sergii-ziborov/weavatrix) (`weavatrix-rust`
2.17.4) for typed repository facts, keeps their provenance, and reports
which declared requirements are covered, missing, contradictory, or stale.

It is local-first. No account, model download, or hosted Cortex service
is required for the deterministic path. Your coding agent still edits
and verifies the code.

CLI, MCP, and one skill are three ways to use the same compiler — not
three products.

> Cortex Loom — проверяемый компилятор контекста: модели только нужные
> факты, плюс доказательство полноты. Неизвестное не скрывается.

<p align="center">
  <img src="docs/images/cli-help.png" alt="cortex-loom --help in a terminal" width="920" />
</p>

<p align="center">
  <img src="docs/images/cli-doctor.png" alt="cortex-loom doctor --repo . JSON status" width="450" />
  <img src="docs/images/cli-setup.png" alt="cortex-loom setup --agent claude-code --dry-run preview" width="450" />
</p>

```text
task  →  evidence plan  →  Weavatrix  →  budgeted packet  →  coverage
                                                      ↓
                                            expand a missing facet
                                                      ↓
                                                coding agent
```

## When it helps

Use Cortex to investigate unfamiliar code, understand callers and
contracts before a cross-file change, or prepare a bounded evidence set
for debugging and review. Skip it for a typo or a small edit in a file
the agent already understands.

Cortex does not replace your agent, build a second repository index, or
filter shell output like RTK. Weavatrix supplies repository
intelligence. Cortex owns task requirements, evidence selection, and
delivery. Refactor support stays preview-only. Weavatrix Quality is a
sibling product: if it wrote `.weavatrix/coverage/lcov.info`, Cortex can
ingest it through `coverage_map`. Cortex does not run Quality or tests.

## Install

Product binaries are **not on crates.io** (`publish = false`). From this
repo:

```powershell
cargo install --path crates/cortex-mcp --locked
cargo install --path apps/cortex-loom --locked
cortex-loom --version
cortex-loom doctor --repo .
```

The headless path does not require Node, Studio, or a local language
model. Building from source needs Rust 1.89+ and a C toolchain.
Platforms, checksums, and the optional Studio package:
[docs/install.md](docs/install.md). CLI contract:
[docs/cli.md](docs/cli.md).

Four libraries are on crates.io (`cortex-context`, `cortex-domain`,
`cortex-router`, `cortex-skills`). They do not start the MCP server.

## Try a task without an agent

```powershell
cortex-loom prepare --repo . --task-file task.md --budget 6000 --format json
cortex-loom expand --packet <packet-id> --facet callers --format json
cortex-loom report --last
```

Use exactly one of `--task`, `--task-file`, or `--task-stdin`. JSON
goes to stdout; diagnostics to stderr. `sufficient` is coverage of
declared evidence requirements, not proof that a change is correct.
A process can exit 0 with an incomplete packet — read the certificate.

## Connect a coding agent

```powershell
cortex-loom setup --agent claude-code --dry-run
cortex-mcp --profile agent
```

Setup is preview-only. It prints `.mcp.json` / skill files and refuses
`--write`. Place them yourself. Default agent profile:

```text
cortex_prepare({ repository, task, runId?, budgetClass })
cortex_expand({ packetId, facet })
```

The `cortex-context` skill says when to call those tools. It does not
ask for Cortex on every file read and it does not use `skill_read`.

Adapters: [docs/install.md](docs/install.md#wire-a-coding-agent).
Optional classifier backends (`off` / `local` / `composer`) are an
environment switch, `CORTEX_LLM_BACKEND`. The CLI has no
`--llm-backend` flag. Details:
[docs/llm-backends.md](docs/llm-backends.md).

## Measured work

Scores below are **task-close /10**: how completely that cell closed
*this* task, relative to the best close of the same task in this tree.
They are not leak-ceiling scores and not “every model found a different
bug.” Full tables, method, and how to repeat:
[docs/benchmark.md](docs/benchmark.md),
[docs/guide.md](docs/guide.md), and
[benchmarks/coding-agents](benchmarks/coding-agents).

SweepLoom matrix at baseline `9f2646c`. **T1** `find_fix_bug`, **T2**
`remove_duplicate`, **T3** `split_api`. One detached worktree per cell.
Parent-verify is the git diff plus the required `cargo test` in an
isolated `CARGO_TARGET_DIR`. A cell is done only when the agent JSON
is `type=result` and `is_error` is false. Session-limit / 429 logs are
gaps, not scores.

What was verified on filled cells:

| Close class | Best cell | What the tree actually did |
| --- | --- | --- |
| T1 / 10.0 | Sonnet 5 max Without | Dead `apply_cleanup`: Cache/Log ids never reached `review_rows` |
| T1 / 8.4–8.2 | Opus Without, Grok Without / models-off | Same `take_value` swallow. Not apply-class |
| T1 / 3.2 | Composer Without | Live tree is `is_silent_ask` (catalog, not production) |
| T2 / 10.0 | Grok models-off | One shared naive classifier: leaf/secret/sqlite/log/history |
| T3 / 10.0 | Opus Without; Grok × Opus-classifier | 18-line facade, six modules |
| T3 / 9.4 | Haiku models-off | Cheap close: `api.rs` 16 lines, five modules, commit `d84731c` |

Opus is not weak on empty Cortex lanes. Those Opus CLI cells did not
run (session limit). On filled cells Opus matches Grok on T1 and is
the best T3 close.

Claude Code spend is usage `input + output + cache_create`. Cursor
spend is estimated context material plus visible response (chars÷4).
Do not pool them. Cortex often cuts spend (Grok T1 809,468 → 40,403)
without raising close class, because those packets were module maps.
Classifier tokens (209–247) did not buy apply-class.

How to use the result: pick **Sonnet Without** when T1 quality is the
goal; **Grok models-off** for T2; **Opus Without** or **Grok × Opus
classifier** for T3; **Haiku models-off** when T3 must stay cheap.
Models-off is the default Cortex lane. Do not expect a classifier
switch to change the close by itself.

Leftover Claude CLI cells (Opus models-off T2/T3, Opus local/Composer
lanes) remain unfilled after 429. They are not scored. Haiku T3
Composer-classifier closed: 79,909 / 8.8 / 45c, `mod.rs` 51, five
modules, commit `99dc62d`, isolated 16/16.

The context-compiler benches (`cortex-bench`) are a separate study:
declared facts in, token count out. They do not score coding-agent
quality. Repeat:

```powershell
cargo test -p cortex-bench --lib
cargo run -p cortex-bench -- --repo . --budget 4000 --set probe `
  --out .cortex-loom/bench/probe.json --stamp local-probe
```

`fixture_anchors_exist_in_the_repository` fails the build if a fixture
cannot be satisfied by reading the named files. That is fixture
authoring, not agent quality.

RTK 0.49.0 is a bash-stdout neighbor. It is not a Cortex substitute.

### Probe — quality stamp (`restore-40-final`, 4 000 tokens)

Ten tasks, 40 facts, this repository. Historical baseline 2026-08-13
was 21 363 / 40/40. The restore stamp is **18 698 / 40/40**. Recheck
after first-pass semantic windows: **19 035 / 40/40**.

| arm | selected tokens | delivered over MCP | facts |
| --- | ---: | ---: | ---: |
| naive known directories | 403 238 | — | 40/40 |
| raw Weavatrix | 95 927 | — | 28/40 |
| Cortex targeted + source windows | 19 035 | 22 850 | **40/40** |
| **Cortex + verified source** | **19 035** | **22 936** | **40/40** |

**95.3% fewer** selected tokens than naive at equal recall.

| set | tasks / facts | cortex-source | targeted |
| --- | ---: | ---: | ---: |
| probe @ 4k | 10 / 40 | **19 035 / 40/40** | **19 035 / 40/40** |
| probe @ 16k | 10 / 40 | **22 818 / 40/40** | — |
| core @ 4k | 7 / 41 | **14 469 / 41/41** | **14 486 / 41/41** |
| langs @ 4k | 6 / 12 | **4 146 / 12/12** | **4 146 / 12/12** |
| intent @ 4k | 3 / 12 | **4 978 / 12/12** | **4 978 / 12/12** |

### Live server — one question, every approach

*Who calls `compile_evidence_bundle`, and what breaks if it starts
refusing more packets?* Four declared facts, real JSON-RPC, 2026-08-10:

| approach | session tokens | calls | facts |
| --- | ---: | ---: | ---: |
| read the candidate files | 79 040 | — | 4/4 |
| `ripgrep` + file reads | 4 904 | 5 | 3/4 |
| Serena MCP 1.28.1 | 10 540 | 3 | 4/4 |
| **Cortex `--profile context`** | **4 167** | **1** | **4/4** |

### Methodology — what enters context this step?

28 declared quality/safety scenarios, synthetic evidence held constant:

| arm | methodology tokens | scenarios |
| --- | ---: | ---: |
| bundled Cortex skills | 10 401 | 3/28 |
| raw Superpowers 6.2.0 | 72 839 | 15/28 |
| **Cortex active-step packet** | **3 812** | **28/28** |

```powershell
cargo run -p cortex-bench -- --repo . --budget 4000 --set probe `
  --out .cortex-loom/bench/probe.json --stamp local-probe
```

## Models are optional

Models-off is a useful default. Calibrated local or proxy profiles can
assist inside explicit roles. They may not lower the deterministic risk
floor or mint verified evidence. Missing Qwen is not an installation
error.

| profile | role | authority |
| --- | --- | --- |
| `gpu-embedding` | Qwen3-Embedding 0.6B on OVMS/GPU | reorder inside a priority band |
| `npu-classifier` | Qwen3-8B INT4 on OVMS/NPU | escalate above the lexical floor |
| `gpu-digest` | `qwen3.5:9b` on Ollama | off-path; gate not passed |

Map: [local models](docs/local-models.md).

## Studio and workflows

Studio remains an optional interface for graphs, sequences, runs, and
docs. It is leaving this repository for a private host. A workflow
suggestion is not a claim that a step ran.

<p align="center">
  <img src="docs/images/studio-canvas.png" alt="Optional Studio graph canvas" width="450" />
  <img src="docs/images/studio-sequences.png" alt="Optional Sequence Studio" width="450" />
</p>

## Workspace

| crate | job |
| --- | --- |
| `cortex-domain` | typed graph schema |
| `cortex-context` | budgeted packet compile |
| `cortex-router` | fail-closed routing |
| `cortex-skills` | `SKILL.md` round trip |
| `cortex-weavatrix` | plan, gather, verify, preview |
| `cortex-mcp` / `cortex-loom` | MCP transport + thin CLI |
| `cortex-eval` / `cortex-bench` | calibration and benches |

**Build:** Rust 1.89+, a C toolchain for bundled SQLite. Node is
build-time only for Studio (`npm.cmd --prefix ui run build`).

```powershell
cargo run -p cortex-mcp --release -- --profile agent
cargo run -p cortex-loom --release -- doctor --repo .
cargo test --workspace
```

Four crates are dual-licensed **MIT OR Apache-2.0** (`cortex-domain`,
`cortex-context`, `cortex-router`, `cortex-skills`). Root `LICENSE-*`
files apply **only** to those crates. Everything else is unlicensed —
see [docs/licensing.md](docs/licensing.md). That is the current grant,
not a claim that the whole product is MIT.

## Docs

[Install](docs/install.md) · [CLI](docs/cli.md) ·
[Guide](docs/guide.md) ·
[Architecture](docs/architecture.md) · [Benchmark](docs/benchmark.md) ·
[LLM backends](docs/llm-backends.md) ·
[Local models](docs/local-models.md) ·
[Competitors](docs/competitors.md)

The same files are served from a running Studio binary at `/api/docs`.
