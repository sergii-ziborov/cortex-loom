# Context benchmark

`cortex-bench` measures one thing, on purpose:

> For one engineering task on one repository, how many context tokens reach the
> upstream agent, and how many of the facts the task needs are in them?

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
cargo run -p cortex-bench -- --repo . --budget 16000 --out .cortex-loom/bench/report-16k.json
cargo run -p cortex-bench -- --list
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

**Weavatrix earns the recall.** Planning which of its 42 operations to call
moved facts from 14/24 to 18/24; the compiler cannot invent evidence that was
never fetched.

**Cortex Loom earns the tokens.** The same planned evidence costs 52 087
tokens under Weavatrix's own budgeting and 14 898 through the compiler — **71 %
fewer at identical recall, 18/24 either way.** The reason is structural rather
than clever: each of five operations budgets its own answer with no knowledge
that the others ran, so none of them can spend a low-value fragment's share on
a high-value one. Only the layer holding every fragment can rank them against
each other and stop when the budget is spent.

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

## P0: source follow-up on the MCP path

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

## P1: skill → gather → verify

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

## Next measurements worth taking

1. Recover the remaining identifier-set misses (`CriticalItemExceedsBudget`,
   `unquote`, `heading_text`, `tools/list`, `RetryLimitTooLarge`) without
   approaching naive cost — likely tighter symbol windows, not more ops.
2. Score against real upstream consumption from the usage ledger
   (`usage_report`) instead of the character estimate.
