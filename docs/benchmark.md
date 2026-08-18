# Context benchmark

`cortex-bench` measures one thing, on purpose:

> For one engineering task on one repository, how many context tokens reach the
> upstream agent, and how many of the facts the task needs are in them?

The context benchmark does not exercise refactor planning. The current refactor boundary is Rust-only and preview-only: an upstream-authored exact plan is validated and rendered without changing files. No JavaScript planner, confirmation token, or apply path participates in these measurements.

Three arms answer it:

| arm | what it stands for |
| --- | --- |
| `naive` | no repository intelligence: the candidate files, read whole |
| `weavatrix-raw` | Weavatrix evidence pasted in unbudgeted and unordered |
| `cortex-loom` | the same four operations through `compile_evidence_bundle` |
| `weavatrix-planned` | the planned operations under **Weavatrix's own** `token_budget` |
| `cortex-targeted` | the same planned operations, then compiled |

`weavatrix-planned` is the control that decides whether this project
contributes anything: identical evidence, budgeted by the tool that produced
it, with no compiler. Whatever separates it from `cortex-targeted` is ours and
nothing else is.

`weavatrix-raw` exists to answer the only question that matters for this
project's premise: **what does the control plane add on top of Weavatrix
alone?** The first run of this benchmark answered "nothing", and finding out
why produced both a bug fix and the `cortex-targeted` arm.

```powershell
cargo run -p cortex-bench -- --repo . --budget 4000
cargo run -p cortex-bench -- --repo . --budget 4000 --set intent
cargo run -p cortex-bench -- --repo . --budget 16000 --out .cortex-loom/bench/report-16k.json
cargo run -p cortex-bench -- --list
cargo run -p cortex-bench -- release
```

Reports land in `.cortex-loom/bench/`. `--no-weavatrix` measures the naive arm
alone, without building a graph.

## Method

Each fixture in `crates/cortex-bench/src/tasks.rs` declares the directories a
keyword sweep would open and a set of **anchors** — literals that prove a
required fact is present. An arm scores `recall` (anchors satisfied) and
`tokens/fact` (context tokens per fact actually delivered). An arm that returns
nothing scores no cost per fact rather than a flattering zero, and an
unavailable arm is reported with its reason instead of being scored.

A fixture the naive arm cannot satisfy is a bug in the fixture. The test
`fixture_anchors_exist_in_the_repository` fails the build if that happens.

### What it does not measure

Task success, answer quality, latency, or the agent's own reasoning tokens. No
model runs. Tokens are the workspace's 4-chars-per-token estimate, not a
tokenizer count. The naive arm is handed the right directories, so its cost is
a **lower bound** on the cost of having none of this.

### Scoring excludes the prompt echo

The compiled packet prepends a synthetic `TASK` item containing the prompt.
Tokens count it, because it is sent. Anchors do **not** — otherwise a prompt
that names `MAX_RETRY_ATTEMPTS` would satisfy that anchor without anything
having been retrieved, and the arm would be scored on the question instead of
the answer. See `measure_scoped`.

## The first run, and what it found

At the recommended 4 000-token budget the original three arms produced:

| arm | tokens | facts |
| --- | ---: | ---: |
| `naive` | 63 745 | 24/24 |
| `weavatrix-raw` | 46 185 | 10/24 |
| `cortex-loom` | 3 476 | 1/6 — **three of four tasks failed closed** |

and at 16 000, where nothing had to be dropped, `cortex-loom` was
`weavatrix-raw` plus 30 tokens at identical recall — the control plane adding
a header and nothing else. Two causes, both real:

**A. Blanket criticality on split fragments.** `critical evidence WX-SYMBOL-4
needs 1035 tokens; context budget is 4000`. Weavatrix `context_bundle` output
splits into sub-citations and every one inherited `SymbolContext` →
`Critical`, so their sum exceeded any budget below roughly 5 000. **Fixed:**
criticality now attaches to the *head* sub-citation only
(`evidence_policy(kind, head)`); the tail is high-priority and truncatable.
The definition of a symbol still cannot be dropped; page twenty of its
reference list can.

**B. Four operations out of forty-two.** `prepare_context` always asked
`graph_stats`, `module_map`, `context_bundle`, `verified_change` — every one a
*summary*. They describe a repository's structure and contain none of the
identifiers a task names. Meanwhile Weavatrix already implements its own
deterministic `token_budget` with the same `bytes / 4` estimator, so budgeting
alone was never going to differentiate anything. **Fixed:**
`cortex_weavatrix::plan` chooses operations from the task text — identifiers
extracted by shape, no model involved — and pushes a share of the budget into
each call, so Weavatrix trims the array it understands instead of a whole
fragment being dropped afterwards.

## Measured after the fixes — 2026-08-05, four tasks, 4 000-token budget

| arm | tokens | facts | vs naive |
| --- | ---: | ---: | --- |
| `naive` | 74 519 | 24/24 | — |
| `weavatrix-raw` | 47 723 | 14/24 | −36 % tokens, −42 % facts |
| `cortex-loom` | 14 261 | 14/24 | −81 % tokens, −42 % facts |
| `weavatrix-planned` | 52 087 | 18/24 | −30 % tokens, −25 % facts |
| `cortex-targeted` | **14 898** | **18/24** | **−80 % tokens, −25 % facts** |

The single clearest case, `retry-exhaustion`:

| arm | tokens | facts | tokens/fact |
| --- | ---: | ---: | ---: |
| `naive` | 24 321 | 6/6 | 4 054 |
| `weavatrix-raw` | 13 532 | 3/6 | 4 511 |
| `cortex-loom` | 3 492 | 3/6 | 1 164 |
| `weavatrix-planned` | 14 749 | 6/6 | 2 458 |
| `cortex-targeted` | **3 820** | **6/6** | **637** |

**84 % fewer tokens than reading the files, with every required fact
present.** That is the first result in this exercise that supports the
project's premise.

## Who earns which half

The control answers the question this project has to answer honestly.

Weavatrix is built by the same author, so this split is engineering
attribution, not a competitive claim: it decides which layer to invest in
next, and nothing here is a reason to present one as beating the other. The
figure that matters outside this section is what the two cost **together**
against not having them — see "One question, every approach" below.

> **Correction, updated 2026-08-11.** The 71 % compiler saving recorded below was
> measured against `weavatrix-rust` 2.1.1, which accepted `token_budget`
> everywhere and honoured it in one place. **That saving no longer exists.**
> Since 2.2.0 the parameter is refused where it is not implemented and the
> planner only sends it where it works, so Weavatrix now trims its own
> answers. On the current ten-task probe set (`weavatrix-rust` 2.5.0), the
> compiler and the operations it compiles are effectively tied:
>
> | arm | tokens | facts |
> | --- | ---: | ---: |
> | `weavatrix-planned` | 23 636 | 28/40 |
> | `cortex-targeted` | **23 287** | 28/40 |
>
> **−1.5 % tokens, identical recall on all ten tasks.** This is too small to
> support an economic claim about compiler budgeting. The paragraphs below are kept
> because they explain what was true on 2.1.1, not what is true now.
>
> This does not make the layer worthless, and it does change what it is for.
> What survives the control is *planning* (which operations to call at all),
> *source follow-up* (28/40 → 40/40), the fail-closed refusal, the omission
> record, and — measured separately in "As an MCP" below — collapsing a
> four-call agent session into one. What does **not** survive is the claim
> that priority-ordered budgeting saves tokens over Weavatrix's own.

**Weavatrix earns the recall.** Planning which of its operations to call
moved facts from 14/24 to 18/24; the compiler cannot invent evidence that was
never fetched.

**Cortex Loom earned the tokens on 2.1.1.** The same planned evidence cost
52 087 tokens under Weavatrix's own budgeting and 14 898 through the
compiler — 71 % fewer at identical recall, 18/24 either way. The reason was
structural rather than clever: each of five operations budgeted its own answer
with no knowledge that the others ran, so none of them could spend a low-value
fragment's share on a high-value one. Only the layer holding every fragment
could rank them against each other and stop when the budget was spent. On
2.2.0 and later Weavatrix does that trimming itself.

