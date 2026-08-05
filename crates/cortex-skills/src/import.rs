use std::collections::{BTreeMap, HashMap, HashSet};

use cortex_domain::{
    EdgeKind, GRAPH_SCHEMA_VERSION, GraphDocument, GraphEdge, GraphNode, NodeKind, Position,
    Provenance,
};
use serde_json::Value;

use crate::SkillError;

#[derive(Default)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    extra: BTreeMap<String, String>,
}

#[derive(Clone, Copy)]
enum ListMarker {
    Numbered(u32),
    Checklist(bool),
    Bullet,
}

#[derive(Clone, Copy)]
struct ListItem<'a> {
    marker: ListMarker,
    text: &'a str,
    indent: usize,
}

struct Builder<'a> {
    source: &'a str,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    headings: Vec<(usize, String)>,
    steps: Vec<String>,
    dependency_hints: Vec<(String, Vec<usize>)>,
    order: usize,
    edge_count: usize,
}

/// Compile a Markdown skill into a validated, provenance-bearing graph.
pub fn import_skill_markdown(source: &str, markdown: &str) -> Result<GraphDocument, SkillError> {
    // A UTF-8 BOM is invisible and common in files written on Windows. Left
    // in place it makes the first line `\u{feff}---` instead of `---`, so the
    // frontmatter block is not recognised at all and the document silently
    // imports with no name, description, or licence — found by importing a
    // real library, not by reading the code.
    let markdown = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let (frontmatter, body, body_line) = split_frontmatter(markdown)?;
    let first_heading = body.lines().find_map(parse_heading).map(|(_, text)| text);
    let name = frontmatter
        .name
        .clone()
        .or(first_heading.map(str::to_owned))
        .unwrap_or_else(|| source_stem(source));
    let description = frontmatter.description.clone().unwrap_or_default();
    let provenance = provenance(source, 1, markdown);
    let mut root_config = HashMap::new();
    root_config.insert("role".to_owned(), Value::String("skill".to_owned()));
    root_config.insert("order".to_owned(), Value::from(0));

    let mut builder = Builder {
        source,
        nodes: vec![GraphNode {
            id: "skill".to_owned(),
            kind: NodeKind::Skill,
            label: name.clone(),
            description: description.clone(),
            position: Position { x: 0.0, y: 0.0 },
            execution: None,
            provenance: vec![provenance],
            config: root_config,
        }],
        edges: Vec::new(),
        headings: Vec::new(),
        steps: Vec::new(),
        dependency_hints: Vec::new(),
        order: 0,
        edge_count: 0,
    };
    builder.parse_body(body, body_line, &name);
    builder.add_dependencies();

    let mut metadata = HashMap::from([
        ("compiler".to_owned(), "cortex-skills".to_owned()),
        ("description".to_owned(), description),
        ("source".to_owned(), source.to_owned()),
    ]);
    for (key, value) in frontmatter.extra {
        metadata.insert(format!("frontmatter.{key}"), value);
    }
    let graph = GraphDocument {
        schema_version: GRAPH_SCHEMA_VERSION.to_owned(),
        id: format!("{}-skill", slug(&name)),
        name,
        revision: 0,
        nodes: builder.nodes,
        edges: builder.edges,
        metadata,
    };
    graph.validate()?;
    Ok(graph)
}

