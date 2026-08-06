# cortex-skills

A round-trip compiler between readable `SKILL.md` Markdown workflows and
typed process graphs.

Import turns frontmatter, headings, numbered and checklist steps, and fenced
guidance into a validated graph with source provenance on every node. Export
turns a graph back into stable, human-editable Markdown. Editing either side
is safe: the round trip preserves workflow semantics.

## Why

Methodology written as Markdown is easy for humans and useless to a program;
a workflow encoded as a graph is the opposite. This crate lets one artifact
be both.

- **Provenance on every node** — source file, line, and a content digest.
- **Typed steps and dependencies** — sequence edges between steps, plus
  explicit dependency edges from `[depends: 2, 3]` or `after step 2`.
- **Stable export** — exporting twice is byte-identical; import → export →
  import preserves node semantics and edge counts.

```rust
use cortex_skills::{export_skill_markdown, import_skill_markdown};

let markdown = "---\nname: Evidence First\ndescription: Facts before change.\n---\n\
# Evidence First\n\n## Workflow\n\n1. Inspect the relevant files.\n\
2. Record evidence ids. [depends: 1]\n";

let graph = import_skill_markdown("SKILL.md", markdown).expect("valid skill");
let exported = export_skill_markdown(&graph).expect("exportable graph");

let reimported = import_skill_markdown("SKILL.md", &exported).expect("valid skill");
assert_eq!(exported, export_skill_markdown(&reimported).expect("stable"));
```

`export_skill_markdown` only accepts graphs this crate produced: it requires
`metadata["compiler"] == "cortex-skills"`, because export reads the node
roles and ordering that import writes.

## Bundled methodology

`bundled_skills()` returns the methodology shipped with the crate, so a
consumer starts from working workflows instead of an empty editor:

| skill | when it applies |
| --- | --- |
| Test-Driven Development | growing behaviour through failing tests |
| Systematic Debugging | a reproducible failure with an unknown cause |
| Grounded Review | reviewing a change against evidence and invariants |
| Evidence-First Change | any edit that has to be defensible afterwards |
| Blast Radius Analysis | deciding how large a change really is |
| Interface Contract Change | altering something a consumer already depends on |
| Dependency Upgrade | adopting behaviour someone else authored |
| Performance Investigation | a workload that is slower than its target |
| Incident Response | production is broken right now |
| Migration Cutover | traffic or data must move with a one-step rollback |
| API Versioning | a published contract already has outside callers |
| Flaky Test Quarantine | an intermittent test is blocking honest signal |
| Security Threat Model | a new trust boundary, secret, or privilege |
| Observability First | a path that will be hard to inspect once live |
| Data Migration | stored shape changes under live traffic |
| Feature Flag Rollout | staged exposure that must not become permanent debt |
| Documentation Sync | behaviour changed and docs would otherwise lie |
| Release Checklist | before tagging, publishing, or a production deploy |
| Backlog Triage | the queue is longer than the next planning window |
| Accessibility Audit | UI focus, names, or contrast changed |
| Configuration Drift | live config cannot be explained from the source |
| Cache Invalidation | a cache can serve wrong data after writes |
| Concurrency Bug Hunt | races, locks, or shared mutable state |
| Schema Evolution | persisted or wire formats must change safely |
| Dependency Audit | adding a package or responding to an advisory |
| Error Budget Review | SLOs exist and budget is being spent |
| Capacity Planning | load is rising or a launch multiplies traffic |
| Rollback Drill | practice the rollback before the risky window |
| Contract Testing | two sides share a schema that must fail in CI |
| Postmortem Writeup | after a mitigated incident, while memory is fresh |

```rust
for skill in cortex_skills::bundled_skills() {
    let graph = cortex_skills::import_skill_markdown(skill.source, skill.markdown)
        .expect("bundled skills compile");
    println!("{}: {} nodes", skill.id, graph.nodes.len());
}
```

The same documents are the crate's round-trip fixtures: whatever ships to a
consumer has to survive its own compiler, and a test fails if the library and
the fixture set ever drift apart.

## Importing somebody else's library

A compiler that can only read its own fixtures is a compiler nobody needs.
`import_library` takes documents a caller has already read and returns
validated graphs, with failures reported per document rather than failing the
whole library, and colliding titles disambiguated instead of dropped:

```rust
use cortex_skills::{LibraryEntry, import_library};

let import = import_library(
    vec![LibraryEntry {
        source: "skills/review/SKILL.md".to_owned(),
        markdown: "---\nname: Review\ndescription: Check it.\n---\n# Review\n\n- Read the diff.\n".to_owned(),
    }],
    Vec::new(),
    "/checkout/some-library",
);
assert_eq!(import.skills.len(), 1);
assert_eq!(
    import.skills[0].graph.metadata.get("library").map(String::as_str),
    Some("/checkout/some-library"),
);
```

The function is pure: walking a directory is the caller's job, which keeps
this crate free of filesystem and transport concerns. `LibraryImport::notices`
carries the licence and notice files a caller found beside the skills, so
attribution can be shown before anything is stored. Nothing here decides
whether a licence permits the use.

## Format notes

- `[kind: review_gate]` types a step. Without it a step is deterministic work;
  with it the step becomes that node kind, so a workflow can carry gates,
  escalation, branches, and an end instead of being a flat list. Hyphens and
  spaces are accepted (`review-gate`). The graph is canonical: change a node's
  kind in the editor and the next export writes the new annotation.
- `[depends: N]` is a **tail** annotation: put it at the end of the step
  line. Trailing punctuation after it is preserved as part of the label and
  will make the round trip report a changed label.
- Annotations never appear in a node label. Import strips them into the graph
  and export writes them back from it, so the label is the same text on the
  canvas, in the inspector, and in the Markdown.
- `after step N` and `depends on step N` are recognised as prose
  alternatives.
- Frontmatter keys other than `name` and `description` are preserved under
  `frontmatter.*` metadata.

The `SKILL.md` shape follows the portable format popularized by
[obra/superpowers](https://github.com/obra/superpowers) (MIT). The bundled
fixtures are original text written for round-trip testing; see
`fixtures/NOTICE.md`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