Cross-operation deduplication (`ContextRequest::deduplicate`) is part of the
same idea and, on this fixture set, a small part: 1 repeated line on two of
four tasks, about 7 tokens each. It is reported rather than inflated — the 71 %
comes from priority-ordered budgeting across operations, not from dedup.

## What the numbers say now

1. **The fail-closed fix is what unlocked the budget arm.** `cortex-loom` now
   completes all four tasks and delivers `weavatrix-raw`'s recall for 70 %
   fewer tokens. That value existed all along and was hidden by the defect.
2. **Targeting buys recall, not tokens.** +6 % tokens for +29 % facts overall,
   and +100 % on the task whose facts were identifier-level. Budgeting and
   targeting are different levers and only the second closes a recall gap.
3. **It is not uniformly better.** On `evidence-priority-band`, targeting
   *lost* an anchor (83 % → 67 %): the `fn rank` fact lived in the symbol
   bundle the untargeted arm happened to fetch, and the search share of the
   budget cut it. Planning trades a broad sweep for a sharp one, and a sharp
   one can miss.
4. **A vague prompt still gets a vague packet.** The planner reads identifiers
   from the task text. "make it faster" plans two structural operations and
   nothing else — correct behaviour, and a reason the tool description now
   tells callers to name what they care about.
5. **Reading the files still wins on recall, always.** 24/24, every time, for
   4.5× the tokens. The trade is real and it is a trade, not a free lunch.

## Measured 2026-08-15 — restored probe, plus core and langs

Host: Windows 11, Intel Core Ultra 7 255U (14 logical), 47.5 GB RAM,
Intel Graphics. `rustc 1.97.1`. **No NVIDIA GPU.** The context bench
is CPU-only (Weavatrix graph + compile). GPU/NPU are unused unless
`CORTEX_SEMANTIC=1` or a gated local profile is on. Resources sampled
every 250 ms from `scripts/measure-bench.ps1`.

### Probe — 10 tasks, 40 facts, 4 000-token budget

Stamp `restore-40-final`. Report:
`.cortex-loom/bench/probe-restore-40-final.json`.

The 2026-08-13 quality stamp is the right historical baseline for
`cortex-source`: **21 363 tokens at 40/40**. An intermediate run after
ranking changes dropped to 17 748 / 37/40. That was a regression, not a
saving. This stamp restores **40/40**.

| arm | selected tokens | delivered over MCP | facts |
| --- | ---: | ---: | ---: |
| naive known directories | 398 441 | — | 40/40 |
| raw Weavatrix | 95 927 | — | 28/40 |
| Weavatrix planned | 8 419 | — | 29/40 |
| Cortex targeted | 9 282 | 12 231 | 29/40 |
| **Cortex + verified source** | **18 698** | **22 794** | **40/40** |

Same facts as the 21 363 baseline, **12.5% fewer** selected tokens
(compact dependents render, not a cheaper counter). Against naive:
**95.3% fewer** selected tokens at equal recall.

Resources for the same 10-task run: **14.9 s wall**, **13.8 s CPU**,
**83.5 MB** peak working set.

### Probe — same tasks, 16 000-token budget

Stamp `restore-40-16k`. A wider budget is slack, not quality.

| arm | selected tokens | facts |
| --- | ---: | ---: |
| naive | 398 441 | 40/40 |
| Cortex + source | **22 818** | **40/40** |

Same 40/40. Extra tokens (~4k over the 4k-budget packet) are unused
headroom. Resources: **22.9 s wall**, **14.4 s CPU**, **80.5 MB** peak.

### Core fixtures — 7 tasks, 41 facts, 4 000-token budget

Stamp `core-hole-fix` (2026-08-18). Report:
`.cortex-loom/bench/core-hole-fix.json`. These are the original
hand-written fixtures (retry, priority, skills, usage, blast radius,
HTTP contract, MCP transport). Facts that live one file away from the
first search hit — `MAX_RETRY_ATTEMPTS`, `fn rank`, `unquote`,
`tools/list`, `build_server`, `Mcp-Session-Id` — are implied by the
task wording and opened as preferred source windows. Probe prompts do
not trip those cues.

| arm | selected tokens | delivered over MCP | facts |
| --- | ---: | ---: | ---: |
| naive | 329 735 | — | 41/41 |
| raw Weavatrix | 69 009 | — | 19/41 |
| Weavatrix planned | 7 741 | — | 27/41 |
| Cortex targeted | 8 354 | 10 474 | 27/41 |
| **Cortex + verified source** | **13 601** | **16 561** | **41/41** |

Stamp `targeted-source-core` (2026-08-18) re-measures the same seven
tasks after targeted gained the same bounded source windows (no
sufficiency retry) and sufficiency stopped treating search-only
identifiers / Python `def` / a short graph span as a miss.

| arm | selected tokens | delivered over MCP | facts |
| --- | ---: | ---: | ---: |
| naive | 331 143 | — | 41/41 |
| Cortex targeted + source windows | 13 147 | 15 865 | **41/41** |
| **Cortex + verified source** | **13 449** | **16 257** | **41/41** |

Every `cortex-source` task reports `sufficient: true`. Targeted now
matches source recall on this set. `cortex-loom` (four operations, no
windows) stays 18/41. Resources: **14.2 s wall**, **22.8 s CPU**,
**80.7 MB** peak.

Stamp `close-2-probe` (2026-08-18): first-pass semantic hits plus
source windows that name their path. Source and targeted both
**19 035 / 40/40**. `cortex-loom` stays 24/40. Resources: **21.6 s
wall**, **30.8 s CPU**, **86.2 MB** peak.

### Languages — 6 tiny fixtures (TS, JS, Python, Go, Java, C#)

Stamp `langs-honest` (2026-08-18). Report:
`.cortex-loom/bench/langs-honest.json`. Anchors live in
`crates/cortex-bench/fixtures/langs/`. The previous 2 973 / 12/12
stamp could be satisfied by `lang_tasks.rs` itself. This run hides
those literals with `concat!`, ranks language samples above the task
list, and still scores **12/12** from the real files. Naive is cheap
because the files are a few dozen lines — that is not a win for
dumping the repo.

| arm | selected tokens | delivered over MCP | facts |
| --- | ---: | ---: | ---: |
| naive (the one file) | 357 | — | 12/12 |
| raw Weavatrix | 38 800 | — | 8/12 |
| Weavatrix planned | 1 841 | — | 12/12 |
| Cortex targeted | 2 310 | 3 739 | 12/12 |
| **Cortex + verified source** | **3 549** | **5 248** | **12/12** |

`cortex-loom` (no source follow-up) stays **8/12** — it finds the
cap constant and drops the `function` / `def` / `func` head on TS,
JS, Python, and Go. Resources: **38.5 s wall**, **8.5 s CPU**,
**80.0 MB** peak.

Sequence recheck on the same tree: stamp `sequence-after-core-fix`,
native **28/28**, **10 401 → 3 812** tokens. `promoted` is false
because `--superpowers-root` was not passed.

```powershell
cargo build -p cortex-bench --release
powershell -File scripts/measure-bench.ps1 -Set probe -Budget 4000 `
  -Stamp local-probe -Out .cortex-loom/bench/probe.json
powershell -File scripts/measure-bench.ps1 -Set core -Budget 4000 `
  -Stamp local-core -Out .cortex-loom/bench/core.json
powershell -File scripts/measure-bench.ps1 -Set langs -Budget 4000 `
  -Stamp local-langs -Out .cortex-loom/bench/langs.json