impl Builder<'_> {
    fn parse_body(&mut self, body: &str, first_line: usize, skill_name: &str) {
        let lines: Vec<&str> = body.lines().collect();
        let mut index = 0;
        let mut skipped_title = false;
        while index < lines.len() {
            let line = lines[index];
            let source_line = first_line + index;
            if line.trim().is_empty() {
                index += 1;
                continue;
            }
            if let Some((level, text)) = parse_heading(line) {
                if !skipped_title && level == 1 && text == crate::heading_text(skill_name) {
                    skipped_title = true;
                } else if !text.trim().is_empty() {
                    // An empty heading carries no workflow meaning. Skipping
                    // it keeps a stray `##` from failing the whole import on
                    // the empty-label invariant.
                    self.add_heading(level, text, source_line);
                }
                index += 1;
                continue;
            }
            if let Some(item) = parse_list_item(line) {
                // Likewise for a stray `- ` or `1. ` with no text: it is
                // valid Markdown and must not abort the document.
                if !item.text.trim().is_empty() {
                    self.add_step(item, source_line);
                }
                index += 1;
                continue;
            }
            if line.trim_start().starts_with("```") {
                let start = index;
                index += 1;
                while index < lines.len() {
                    let closed = lines[index].trim_start().starts_with("```");
                    index += 1;
                    if closed {
                        break;
                    }
                }
                self.add_guidance(&lines[start..index].join("\n"), source_line);
                continue;
            }
            let start = index;
            index += 1;
            while index < lines.len()
                && !lines[index].trim().is_empty()
                && parse_heading(lines[index]).is_none()
                && parse_list_item(lines[index]).is_none()
                && !lines[index].trim_start().starts_with("```")
            {
                index += 1;
            }
            self.add_guidance(&lines[start..index].join("\n"), source_line);
        }
    }

    fn add_heading(&mut self, level: usize, text: &str, line: usize) {
        while self.headings.last().is_some_and(|(seen, _)| *seen >= level) {
            self.headings.pop();
        }
        self.order += 1;
        let id = format!("section-{}", self.order);
        let mut config = semantic_config("section", self.order, line);
        config.insert("headingLevel".to_owned(), Value::from(level));
        self.add_node(&id, NodeKind::Skill, text, "", level, line, config);
        self.link_parent(&id);
        self.headings.push((level, id));
    }

    fn add_step(&mut self, item: ListItem<'_>, line: usize) {
        self.order += 1;
        let step_number = self.steps.len() + 1;
        let id = format!("step-{step_number}");
        let mut config = semantic_config("workflow_step", self.order, line);
        config.insert("indent".to_owned(), Value::from(item.indent));
        // A step is deterministic work unless the author says otherwise.
        // Inferring "this sentence sounds like an approval" from prose would
        // put a guess where the whole project puts an explicit decision, so
        // the kind is annotated or it is not there.
        let kind = kind_annotation(item.text).unwrap_or(NodeKind::Deterministic);
        if kind != NodeKind::Deterministic {
            config.insert(
                "declaredKind".to_owned(),
                Value::String(kind.as_str().to_owned()),
            );
        }
        match item.marker {
            ListMarker::Numbered(number) => {
                config.insert("marker".to_owned(), Value::String("numbered".to_owned()));
                config.insert("listNumber".to_owned(), Value::from(number));
            }
            ListMarker::Checklist(checked) => {
                config.insert("marker".to_owned(), Value::String("checklist".to_owned()));
                config.insert("checked".to_owned(), Value::Bool(checked));
            }
            ListMarker::Bullet => {
                config.insert("marker".to_owned(), Value::String("bullet".to_owned()));
            }
        }
        let depth = self.headings.last().map_or(1, |(level, _)| level + 1);
        // Annotations are structure, not prose: they belong in the graph, not
        // in a label a human has to read around.
        let label = strip_annotations(item.text);
        self.add_node(&id, kind, &label, "", depth, line, config);
        self.link_parent(&id);
        if let Some(previous) = self.steps.last().cloned() {
            self.add_edge(previous, id.clone(), EdgeKind::Sequence, "next step", None);
        }
        self.dependency_hints
            .push((id.clone(), dependency_numbers(item.text)));
        self.steps.push(id);
    }

    fn add_guidance(&mut self, text: &str, line: usize) {
        self.order += 1;
        let id = format!("guidance-{}", self.order);
        let config = semantic_config("guidance", self.order, line);
        let depth = self.headings.last().map_or(1, |(level, _)| level + 1);
        self.add_node(
            &id,
            NodeKind::Skill,
            &guidance_label(text),
            text,
            depth,
            line,
            config,
        );
        self.link_parent(&id);
    }

    #[allow(clippy::too_many_arguments)]
    fn add_node(
        &mut self,
        id: &str,
        kind: NodeKind,
        label: &str,
        description: &str,
        depth: usize,
        line: usize,
        config: HashMap<String, Value>,
    ) {
        self.nodes.push(GraphNode {
            id: id.to_owned(),
            kind,
            label: label.trim().to_owned(),
            description: description.trim().to_owned(),
            position: Position {
                x: Self::coordinate(depth, 220.0),
                y: Self::coordinate(self.order, 110.0),
            },
            execution: None,
            provenance: vec![provenance(self.source, line, label)],
            config,
        });
    }

    fn coordinate(index: usize, step: f64) -> f64 {
        f64::from(u32::try_from(index).unwrap_or(u32::MAX)) * step
    }

    fn link_parent(&mut self, child: &str) {
        let parent = self.headings.last().map_or("skill", |(_, id)| id.as_str());
        self.add_edge(
            parent.to_owned(),
            child.to_owned(),
            EdgeKind::Context,
            "contains",
            None,
        );
    }

    fn add_dependencies(&mut self) {
        let mut seen = HashSet::new();
        for (target, numbers) in self.dependency_hints.clone() {
            for number in numbers {
                let Some(source) = self.steps.get(number.saturating_sub(1)).cloned() else {
                    continue;
                };
                if source != target && seen.insert((source.clone(), target.clone())) {
                    self.add_edge(
                        source,
                        target.clone(),
                        EdgeKind::Conditional,
                        "explicit dependency",
                        Some(format!("depends on step {number}")),
                    );
                }
            }
        }
    }

    fn add_edge(
        &mut self,
        from: String,
        to: String,
        kind: EdgeKind,
        label: &str,
        condition: Option<String>,
    ) {
        self.edge_count += 1;
        self.edges.push(GraphEdge {
            id: format!("edge-{}", self.edge_count),
            from,
            to,
            kind,
            label: label.to_owned(),
            condition,
        });
    }
}

