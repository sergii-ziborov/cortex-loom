/**
 * Reference content for the Help panel.
 *
 * Kept as data rather than markup so it stays close to the typed vocabulary in
 * `types.ts` and `cortex-domain`. When a node kind, edge kind, or run state is
 * added there, it belongs here too — an editor that offers a vocabulary it
 * cannot explain is worse than one that offers less.
 */

export interface HelpSection {
  heading: string
  note?: string
  entries: [string, string][]
}

export interface HelpTopic {
  id: string
  title: string
  lede: string
  sections: HelpSection[]
}

export const HELP_TOPICS: HelpTopic[] = [
  {
    id: 'start',
    title: 'Getting started',
    lede: 'The graph is canonical and lives on the server. The editor is a client: '
      + 'nothing is persisted until you save, and a run is created from a saved revision.',
    sections: [
      {
        heading: 'First five minutes',
        entries: [
          ['1. Pick a graph', 'The selector lists every stored workflow, including the methodology library seeded on first run.'],
          ['2. Edit', 'Add nodes, connect them, and fill in the inspector on the right. Changes stay local until saved.'],
          ['3. Save', 'The save carries the revision you loaded. If someone else saved first you get a conflict, never a silent overwrite.'],
          ['4. Create a run', 'A run pins the exact graph revision and snapshot. Later graph edits never rewrite an active run.'],
          ['5. Drive the run', 'Start a node, submit evidence, then complete it by choosing the outgoing edges that were actually taken.'],
        ],
      },
      {
        heading: 'Toolbar',
        entries: [
          ['+ Node', 'Adds a node at the centre of the canvas and selects it.'],
          ['Connect', 'Click a source node, then a target node. Click the same node twice to restart; press the button again to cancel.'],
          ['Auto-layout', 'Recomputes positions from the edge structure. It changes only geometry.'],
          ['Import / Export Markdown', 'Round trip through the SKILL.md compiler. An imported graph starts unsaved at revision zero.'],
          ['Library', 'Every workflow with its purpose, shape, gate count, and provenance. Search by name, purpose, or node kind. Import a checkout from here.'],
          ['Import a checkout', 'Compiles every SKILL.md under a local path. Preview first: it shows what would be imported and any licence sitting beside it. Import never overwrites an existing graph.'],
          ['Reload', 'Discards local edits and re-reads the server graph, after a confirmation.'],
          ['Model interaction', 'Read-only telemetry: routing, evidence budgets, and shadow comparisons.'],
        ],
      },
      {
        heading: 'Keyboard and pointer',
        entries: [
          ['Drag a node', 'Moves it. Positions are part of the document, so moving a node makes the graph dirty.'],
          ['Enter or Space', 'Activates the focused node — the same as clicking it, including while linking.'],
          ['Arrow keys', 'Nudge the focused node by 8 px, or 24 px with Shift held.'],
          ['Escape', 'Closes this panel and the import dialog.'],
          ['Inspector edge', 'Focus the divider and use Arrow keys, Home, or End to resize the inspector.'],
        ],
      },
    ],
  },
  {
    id: 'nodes',
    title: 'Node kinds',
    lede: 'A node kind states who is allowed to do the work and what has to be true before the run moves past it.',
    sections: [
      {
        heading: 'Work',
        entries: [
          ['input', 'Entry point that receives the request or task.'],
          ['deterministic', 'Parsers and repository tooling. No model runs, so the result is reproducible.'],
          ['weavatrix', 'Repository graph, impact, and architecture evidence.'],
          ['skill', 'A reusable methodology workflow, typically compiled from SKILL.md.'],
          ['agent_task', 'A unit of work handed to an agent.'],
          ['local_model', 'A bounded local-model step. Its output is advisory and must be reviewed.'],
          ['upstream_agent', 'Work reserved for the strong upstream agent.'],
          ['output', 'Exit point carrying the verified result.'],
        ],
      },
      {
        heading: 'Gates',
        note: 'A gate refuses generic completion. Human and review gates need an explicit '
          + 'approved or rejected decision with an actor, a reason, and evidence from the same attempt.',
        entries: [
          ['quality_gate', 'Checks structure, provenance, risk, and budgets.'],
          ['human_gate', 'Requires an explicit human decision.'],
          ['test_gate', 'Gates on a test run.'],
          ['review_gate', 'Requires an explicit review decision.'],
          ['evidence_gate', 'Requires cited evidence before proceeding.'],
        ],
      },
      {
        heading: 'Control',
        entries: [
          ['branch', 'Selects exactly one outgoing conditional edge, always by explicit edge id.'],
          ['retry', 'Reopens a failed target for a bounded number of attempts. Configure targetNodeId and maxAttempts.'],
          ['handoff', 'Transfers responsibility to another executor.'],
          ['terminal', 'Ends its path.'],
        ],
      },
    ],
  },
  {
    id: 'edges',
    title: 'Edge kinds',
    lede: 'Edges decide where a run goes next. Success traverses sequence, context, tool, success, '
      + 'approval and requires edges; failure traverses failure, fallback, reject and escalation edges.',
    sections: [
      {
        heading: 'Flow',
        entries: [
          ['sequence', 'Plain ordering: the target follows the source.'],
          ['context', 'The source supplies context to the target.'],
          ['tool', 'The source invokes the target as a tool.'],
          ['requires', 'The target requires the source to have completed.'],
        ],
      },
      {
        heading: 'Outcome',
        entries: [
          ['success', 'Taken when the source succeeds.'],
          ['failure', 'Taken when the source fails.'],
          ['fallback', 'Recovery path taken when the source fails.'],
          ['approval', 'Taken when a gate approves.'],
          ['reject', 'Taken when a gate rejects.'],
          ['escalates', 'The source escalates to the target.'],
        ],
      },
      {
        heading: 'Explicit only',
        note: 'A conditional branch is never inferred from a free-form expression. '
          + 'The run command must name the edge id.',
        entries: [
          ['conditional', 'Taken only when explicitly selected by id.'],
          ['blocks', 'The source prevents the target from proceeding.'],
          ['invalidates', "The source invalidates the target's result."],
          ['supersedes', 'The source replaces the target.'],
        ],
      },
    ],
  },
  {
    id: 'runs',
    title: 'Runs and evidence',
    lede: 'A run is an append-only record. Every command carries the revision it expected and '
      + 'appends one durable event, so the whole run can be replayed and compared.',
    sections: [
      {
        heading: 'Node state',
        entries: [
          ['pending', 'Not reachable yet: an incoming edge is still unresolved.'],
          ['ready', 'Every incoming executable edge is resolved; the node can start.'],
          ['running', 'Started and not yet completed.'],
          ['succeeded / failed', 'Completed with an outcome and the edges that were taken.'],
          ['skipped', 'On a branch that was not taken; it propagates not_taken onward.'],
          ['cancelled', 'The run was cancelled before this node resolved.'],
        ],
      },
      {
        heading: 'Edge state',
        entries: [
          ['dormant', 'The source has not resolved.'],
          ['pending', 'Available for selection at completion.'],
          ['traversed', 'Selected, and the target advanced because of it.'],
          ['not_taken', 'Explicitly not selected. It never becomes traversed later.'],
        ],
      },
      {
        heading: 'Rules that will reject a command',
        note: 'These are deliberate. A refusal here is the product working, not a bug.',
        entries: [
          ['Stale revision', 'A command must carry the current run revision, or it is a conflict.'],
          ['Evidence from another attempt', 'Evidence is immutable and scoped to one node attempt. A retry preserves it for audit but cannot cite it.'],
          ['Invalidated evidence', 'An invalidated id can no longer be cited. The record is never deleted, and decisions made before invalidation are never rewritten.'],
          ['Gate without a decision', 'Human and review gates need approved or rejected with an actor, a reason, and same-attempt evidence.'],
          ['Branch without an edge id', 'A conditional transition requires the explicit edge id.'],
          ['Retry past the limit', 'When the target reaches maxAttempts the retry edge becomes not_taken and the run resolves.'],
          ['Held lease', 'A lease gives one typed identity exclusive execution. Expiry is evaluated against the command timestamp, so replay stays deterministic.'],
        ],
      },
    ],
  },
  {
    id: 'evidence',
    title: 'Evidence and budgets',
    lede: 'Evidence selection is deterministic and fail-closed. The compiler orders by trust and '
      + 'priority, never by a model deciding what looks important.',
    sections: [
      {
        heading: 'Priority',
        entries: [
          ['critical', 'Never dropped. A budget too small for critical evidence is an error, not a truncation.'],
          ['high / normal / low', 'Dropped from the bottom when the budget binds, and every omission is reported.'],
          ['contradictory', 'Sorted first regardless of priority, and it forces upstream review.'],
          ['unverified', 'Marks the whole packet as requiring upstream review.'],
        ],
      },
      {
        heading: 'Reading the numbers honestly',
        note: 'See the Context benchmark document for the measured comparison against '
          + 'Weavatrix alone and against reading the files.',
        entries: [
          ['Omitted tokens', 'Evidence that was assembled and then dropped to fit the budget. It is an omission volume, not a saving.'],
          ['A saving', 'Requires a measured baseline of what the alternative actually cost.'],
          ['Quality-equivalent', 'Savings are credited only on clean succeeded runs.'],
          ['Upstream consumption', 'Self-reported by the executor. It is honest reporting, not billing verification.'],
        ],
      },
    ],
  },
]