cargo run -p cortex-bench --release -- --set intent --budget 4000
cargo run -p cortex-bench --release -- release --out .cortex-loom/bench/release-status.json
```

### Intent fixtures and untrimmed compile — 2026-08-18

Stamp `intent-stage4` / `full-untrim-core`. `--set intent` scores git
history, a pasted stack trace, and test selection against this repo.
`cortex-bench release` writes the Stage 4 arm/metric envelope and
fails closed when Serena is not configured.

Compiling `cortex-full` at the declared 4k budget after an overcommit
gather dropped sibling facts (measured 38/41). The untrimmed arm now
compiles at `budget × 16` (min 32k) so gather is the experiment, not
a second trim. That is **not** the quality path — targeted and source
stay at 4k.

| set | arm | selected tokens | facts |
| --- | --- | ---: | ---: |
| core | naive | 336 812 | 41/41 |
| core | cortex-source | **14 434** | **41/41** |
| core | cortex-targeted | **14 451** | **41/41** |
| core | cortex-full (compile 32k) | **16 956** | **41/41** |
| intent | cortex-source / targeted | **5 140** | **11/12** |
| intent | cortex-full | **7 538** | **12/12** |

Git 4/4 after `git_history` stopped asking for analytics (cochange
ate the 800-token cap). Stack 4/4 after panic paths become source
hits. Tests 3/4 on the quality path: the suite file opens, but the
4k compile drops the line-1 window that names
`selects_priority_order_and_reports_token_savings`. Full keeps it.

Stage 4 on this machine: `liveComparison=false`, `serena=false`
(`CORTEX_SERENA_ROOT` unset). That is a missing arm, not a silent
zero.

## The honest conclusion

- **Over Weavatrix alone**, the control plane buys a measured 71 % token
  reduction at equal recall — against Weavatrix's *own* budgeting, not against
  an unbudgeted dump — plus the omission record and the fail-closed refusal.
  That is a defensible claim.
- **Over reading the files**, it buys 78 % fewer tokens for 75 % of the facts.
  Whether that trade is good depends on the task, and the benchmark reports
  both numbers rather than picking the flattering one.
- The **"90 % less upstream work"** hypothesis remains unproven: 78 % of
  *evidence* tokens on four hand-written fixtures is not 90 % of upstream
  work, and no model ran.

## Budget-aware planning — measured 2026-08-05

Measuring the *composition* of a targeted plan, rather than only its total,
found the next win and refuted the one that had been planned.

A digest cache keyed by repository revision was supposed to compress the
structural evidence. Then the fragments were measured:

| kind | operation | fragments | tokens | share |
| --- | --- | ---: | ---: | ---: |
| ChangePlan | `verified_change` | 6 | **6 007** | **41 %** |
| SymbolContext | `inspect_symbol` | 5 | 4 834 | 33 % |
| Dependents | `get_dependents` | 3 | 2 455 | 17 % |
| SearchHits | `search_code` | 2 | 1 322 | 9 % |
| ModuleMap | `module_map` | 1 | **55** | **0.4 %** |

`module_map` — the thing the cache was for — is four tenths of one percent.
Caching it saves nothing. The bulk is `verified_change`, which depends on the
*task*, not the revision, so it is not cacheable at all; and it is the exact
evidence the 4 000-token budget was already throwing away without costing a
single required fact.

### `token_budget` — three different behaviours, one parameter

Measured across `weavatrix-rust` 2.1.1 and 2.2.0, all with `token_budget`
set:

| operation | 2.1.1 | 2.2.0 |
| --- | --- | --- |
| `search_code` | asked 1 600, returned 1 322, reported `fit: true` | same |
| `context_bundle` | — | asked 800, dropped 46 items, **returned 4 778**, reported `fit: false` |
| `inspect_symbol` | asked 800, returned 4 834, **no report** | **rejected**: "does not bound its answer by token_budget" |
| `get_dependents` | asked 800, returned 2 455, no report | rejected |
| `verified_change` | asked 800, returned 6 007, no report | rejected |

2.1.1 accepted the parameter everywhere and honoured it in one place, with
no signal — a caller protecting its context window overran by six times and
had nothing to attribute it to. **2.2.0 fixes exactly that**: the parameter is
now refused where it is not implemented, and the error names the operations
that do implement it (`context_bundle`, `query_graph`, `read_source`,
`search_code`).

The remaining subtlety is easy to mistake for a second defect, and is not
one: **`bounded` means "trims what it can and tells you whether it fitted",
not "will fit"**. Under an 800-token request `context_bundle` dropped 46
items, emptied both source arrays, still returned 4 778, and reported
`fit: false`. The rest is the relationship graph, which Weavatrix holds
lossless by contract — it would rather return a truthful answer that is too
big than a smaller one that is wrong. Verified identical on 2.2.0 and 2.2.1.

Two consequences, both now implemented: the planner estimates bounded
operations like any other, because a granted request is not a guarantee; and
`prepare_targeted_context` reads the `fit: false` report and records it as a
warning on the bundle, so an overrun is attributed to an operation instead of
being discovered in the total.

Every call above passed `token_budget`. Only `search_code` obeyed it (asked
1 600, returned 1 322) and only `search_code` reports a `token_budget` block
in its reply. `inspect_symbol` asked 800 and returned 4 834;
`get_dependents` 800 → 2 455; `verified_change` 800 → 6 007. The parameter is
accepted without error and ignored without a signal, so a caller has no way
to notice. Per-operation budgets are therefore fiction for most of the plan,
and the compiler is the only real enforcement.

### The fix: cost as policy, and stop before the budget is spent

Three changes, each forced by a measurement rather than chosen:

1. **Prefer the bounded operation.** Symbol evidence moved from
   `inspect_symbol` to `context_bundle`, which at least trims and reports.
2. **Send `token_budget` only where it exists.** Anywhere else it is now an
   error, and before that it was a lie.
3. **Estimate every operation, and make the estimates a policy.**
   `PlanPolicy` carries them with defaults from this repository, and
   `plan_with` takes an override — because these numbers are a snapshot of
   one codebase and will be wrong on another. The plan then keeps only the
   prefix the budget can carry: a bounded operation may reach the ceiling
   (the runtime at least trims it), an unbounded one is only requested if its
   whole estimate fits.

| | 2.1.1, unplanned | 2.1.1, planned | 2.2.0, bounded |
| --- | ---: | ---: | ---: |
| evidence assembled per task | 14 671 | 8 662 | **6 183** |
| `weavatrix-planned`, 4 tasks | 52 087 | 34 022 | **25 577** |
| `cortex-targeted`, 4 tasks | 14 898 | 15 055 | **13 887** |
| facts | 18/24 | 18/24 | **18/24** |

**58 % less evidence fetched and a 7 % smaller delivered packet, at identical
recall.** Most of that is work not done at all: latency, Weavatrix CPU, and
evidence assembled only to be discarded. No single Weavatrix operation can
make this decision — it requires knowing what the others already committed.

## Was the trimming free? — `cortex-full`

Trimming an operation away is only a saving if the operation carried nothing.
Asserting that would be cheap, so there is an arm for it: `cortex-full` runs
the identical plan with `PlanPolicy { overcommit: 1_000, .. }`, which asks for
every operation the budget would normally drop — including the
~6 000-token `verified_change` — and then compiles the result the same way.

| arm | tokens | facts |
| --- | ---: | ---: |
| `cortex-targeted` | 13 906 | 18/24 |
| `cortex-full` | 15 001 | **18/24** |

**Not one extra fact.** On `usage-quality-tool` the two arms are byte-identical
(3 774 tokens, same omitted ids) because the compiler discarded the extra
evidence anyway — the fetch was pure waste. Across the set, asking for
everything costs 1 095 more delivered tokens plus roughly 6 000 tokens of
Weavatrix work per task, and returns nothing.

**The limit of this claim (identifier set).** Those four fixtures scored
identifier-level facts, and `verified_change` produces a *change plan* —
advice, not identifiers. Anchor recall cannot see advice, so that run measured
that the plan carries no required **facts**, not that it is worthless to a
reader.

## Structural fixtures — blast radius, contracts, transport

Three tasks were added whose anchors are structural facts Weavatrix graph
tools should surface: dependents of `compile_context`, the
`/api/skills/compile` HTTP contract, and readers of the Streamable HTTP
`/mcp` transport. Anchors were taken from live `get_dependents` /
`list_endpoints` evidence on this repository (graph CURRENT, ~2 638 nodes).

Two planner gaps showed up immediately and were fixed with measurement:

1. **Intent, not only identifier shape.** A "who depends / what breaks"
   question was still planned like a rename, so at the 4 000-token budget
   `get_dependents` was trimmed behind an expensive `context_bundle`.
   Deterministic intent cues now put `get_dependents` first on blast-radius
   questions and `list_endpoints` first on API/transport-contract questions.
2. **Compiler priority.** `Dependents` / `Endpoints` sat at Normal and lost
   to an unverified `ChangePlan` whenever `cortex-full` fetched both —
   structural recall collapsed on the full arm until those kinds were raised
   to High (verified facts beat unverified advice within the High band by
   submission order; the change plan is what gets omitted).

Stamp `structural-final-2026-08-06`, budget 4 000, **seven** tasks (4
identifier + 3 structural):

| arm | tokens | facts |
| --- | ---: | ---: |
| `naive` | 192 246 | 42/42 |
| `weavatrix-raw` | 75 771 | 23/42 |
| `weavatrix-planned` | 33 793 | 33/42 |
| `cortex-targeted` | **22 165** | **33/42** |
| `cortex-full` | 25 338 | **33/42** |

**Token savings still hold.** Against naive, `cortex-targeted` is ~88 % fewer
tokens at 33/42 facts. Against the unplanned `cortex-loom` fixed-ops arm
(24 916 tokens, 21/42), the planned path is both cheaper and denser. Against
`weavatrix-planned` (same ops, no compiler), the compiler saves another ~34 %
at identical recall.

### Structural tasks alone

| task | targeted tokens | targeted facts | full tokens | full facts |
| --- | ---: | ---: | ---: | ---: |
| `compile-context-blast-radius` | 3 965 | **6/6** | 3 965 | **6/6** |
| `skills-compile-contract` | 2 106 | 5/6 | 3 141 | 5/6 |
| `mcp-transport-readers` | 2 132 | 4/6 | 3 168 | 4/6 |

On blast radius, the planned path reaches full recall where the unplanned
`cortex-loom` arm manages 2/6 — graph dependents win outright when asked for.
On the contract/transport tasks, `list_endpoints` + `search_code` recover the
route and handlers once prose acronyms stop polluting the search regex — see
`cortex-source` below; at stamp `structural-final-2026-08-06` those misses were
still open because `|HTTP` hid the real hits.

### `verified_change` on the structural set

**Still no extra facts.** After the priority fix, `cortex-full` matches
`cortex-targeted` recall (33/42) while costing 3 173 more delivered tokens.
On blast radius the two arms are equal (3 965, 6/6) because the compiler
omits the entire `WX-VERIFY-*` split under budget and keeps dependents. On
the contract tasks, full spends ~1 000 extra tokens for the same 5/6 and 4/6.
Proven: forcing the change plan does not raise structural anchor recall.
Not proven: that a human reader gains nothing from the plan text — anchors
still cannot score advice.

## `cortex-source` — bounded `read_source` on search hits

Stamp `source-followup-fixed-2026-08-06`, budget 4 000, seven tasks. A new arm
runs the targeted plan, then opens ranked `read_source` windows on the files
`search_code` hit (product `.rs`/`.ts` preferred over docs/bench fixtures).

Two search bugs had to be fixed before the arm could see the missing facts:

1. **Prose acronyms.** `HTTP` (and similar all-caps tokens without `_`) were
   treated as identifiers, so the regex became `/api/skills/compile|HTTP` and
   drowned the UI client hit. Acronyms are no longer identifiers.
2. **Rust-regex escapes.** Escaping `/` as `\/` is rejected by Rust's `regex`
   crate; only real metacharacters are escaped now.

| arm | tokens | facts |
| --- | ---: | ---: |
| `naive` | 195 906 | 42/42 |
| `cortex-targeted` | **21 431** | **35/42** |
| `cortex-source` | 26 320 | **36/42** |
| `cortex-full` | 25 637 | 35/42 |

### Structural tasks after the search fixes

| task | targeted | source | naive |
| --- | ---: | ---: | ---: |
| `skills-compile-contract` | **1 475 / 6/6** | 3 189 / 6/6 | 30 438 / 6/6 |
| `mcp-transport-readers` | **2 219 / 6/6** | 3 745 / 6/6 | 24 677 / 6/6 |

**Verdict.** The acronym/escape fixes closed the contract and transport misses
on `cortex-targeted` alone — no source follow-up required for those fixtures.
`cortex-source` recovers one extra identifier-set fact (`priority-ordering` on
`evidence-priority-band`) at roughly 5 000 more delivered tokens than targeted,
still ~87 % below naive. Source follow-up is insurance for identifier-adjacent
facts, not the primary structural fix.

## Source follow-up on the MCP path

Stamp `p0-source-skip-verify-final-2026-08-09`, budget 4 000, ten probe tasks.
The MCP compile path now uses bounded source follow-up. Identifier, blast,
API-contract, module-topology, and runtime-config tasks no longer spend budget
on `verified_change` unless the task explicitly asks for a change plan.

| arm | tokens | facts | recall |
| --- | ---: | ---: | ---: |
| `naive` | 264 567 | 40/40 | 100 % |
| `cortex-targeted` | **20 881** | 27/40 | 68 % |
| `cortex-source` | **30 973** | **33/40** | **83 %** |

Compared with `probe-10-recheck-2026-08-09`, source follow-up recovers two
additional facts while delivering 1 598 fewer tokens. The source arm remains
88 % below naive. Both runtime-config failures improved to at least 50 % recall:
`llm-profile-gate` reaches 3/4 and `shadow-handle` reaches 2/4.

## Skill-guided gather and verification

Stamp `p1-skill-gather-verify-final-2026-08-09`, budget 4 000, ten probe
tasks. The source arm now exercises the same gather/sufficiency/retry path as
the MCP tool (without an active-skill override).

| arm | tokens | facts | recall |
| --- | ---: | ---: | ---: |
| `naive` | 276 452 | 40/40 | 100 % |
| `cortex-targeted` | **21 091** | 27/40 | 68 % |
| `cortex-source` | **30 759** | **32/40** | **80 %** |

All ten gathered probe packets passed the structural sufficiency check without
a retry. That is not the same as anchor parity: `store-module-map` remains
2/4, so the report must not be presented as answer completeness. A separate
live UI-only probe starts with a deliberately empty Rust-only search for
`compileMarkdown`; the single wide retry finds `ui/src/api/client.ts`, opens a
bounded source window, and compiles to 1 815 estimated tokens versus 3 247 for
that one known whole file (44 % less). The point of the retry is recovery from
an obviously thin gather, not repeated searching until recall looks good.

## Semantic sufficiency and one contract retry

Stamp `p2-semantic-sufficiency-final2-2026-08-09`, budget 4 000, ten probe
tasks. Source verification now checks task-specific evidence terms instead of
equating "a source read exists" with sufficiency. A thin packet gets one retry
that replays the whole semantic contract, then stops and reports any remaining
gap. Distant hits in one file retain separate windows, and explicit lowercase
backticks plus templated URL paths remain searchable identifiers.

| arm | tokens | facts | recall | versus naive |
| --- | ---: | ---: | ---: | ---: |
| `naive` | 280 910 | 40/40 | 100 % | - |
| `cortex-targeted` | **22 607** | 28/40 | 70 % | **-92.0 %** |
| `cortex-source` | **36 105** | **40/40** | **100 %** | **-87.1 %** |

Compared with skill-guided gathering, source costs 5 346 more delivered tokens
and recovers eight more facts (32/40 -> 40/40). Compared with the first semantic-retry attempt it
recovers the last three facts with 14 fewer tokens: retrying the complete
contract fixed `ProfileRegistry` and `ShadowHandle` without another pass.
Every source packet is structurally sufficient after at most one retry.

The quality target (at least 32/40 and no env/config task below 50 %) passes;
the approximately 30k aggregate token target does not. Dropping raw search
snippets or retaining both initial and retry windows was measured and rejected
because each reduced recall while still approaching the 4k per-task ceiling.
Hot-path LLM compression remains the wrong lever.

The first run refreshed repository evidence, so cold-to-warm JSON was not byte
identical. Two subsequent warm runs with the same stamp were byte identical
(SHA-256 `1E2A44C29001354C4830C4E3B333F71BD039C6A501BDA16CA93936F35EEFB70C`).

## Editable sequences and current self-benchmark — 2026-08-09

The sequence harness compares four methodology-context arms on 28 declared
quality and safety scenarios. It is separate from repository retrieval: the
same synthetic evidence identity is held constant, and only methodology
changes.

| arm | methodology tokens | hard scenarios passed |
| --- | ---: | ---: |
| `none` | 28 | 0/28 |
| `cortex-current` | 10 401 | 3/28 |
| `superpowers-raw` 6.2.0 | 72 839 | 15/28 |
| `cortex-native` | **3 812** | **28/28** |

`cortex-native` transmits only one `ActiveStepPacket`; the typed graph keeps
the remaining gates, recovery paths, evidence requirements, and upstream
handoffs outside the prompt. It therefore uses **63.35% fewer methodology
tokens than current Cortex skills** and **94.77% fewer than raw Superpowers**
without losing a declared scenario. The default-promotion gate passes.

> **Read the 94.77 % carefully.** It compares **one** `ActiveStepPacket`
> (~142 tokens) against **one whole** `SKILL.md` (~2 502 tokens). Each
> scenario scores a single step; the fixtures reach `step-6`, so executing a
> complete sequence sends five or six packets, roughly 715–860 tokens. The
> honest amortized figure against raw Superpowers is therefore about
> **66–71 %**, not 94.77 %. The per-step number is the right one for "what
> enters context at this moment" and the wrong one for "what the workflow
> costs".
>
> **And the scoring is not symmetric.** `cortex-native` is scored from its own
> typed graph — `node.config["requiredEvidence"]` and node kinds read straight
> off the instantiated template — while the prose arms are scored by keyword
> inference over their text (`infer_node_kinds`, `infer_evidence` in
> `sequence_arms.rs`). The fixtures' `requiredNodeKinds` and `requiredEvidence`
> are written in the same vocabulary the templates declare, and the harness
> hands native the template named by `expected.sequenceId`. Only
> `selected-sequence` independently tests routing, and that check auto-passes
> for every other arm. **28/28 is therefore closer to an internal consistency
> check than to a comparison**, and the gap to `superpowers-raw` measures how
> often English prose happens to contain the expected keywords.

This is a structural methodology benchmark, not proof that an LLM completed a
coding task. Raw prose is normalized into declared capabilities for scoring;
native sequences are scored from their typed graph. Every report records the
external root label, version, LICENSE SHA-256, and SHA-256 of all 14 upstream
`SKILL.md` inputs. Promotion now fails closed unless the current, raw, and
native arms are all available, and native has zero check regression against
both baselines. The two final reports are byte-identical with SHA-256
`0F3D4C703D16202F8C566EF37D7C8AAB32274C5709018B5474E0A22CC2996D9B`.

The optional live-model gate inherits that verdict: it also requires the
deterministic report to be promoted, recreates an available raw methodology
packet from the explicitly supplied Superpowers root, and checks paired losses
against both current and raw. Missing external input is an evaluated
fail-closed result, never a promotion.

### Paired live smoke: no promotion for the 4B model

The optional live suite ran one representative scenario for three alternating
repetitions across all four arms: 12 calls to the already-installed
`qwen3.5:4b` Ollama profile. No model was downloaded.

- exact gate: **0/12**;
- paired regressions from current to native: **0** (both failed exact recall);
- p95 latency: **86 794 ms**;
- escalation and `claimCompletion=false` were preserved, but required facts
  were paraphrased instead of copied exactly and decoy facts sometimes leaked.

Verdict: the 4B profile is not promoted for sequence work and is far outside a
hot-path budget. Live output remains evaluation data with no routing or run
authority.

### Final repeated repository-context benchmark after review

Stamp `probe-final-reviewed-2026-08-09`, budget 4 000, ten probe tasks. The
review fixed four measurement/product defects: promotion could pass without
the raw baseline, the live model gate compared native only with current, the
MCP caller anchor accepted a module declaration instead of a real call, and
source packing could let search metadata evict a verified runtime/config fact.
Runtime-flag retry now derives a task-specific search
stem (`ShadowHandle` → `CORTEX_*SHADOW`), and the compiler admits the already
bounded direct source pool before high-volume search metadata.

The probe's anchor literals are assembled at compile time from split pieces;
none of the materialized strings exists in `probe_tasks.rs`. A regression test
checks every candidate against the fixture source. Weavatrix therefore cannot
earn a fact by retrieving the benchmark's own answer list.

The current repository is larger than the earlier P2 snapshot, so the naive
baseline changed. Two runs with the same stamp are byte-identical; volatile
native `graph_stats.build_ms` telemetry remains excluded from evidence packets
(SHA-256 `0519EEF64B519CBD754155B6C8AE53329D2F1F75A0F41D6502DCE4300329653C`).

| arm | tokens | facts | recall |
| --- | ---: | ---: | ---: |
| `naive` | 310 625 | 40/40 | 100% |
| `weavatrix-raw` | 87 462 | 26/40 | 65% |
| `cortex-loom` | 36 039 | 24/40 | 60% |
| `weavatrix-planned` | 20 120 | 28/40 | 70% |
| `cortex-targeted` | **20 349** | **28/40** | **70%** |
| `cortex-full` | 21 124 | 28/40 | 70% |
| `cortex-source` | **34 564** | **40/40** | **100%** |

## As an MCP — what a caller actually pays

Everything above measures `packet.content`: the evidence the compiler selected.
That is not what an agent is billed for. `weavatrix_context_compile` returns the
whole `CompiledEvidenceBundle` as JSON, so the packet arrives escaped and
carrying its warnings, sufficiency report, citation ids and counters. Stamp
`final-quality-2026-08-11`, same ten tasks, budget 4 000, on
`weavatrix-rust` 2.5.0:

| arm | selected | delivered over MCP | facts |
| --- | ---: | ---: | ---: |
| `cortex-loom` | 35 350 | 40 070 | 23/40 |
| `cortex-targeted` | 23 287 | **27 571** | 28/40 |
| `cortex-full` | 27 111 | 31 870 | 28/40 |
| `cortex-source` | 32 623 | **37 773** | 40/40 |

**Serialization costs 13–18 % across these aggregate arms on top of the
selected evidence.** `cortex-source` against a 319 564-token naive baseline
is −89.8 % selected and −88.2 % delivered. Two further costs are real and are *not* in the table: an MCP
client that reads both `content` and `structuredContent` pays the payload
twice, and the server's tool schemas are a standing charge before any call is
made — see the profile table below.

### Tool schemas are a standing charge, and `tools/list` paginates

`tool_page_size(16)` means a single `tools/list` reply is not the surface. The
client follows `nextCursor` and loads every page into context for the whole
session. Measured on this workspace:

| profile | tools | pages | schema tokens |
| --- | ---: | ---: | ---: |
| `--profile full` (default) | 27 | 2 | **4 021** |
| `--profile context` | 2 | 1 | **454** |

The context profile registers `context_compile` and
`weavatrix_context_compile` and nothing else. A caller that only wants
evidence was paying 4 021 tokens per session for twenty-five tools it never
called; it now pays 454. Routing, runs, graphs, skills, and sequences need the
full profile.

### One question, every approach

Same question — *who calls `compile_evidence_bundle`, and what breaks if it
starts refusing more packets?* — against four declared facts, every server
driven for real over JSON-RPC, measured 2026-08-10:

| approach | schema | payload | **session** | calls | facts |
| --- | ---: | ---: | ---: | ---: | ---: |
| naive: read the candidate files | 0 | 79 040 | 79 040 | — | 4/4 |
| agent-native: `ripgrep` + file reads | 0 | 4 904 | 4 904 | 5 | 3/4 |
| agent-native + Superpowers 6.2.0 | 0 | 8 037 | 8 037 | 5 | 3/4 |
| Serena MCP 1.28.1 (`ide-assistant`) | 6 283 | 4 257 | 10 540 | 3 | 4/4 |
| Weavatrix MCP 1.4.0 | 5 276 | 14 316 | 19 592 | 4 | 4/4 |
| Cortex Loom MCP, full profile | 4 021 | 9 318 | 13 339 | **1** | 4/4 |
| **Cortex Loom MCP, context profile** | **454** | 9 347 | **9 801** | **1** | **4/4** |

Warm latency for the same sessions: agent-native 42 ms of tool time across
five model turns; Serena 3 013 ms across three (and 45 s on the first call,
paying for the language server's index); Weavatrix 178 ms across four; Cortex
Loom **23.8 ms in one**.

Reading this table honestly:

- **The stack is what earns the saving.** Against reading the files, the
  context profile is **−87.6 % tokens at equal recall**. That is the number
  this project exists for, and Weavatrix earns most of it.
- **Turn count is the term nobody prices.** Every call is a model turn that
  re-reads the whole session. Four calls cost roughly four prefills plus their
  outputs, so a 19 592-token four-call session is far more than twice as
  expensive as a 9 801-token one-call session in practice.
- **Serena is the closest on tokens and is doing something different.** Its
  payloads are the leanest here (4 257) because LSP answers are precise and
  small, and it spends that advantage on a 6 283-token schema and three round
  trips. It is symbol-exact where the graph is heuristic; it pays a language
  server's index where the graph pays 98 ms.
- **Superpowers is orthogonal.** It adds 3 133 tokens of methodology
  (`using-superpowers` 766 + `systematic-debugging` 2 367) and **no
  repository facts** — recall stays 3/4. It is not a retrieval competitor and
  should not be read as one.
- **`agent-native` is the cheapest arm that gets the wrong answer.** 4 904
  tokens and 3/4: `rg` without context lines returns the call site but not the
  enclosing symbol, so "who calls it" is answered as a file rather than as a
  function.

## Live model, unfamiliar repository — what a 9B actually answers

Measured 2026-08-10 on `weavatrix-search` at `50953b3` with `qwen3.5:9b`
(temperature 0, thinking off, `num_predict` 400, one shot). Three questions of
rising difficulty; ground truth read out of the source by hand; answers graded
against required claims, not against literal presence in context. Every MCP
server ran for real (`weavatrix@1.5.0`, Serena 1.28.1, Cortex `--profile
context` on mcport 0.5.0), and the client ingested one representation per
reply.

Score is required claims present in the model's *answer*, summed over the
three tasks (3 + 4 + 5 = 12):

| approach | quality | session tokens | calls | model prefill (tok) |
| --- | ---: | ---: | ---: | ---: |
| no-context (control) | 1/12 | 0 | 0 | 343 |
| **naive: read the module dirs** | **10/12** | 17 580 | 0 | 18 985 |
| agent-native (`rg` + windows) | 7/12 | 23 327 | 12 | 27 824 |
| agent-native + Superpowers | 8/12 | 34 279 | 12 | 36 846 |
| Serena MCP | 5/12 | 20 768 | 6 | **2 503** |
| weavatrix MCP | **10/12** | 40 680 | 9 | 21 150 |
| cortex MCP (one packet) | 4/12 | **8 779** | **3** | 7 933 |

Per difficulty: easy `T1` — naive 3/3, weavatrix 3/3, serena 3/3, cortex 2/3;
medium `T2` — naive/weavatrix/agent 4/4, cortex 2/4, serena 2/4; hard,
cross-cutting `T3` — naive/weavatrix 3/5, agent 2/5, **serena 0/5, cortex
0/5**.

What this run establishes, uncomfortable parts first:

- **Cortex packets are the cheapest and the thinnest.** Best session cost
  and round-trip count in the table, and the worst retrieval-arm quality,
  collapsing to 0/5 on the cross-cutting question. The `T3` packet used
  2 347 of its 4 000-token budget — selection stopped early and the
  sufficiency gate accepted it. Same defect family as the implementation
  benchmark below: **the gate passes packets that are too thin for the
  question.** The lever is not more budget; it is spending the granted
  budget when the intent is broad, and failing sufficiency otherwise.
- **On a small crate, reading the module is the quality king.** 10/12 at
  17.6 k tokens. The premise "reading files whole is wasteful" is a
  large-repository premise; on a 10 k-line crate the module *is* the right
  packet. Cortex's economics argument starts where repositories stop
  fitting in a context window.
- **Serena is the prefill champion and breadth-limited.** 2 503 prefill
  tokens across all three tasks — an order of magnitude under everyone —
  because LSP answers are exact symbol bodies. That exactness is also why it
  scored 0/5 on the question whose answer lives across options, feature
  gates, and path handling.
- **Weavatrix's raw dump ties naive on quality** at 2.3× naive's session
  cost and 4.6× cortex's: breadth wins answers and costs tokens; nothing in
  between exists yet.
- Caveats that keep these numbers honest: one shot per cell, one small
  model; `agent-native` is a scripted lower bound (fixed first-160-line
  windows, which on `T1` cut the very definition an agent would have
  opened); `T3` generation hit the 400-token cap on three arms; and model
  wall-clock varied up to 400× per prefill token between cells (Ollama
  GPU/CPU placement drift), so cross-arm timing claims rest on token
  counts, not milliseconds.

## Rendering graph evidence — 2026-08-12

Profiling a compiled packet fragment by fragment found the largest single
cost in the product, and it was not retrieval:

| fragment | T1 | T2 | T3 | shape |
| --- | ---: | ---: | ---: | --- |
| `WX-SYMBOL` (`context_bundle`) | 1 209 | 2 075 | 1 455 | **one minified JSON line** |
| `WX-SEARCH` (`search_code`) | 405 | 230 | 301 | one minified JSON line |
| everything else (source, definitions, types) | 965 | 967 | 2 186 | readable code |
| packet total | 2 579 | 3 272 | 3 942 | |

`render.rs` already turned `read_source` into plain lines; every other native
answer fell through to `value.to_string()`. So **37–63 % of each packet was
JSON structure** — and it was the same escaped-JSON form the T3 experiment had
already shown a 9B model refuses to reason over. One neighbour cost ~123
tokens to state one relationship:

```
{"direction":"outgoing","node":{"id":"symbol:src/collector/mod.rs#struct:Collector@25:19",
 "kind":"struct","label":"Collector","span":{"end":{"column":2,"line":46},...}},
 "provenance":{"confidence":"high","detail":"resolved through an import...",
 "extractor":"weavatrix.rust.syn","span":{...}},"relation":"references"}