fn split_frontmatter(markdown: &str) -> Result<(Frontmatter, &str, usize), SkillError> {
    if markdown
        .lines()
        .next()
        .is_none_or(|line| line.trim() != "---")
    {
        return Ok((Frontmatter::default(), markdown, 1));
    }
    let mut frontmatter = Frontmatter::default();
    let mut line_number = 1;
    let mut closed = false;
    let mut consumed = 0;
    for segment in markdown.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\r', '\n']);
        consumed += segment.len();
        if line_number == 1 {
            line_number += 1;
            continue;
        }
        if line.trim() == "---" {
            closed = true;
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            line_number += 1;
            continue;
        }
        let (key, value) = trimmed.split_once(':').ok_or_else(|| {
            SkillError::InvalidFrontmatter(format!("expected key: value at line {line_number}"))
        })?;
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return Err(SkillError::InvalidFrontmatter(format!(
                "empty key at line {line_number}"
            )));
        }
        let value = unquote(value.trim());
        match key.as_str() {
            "name" => frontmatter.name = Some(value),
            "description" => frontmatter.description = Some(value),
            _ => {
                frontmatter.extra.insert(key, value);
            }
        }
        line_number += 1;
    }
    if !closed {
        return Err(SkillError::InvalidFrontmatter(
            "opening delimiter has no closing delimiter".to_owned(),
        ));
    }
    Ok((frontmatter, &markdown[consumed..], line_number + 1))
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&level) || trimmed.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    Some((level, trimmed[level + 1..].trim()))
}

fn parse_list_item(line: &str) -> Option<ListItem<'_>> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    for (prefix, checked) in [("- [ ] ", false), ("- [x] ", true), ("- [X] ", true)] {
        if let Some(text) = trimmed.strip_prefix(prefix) {
            return Some(ListItem {
                marker: ListMarker::Checklist(checked),
                text: text.trim(),
                indent,
            });
        }
    }
    for prefix in ["- ", "* ", "+ "] {
        if let Some(text) = trimmed.strip_prefix(prefix) {
            return Some(ListItem {
                marker: ListMarker::Bullet,
                text: text.trim(),
                indent,
            });
        }
    }
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let delimiter = trimmed.as_bytes().get(digits)?;
    if !matches!(delimiter, b'.' | b')') || trimmed.as_bytes().get(digits + 1) != Some(&b' ') {
        return None;
    }
    let number = trimmed[..digits].parse().ok()?;
    Some(ListItem {
        marker: ListMarker::Numbered(number),
        text: trimmed[digits + 2..].trim(),
        indent,
    })
}

