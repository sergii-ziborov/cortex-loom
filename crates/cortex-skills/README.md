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

`bundled_skills()` returns the methodology shipped with the crate —
test-driven development, systematic debugging, and grounded review — so a
consumer starts from working workflows instead of an empty editor:

```rust
for skill in cortex_skills::bundled_skills() {
    let graph = cortex_skills::import_skill_markdown(skill.source, skill.markdown)
        .expect("bundled skills compile");
    println!("{}: {} nodes", skill.id, graph.nodes.len());
}
```

The same three documents are the crate's round-trip fixtures: whatever ships
to a consumer has to survive its own compiler.

## Format notes

- `[depends: N]` is a **tail** annotation: put it at the end of the step
  line. Trailing punctuation after it is preserved as part of the label and
  will make the round trip report a changed label.
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