```

The same facts as a line:

```
  -> references Collector (struct) src/collector/mod.rs:25 via src/multiline/mod.rs:149
```

Search results render as `path:line: text` — the grep shape every agent
already reads. Rendering is lossless in facts: every label, kind, path, line,
relation and evidence site survives. What is dropped is JSON syntax, node ids
that restate `file:line:kind:label`, column offsets, and the extractor name.

Measured, same stamps, same budget, no other change:

| | before | after |
| --- | ---: | ---: |
| `WX-SYMBOL` share of packet | 1 209 / 2 075 / 1 455 | **204 / 346 / 252** (−83 %) |
| probe `cortex-targeted` | 23 293 @ 28/40 | **10 536 @ 28/40** (−55 %) |
| probe `cortex-source` | 32 676 @ 40/40 | **21 363 @ 40/40** (−35 %) |
| probe `cortex-source` delivered over MCP | 37 827 | **24 323** |
| implementation packet (`ArchiveOptions`) | 2 946, six fields | **1 242, six fields** |
| live quality session, three questions | 11 245 @ 10/12 | **6 945 @ 9/12** |

**No recall regression on the deterministic sets.** The one behavioural
coupling this touched was sufficiency's hit detector, which sniffed for the
`"path"`/`"line"` JSON keys to decide whether a search fragment carried hits;
it reads the rendered header now and still recognises the legacy JSON form.

Freeing that much budget exposed two gathering bugs, both found by reading
the warnings rather than the totals:

- A broad question was compiling 2 518 of 4 000 tokens. The source pool for
  enumerating intents moved from three fifths of the budget to four, with
  120-line windows: under-spending a granted budget on the one intent that
  asks for breadth is the opposite of what that window exists for.
- `definition read skipped: no defining hit for Archive`. Candidate
  extraction reads names out of code, so it also proposes *fragments* of
  names; `Archive` defines nothing, and it had consumed one of four
  expansion slots. A name that resolves to no definition now costs a lookup
  instead of a slot, ties break toward the longer name, and each round has
  its own read cap — without that cap the first round spent every slot and
  the second hop, the one that finds a type through an intermediate, never
  ran.

Where this leaves the field, three questions, `qwen3.5:9b`, one shot:

| approach | quality | session tokens | calls |
| --- | ---: | ---: | ---: |
| **Cortex Loom, context profile** | **9/12** | **6 945** | **3** |
| naive: read the module dirs | 10/12 | 17 580 | 0 |
| Serena MCP | 5/12 | 20 768 | 6 |
| agent-native (`rg` + windows) | 7/12 | 23 327 | 12 |
| agent-native + Superpowers | 8/12 | 34 279 | 12 |
| weavatrix MCP, raw dump | 10/12 | 40 680 | 9 |

**Cheapest in the field by 2.5×, and cheaper than every arm it also
out-answers.** It beats Serena, the native agent, and Superpowers on both
axes at once; it remains one point behind naive and the raw graph dump, at a
quarter and a sixth of their cost. T3 alone has scored 0, 2, 2, 3 and 3 of 5
across otherwise identical repeats, so the one-point gap is inside the
one-shot noise this harness can resolve — which is a statement about the
harness, not a claim of parity.

## The fix round — 2026-08-11

The three failures above shared one root: the sufficiency gate accepted
packets that were too thin for their question. Three changes landed, each
carried by a measurement:

1. **The named symbol's definition is read whole.** A dedicated
   `read_source` starts at the definition head and widens once if the braces
   do not balance (`WX-DEF`, critical). Sufficiency now requires
   `definition:<symbol>` — present only when one fragment carries the
   complete, brace-balanced definition — and the one permitted retry re-reads
   it with a doubled window. Clipped duplicates of a definition are pruned
   once the complete read exists, so the model never sees the four-field
   prefix after the six-field truth.
2. **Broad questions spend the budget.** Enumerating cues ("list every
   mechanism", "silently cause") widen the source follow-up (9 files, 84-line
   windows, 3/5 budget pool instead of 2/5).
3. **Second-hop type expansion.** Source windows are scanned for user-defined
   type names, ranked by task-word affinity before frequency, and the best
   candidates' definitions are read (`WX-TYPE-*`, two rounds, four reads
   max). Two rounds because the answering type is often visible only through
   an intermediate: the probe's windows never name `ArchiveOptions` — it
   surfaces as a field inside `SearchOptions`, which round one reads.
   Expansions are a new `type_expansion` evidence kind at High priority, not
   critical, so a full budget drops breadth instead of failing the compile.

Measured effect, same harnesses, same model, same one-shot protocol:

| check | before | after |
| --- | --- | --- |
| implementation: `ArchiveOptions::disabled()` | not compiling (4/6 fields) | **compiles, hidden test passes** (2 946 tok session) |
| T3 packet, cross-cutting facts present | 3/7 | **6/7** (3 925 of 4 000 budget used) |
| live quality, cortex arm | 4/12 | **9/12** (T1 3/3, T2 3/4, T3 3/5) at 11 245 session tokens |
| probe recall (`cortex-source`) | 40/40 @ 34 712 | **40/40 @ 32 628** |
| sequence gate | promoted | promoted |

That puts the cortex arm one point under the two 10/12 arms — naive
(17 580 tokens) and the raw weavatrix dump (40 680) — at 64 % and 28 % of
their session cost respectively, still in one round trip per question.

Two measurement notes that belong next to the number. First, the harness now
feeds the model `context.content` — the packet's actual deliverable — instead
of the JSON envelope; with identical facts in the packet, the envelope run
scored 0/5 on T3 because the model, instructed to use only the evidence,
declined to read mechanisms out of escaped JSON. Form is part of quality. A
correct client consumes the packet, so the harness does; the envelope still
counts toward wire cost. Second, one-shot variance is visible at this scale:
T2 moved between 4/4 and 3/4 across repeats (the model paraphrasing
`finish_block` instead of naming it).

### Form, isolated: labels × consumption — 2026-08-12

Two form-only levers, no change to which facts the packet carries, measured
as a 2×2 on the same three questions:

- **definition labels** — definition reads render their section header as
  `weavatrix:read_source definition:<Type>`, so the packet *says* which
  fragment is the definition the question turns on;
- **packet consumption** — the model reads `context.content` (plain text)
  against the raw JSON envelope a naive client pastes today.

| quality /12 | envelope client | packet client |
| --- | ---: | ---: |
| unlabeled | 7 (T3 0/5) | 9 |
| **labeled** | **9** (T3 2/5) | **10** |

Both levers are worth about two points each and they stack. Labeled + packet
reaches **10/12 — even with `naive` at 64 % of its session tokens, even with
the raw weavatrix dump at 27 % of its cost, in one call per question.**
Labels are the striking half: they recovered T3 from 0/5 to 2/5 *through*
the escaped JSON, purely by naming what each fragment is.

Labels shipped (`WX-DEF`/`WX-TYPE-*` headers). The consumption lever cannot
ship from this repository: mcport 0.5 serializes every success value into
the text block, so a plain-text first block needs a `ToolReply::plain_text`
constructor upstream. The measured payoff for whoever pastes tool results
verbatim — every current agent client — is the envelope column above.

The residual T3 misses are `enabled` (present in the labeled
`ArchiveOptions` expansion, still not surfaced in any answer — an
answer-side limit of the 9B at temperature 0) and `safe_virtual_path`: a
private function no window opens because nothing on the gathered paths
references it by name. That is the current boundary of deterministic
gathering, recorded rather than papered over.

## Implementation, end to end — one approach per isolated worktree

Measured 2026-08-10 on `weavatrix-search` at `50953b3`, a repository neither
the harness author nor the model had worked in. Task: add
`ArchiveOptions::disabled()` returning a configuration with `enabled: false`
and **every** limit field zero. The struct has six fields of mixed types
(`u64` and `usize`), so compilation directly measures whether the evidence
carried the complete definition. Each approach ran in its own `git worktree`,
the model (`qwen3.5:9b`, temperature 0, thinking off, one shot) saw only that
approach's evidence, and a **hidden harness test the model never saw** was the
arbiter: `..Default::default()` would compile and still fail it.

| arm | session tokens | calls | prefill | generation | compiled | hidden test |
| --- | ---: | ---: | ---: | ---: | :-: | :-: |
| agent-native (`rg` + defining file whole) | **1 788** | 2 | 2 264 tok | 78 tok | **yes** | **pass** |
| weavatrix-mcp (search+inspect+read) | 10 788 | 3 | 3 592 tok | 60 tok | no | fail |
| cortex-mcp (one compiled packet) | 2 401 | 1 | 2 217 tok | 60 tok | no | fail |

(The MCP rows are the re-run on the updated stack — `weavatrix@1.5.0`,
mcport 0.5.0 single-representation replies; the first run's numbers were
9 700 / 4 658 with the mirrored payloads and additionally failed on syntax.)

**The plain agent won outright, and it was also the cheapest.** Reading the
defining file whole cost 1 788 tokens and produced the only compiling,
passing implementation.

Why the MCP arms failed is more useful than the scoreboard. On the updated
stack both models produced a clean `impl ArchiveOptions` block — and **both
omitted the same two fields**, `max_entries` and `max_decoder_memory_bytes`.
Replaying the exact cortex call proves the fields were absent from the packet
itself, and the weavatrix arm's evidence truncated at the same boundary:
**both products inherit the same `weavatrix-rust` symbol window, and that
window cuts the struct body**. The cortex sufficiency check accepted the
truncated packet anyway. A packet can therefore pass sufficiency while
truncating the definition of the very symbol the task names. The fix
direction is shared: implementation-intent evidence must carry the named
symbol's complete definition, and Cortex's sufficiency must fail when it
does not. One shot on one small model; a frontier model might guess the
missing fields from the doc comment — the local-first premise is exactly
that it must not have to.

**Post-fix rerun, 2026-08-11.** Cortex now requests the complete owner
definition for creation tasks, rejects an incomplete named definition, keeps
retry search inside source/config trees, renders `read_source` as plain source,
and removes a truncated source duplicate once a complete definition exists.
The same real stdio MCP call and the same one-shot model produced a 97-token
`impl` containing all six fields:

| selected evidence | model prefill | generation | sufficient | upstream | compiled | hidden test |
| ---: | ---: | ---: | :-: | :-: | :-: | :-: |
| **2 310** | 2 669 | 97 | **yes** | no | **yes** | **pass** |

The hidden test was applied only after generation in a detached worktree and
asserted `enabled == false` plus zero for every limit. This closes the specific
false-positive sufficiency defect above; it does not turn the earlier one-shot
comparison into a general model-quality claim.

## Symbol resolution: graph against LSP, against a hand-auditable truth

Measured 2026-08-10 on `weavatrix-search` at `50953b3`, in an isolated
worktree. Ground truth: every textual occurrence of six symbols, mechanically
classified (definition / import / comment / reference) with each reference
mapped to its enclosing function; the classification table is stored with the
results so it can be audited line by line. Unit of comparison: the referencing
function, which is what both tools return. Weavatrix `get_dependents` is
scored on `distance == 1` only; Serena `find_referencing_symbols` on
non-`File` entries.

| symbol | truth | Weavatrix 1.5.0 | Serena 1.28.1 | wx ms | serena ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| `read_limited` | 4 | **4/4** | 4/4 | 8 | 14 925 (cold) |
| `finish_block` | 1 | **1/1** | 1/1 | 18 | 400 |
| `quiet_match` | 2 | 1/2 | **2/2** | 20 | 480 |
| `safe_virtual_path` | 2 | 1/2 | **2/2** | 15 | 356 |
| `search_expanded_file` | 2 | 1/2 | **2/2** | 18 | 768 |
| `ArchiveOptions` (struct) | 2 | 0/2 | **2/2** | 15 | 3 383 |
| **total** | **13** | **8/13 (62 %)** | **13/13 (100 %)** | 8–20 | 356–3 383 warm |

Precision was 1.0 for both sides after the direct-only filter; Serena's two
"extras" on `ArchiveOptions` are real references the mechanical truth cannot
represent (an `impl Default` block and a struct field), so its effective
precision is also 1.0.

What the misses are, specifically — this is the actionable part:

- Free-function **call** edges are fully resolved by both (`read_limited`
  4/4, `finish_block` 1/1).
- Weavatrix missed one **method call through a field receiver**
  (`collector.quiet_match(...)` inside `process_line`), one call in
  `search_zip`, one in `dispatch.rs::expanded`, and **both type references**
  to the struct (`fn default`, `fn with_archives`). The graph holds a
  separate `references` relation, so the data may exist while
  `get_dependents` only walks call edges — a tool-semantics gap rather than
  missing extraction.
- The trade is now quantified rather than asserted: **the graph answers in
  8–20 ms where the LSP takes 0.4–3.4 s warm and ~15 s cold; the LSP resolves
  13/13 where the graph resolves 8/13.** Serena is symbol-exact and pays a
  language server; Weavatrix is instant and misses reference-kind edges on
  this repository. Cortex inherits whichever engine it stands on — one more
  reason its sufficiency checks must not assume the evidence under them is
  complete.

**Addendum, 2026-08-12, `weavatrix@1.7.1` / `weavatrix-rust` 2.5.1.** The
re-run reproduces 8/13 for default `get_dependents` (now at 7–9 ms) — but the
missing edges are *in the graph*: `get_neighbors ArchiveOptions` returns both
missed type-referencing functions in ~1.8 k tokens. The gap is the default
tool's semantics, not extraction. Cortex now compensates on its side: a
blast-radius plan for a `PascalCase` symbol adds `get_neighbors` alongside
`get_dependents`, verified live — the struct packet carries `with_archives`
and the `Default` impl at 3 982/4 000 tokens. The op is gated to type names
because adding it unconditionally evicted search hits under the budget and
cost two probe anchors on function-symbol tasks; measured, reverted, gated.

## git: CLI against Weavatrix git operations

Same three questions an agent actually asks, both sides warm, 2026-08-10.
`git` numbers are what an agent pastes into context; Weavatrix numbers are
the tool result on the wire.

| question | git CLI | Weavatrix MCP 1.4.0 |
| --- | ---: | ---: |
| last 25 commits | 306 tok / 115 ms | 7 416 tok / 44 ms |
| what changed vs HEAD~10 | 1 651 tok / 192 ms | 125 467 tok / 251 ms |
| hot files + co-changes, 6 months | 7 669 tok / 291 ms | 7 433 tok / **79 ms** |

Reading it honestly: **Weavatrix wins latency** (44–79 ms against 115–291 ms
— the CLI pays process spawn and pager plumbing every call), and on the
analytical question it also wins content: `git_history` returns computed
hot-file and co-change tables at the same token cost as the raw `--numstat`
dump the agent would still have to aggregate itself. For the plain-history
question the CLI is 24× cheaper on tokens because `git_history` always
attaches its analytics. And `graph_diff HEAD~10` returned **125 k tokens**
without a budget parameter — a structural diff that would evict half a
session; it needs the same `token_budget` treatment `search_code` already
has. `git worktree add` itself: 7.0 s cold, ~1.2 s each warm.

### Weavatrix 1.1.2 → 1.4.0 → 1.5.0

Measured back to back on the same machine, same repository, same queries:

| | 1.1.2 | 1.4.0 | 1.5.0 |
| --- | ---: | ---: | ---: |
| tools / schema tokens | 38 / 4 665 | 41 / 5 276 | 42 / **8 283** |
| `open_repo` payload | 3 169 | **606** | 610 |
| `open_repo` | 106.4 ms | 228.1 ms | 110.2 ms |
| `search_code` | 6.5 ms | 31.6 ms | 15.5 ms |
| `get_dependents` | 0.22 ms | 16.7 ms | 9.2 ms |
| `inspect_symbol` | 0.94 ms | 22.0 ms | 10.5 ms |

1.4.0 cut the `open_repo` reply by 81 % but regressed latency 5–75× on every
operation. **1.5.0 repairs most of the regression** (2–3× above 1.1.2 rather
than 5–75×) and keeps the lean `open_repo`. Two remaining observations for
the maintainer: the tool schemas grew to 8 283 tokens — a 57 % higher
standing charge per session than 1.4.0 — and `content` + `structuredContent`
carry two *different* renderings of each result (text ~7.3 k chars,
structured ~4.8 k for the same `search_code` answer), so a client that
ingests both still pays ~1.5× one representation.

For reference, `ripgrep` 15.0.0 answers the same literal query on this
repository in **19.7 ms** warm.

### mcport 0.5.0: the mirror is now opt-out, and Cortex opted out

MCP recommends mirroring `structuredContent` into a text block; mcport ≤0.4
always did, so every Cortex reply carried its payload twice. mcport 0.5.0
added `ToolPayload::{Text, Mirrored, Structured}`, and every Cortex tool now
replies `ToolReply::text` — one compact representation, readable by any
client. Measured on the standing example call
(`weavatrix_context_compile`, cortex-loom, budget 4 000):

| | mirrored (≤0.4 behaviour) | `text` (now) |
| --- | ---: | ---: |
| reply payload | 9 347 tok | **5 080 tok** |
| session with context profile | 9 801 tok | **5 566 tok** |

The "delivered over MCP" columns elsewhere in this document predate this
change and now represent an upper bound roughly 1.8× the current wire cost.

On the final 2026-08-11 rerun, targeted is 92.7% below naive. Source
verification adds twelve facts for 9 336 selected tokens and is 89.8% below
naive (88.2% by delivered MCP size). All ten source tasks are 4/4, including
both env/config probes. Two runs with the same stamp produced byte-identical
reports (SHA-256
`B2C650464DCCD233E0186C50A0FC8ACCD7F455B178092A33D52BE237F887BE6B`).
The aspirational `~30k` source cost is still missed by 2 623 selected tokens,
so the next lever remains more selective direct-source packing—not hot-path
model compression.