/// A readable title for a guidance block.
///
/// The first line of a fenced block is the fence itself, so using it made
/// nodes on the canvas read `` ```text `` — three backticks and a language
/// tag, telling a reader nothing. The first line with content is what the
/// block is about.
fn guidance_label(text: &str) -> String {
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        return "Guidance".to_owned();
    }
    // Prose wraps across source lines, so the first *line* cuts a sentence in
    // half. The first sentence is what the block is about.
    let sentence = joined
        .find(". ")
        .map_or(joined.as_str(), |end| &joined[..=end]);
    let trimmed = sentence.trim();
    if trimmed.chars().count() <= MAX_GUIDANCE_LABEL_CHARS {
        return trimmed.to_owned();
    }
    trimmed
        .char_indices()
        .take(MAX_GUIDANCE_LABEL_CHARS)
        .filter(|(_, character)| character.is_whitespace())
        .last()
        .map_or_else(
            || trimmed.chars().take(MAX_GUIDANCE_LABEL_CHARS).collect(),
            |(at, _)| trimmed[..at].to_owned(),
        )
}

/// A guidance title longer than this stops being a title. The full text stays
/// in the node description either way.
const MAX_GUIDANCE_LABEL_CHARS: usize = 88;

/// The node kind an author declared with `[kind: review_gate]`.
pub(crate) fn kind_annotation(text: &str) -> Option<NodeKind> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("[kind:")?;
    let tail = &lower[start + 6..];
    let end = tail.find(']')?;
    NodeKind::parse(&tail[..end])
}

/// Remove every `[…]` annotation this compiler owns from a label.
///
/// Import and export both go through here, so a label is the same text in
/// the graph, on the canvas, and after a round trip.
pub(crate) fn strip_annotations(label: &str) -> String {
    let mut result = label.to_owned();
    loop {
        let lower = result.to_ascii_lowercase();
        let Some(start) = ["[depends:", "[kind:"]
            .iter()
            .filter_map(|marker| lower.find(marker))
            .min()
        else {
            break;
        };
        let Some(relative_end) = lower[start..].find(']') else {
            break;
        };
        result.replace_range(start..=start + relative_end, "");
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn dependency_numbers(text: &str) -> Vec<usize> {
    let lower = text.to_ascii_lowercase();
    let mut numbers = Vec::new();
    for prefix in ["depends on step ", "after step "] {
        let mut rest = lower.as_str();
        while let Some(position) = rest.find(prefix) {
            let tail = &rest[position + prefix.len()..];
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(number) = digits.parse() {
                numbers.push(number);
            }
            rest = tail;
        }
    }
    let mut rest = lower.as_str();
    while let Some(position) = rest.find("[depends:") {
        let tail = &rest[position + 9..];
        let Some(end) = tail.find(']') else { break };
        for value in tail[..end].split(',') {
            let digits: String = value
                .trim()
                .trim_start_matches("step")
                .trim()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(number) = digits.parse() {
                numbers.push(number);
            }
        }
        rest = &tail[end + 1..];
    }
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

fn semantic_config(role: &str, order: usize, line: usize) -> HashMap<String, Value> {
    HashMap::from([
        ("role".to_owned(), Value::String(role.to_owned())),
        ("order".to_owned(), Value::from(order)),
        ("sourceLine".to_owned(), Value::from(line)),
    ])
}

fn provenance(source: &str, line: usize, content: &str) -> Provenance {
    Provenance {
        source: source.to_owned(),
        locator: format!("line:{line}"),
        digest: Some(format!("fnv1a64:{:016x}", stable_hash(content))),
    }
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn slug(value: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "skill".to_owned()
    } else {
        slug
    }
}

fn source_stem(source: &str) -> String {
    source
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(source)
        .trim_end_matches(".md")
        .trim_end_matches(".MD")
        .replace(['-', '_'], " ")
}

/// Read one frontmatter scalar.
///
/// A double-quoted value is a JSON-compatible string literal — that is how
/// [`crate::export_skill_markdown`] writes it — so it is parsed back the same
/// way. Stripping the quotes without unescaping would leave the escapes in
/// the value and the next export would escape them again, so each round trip
/// would add a layer (`say "hi"` becoming `say \"hi\"`, then `say \\\"hi\\\"`).
/// Single-quoted values carry no backslash escapes and only lose their
/// delimiters.
fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if value.len() < 2 {
        return value.to_owned();
    }
    match (bytes[0], bytes[value.len() - 1]) {
        (b'"', b'"') => serde_json::from_str::<String>(value)
            .unwrap_or_else(|_| value[1..value.len() - 1].to_owned()),
        (b'\'', b'\'') => value[1..value.len() - 1].to_owned(),
        _ => value.to_owned(),
    }
}
