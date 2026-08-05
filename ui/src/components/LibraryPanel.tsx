import { useEffect, useMemo, useState } from 'react'
import type { GraphSummary } from '../types'

interface LibraryPanelProps {
  graphs: GraphSummary[]
  currentId: string
  onOpen: (id: string) => void
  onClose: () => void
  onImportLibrary: () => void
}

/** Kinds that stop a run until something is decided. */
const GATE_KINDS = new Set(['quality_gate', 'human_gate', 'test_gate', 'review_gate', 'evidence_gate'])

/**
 * Which shelf a workflow belongs on.
 *
 * The server states the provenance from metadata it wrote itself; sniffing a
 * path here got it wrong, and independent filters put eleven workflows on
 * twenty cards. Exactly one bucket per graph.
 */
const GROUP_TITLES: Record<GraphSummary['originKind'], string> = {
  bundled: 'Methodology',
  imported: 'Imported',
  local: 'This workspace',
}

/**
 * Browse the workflows this instance holds.
 *
 * A dropdown of names tells you a graph exists and nothing else — not what it
 * is for, not whether it has gates, not where it came from. Choosing a
 * methodology is a decision, so it gets a surface where the decision can be
 * made: description, shape, provenance, and what the workflow is built out of.
 */
export function LibraryPanel({ graphs, currentId, onOpen, onClose, onImportLibrary }: LibraryPanelProps) {
  const [query, setQuery] = useState('')

  useEffect(() => {
    const handler = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose() }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [onClose])

  const groups = useMemo(() => {
    const needle = query.trim().toLowerCase()
    const matched = graphs.filter(graph => !needle
      || graph.name.toLowerCase().includes(needle)
      || graph.description.toLowerCase().includes(needle)
      || graph.kinds.some(kind => kind.includes(needle)))
    // One pass, one bucket per graph. Independent filters overlapped and
    // showed eleven workflows as twenty cards.
    const order: GraphSummary['originKind'][] = ['bundled', 'imported', 'local']
    const buckets = new Map<string, GraphSummary[]>(order.map(kind => [kind, []]))
    for (const graph of matched) buckets.get(graph.originKind)?.push(graph)
    return order
      .map(kind => ({ title: GROUP_TITLES[kind], items: buckets.get(kind) ?? [] }))
      .filter(group => group.items.length > 0)
  }, [graphs, query])

  return (
    <div className="dialog-backdrop" role="dialog" aria-modal="true" aria-label="Workflow library">
      <div className="dialog library-browser">
        <header className="dialog-header">
          <h2>Library</h2>
          <div className="header-spacer" />
          <button className="tool-button" type="button" onClick={onImportLibrary}>Import a checkout…</button>
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close">×</button>
        </header>

        <div className="library-browser-body">
          <label className="library-search">
            <span className="sr-only">Search workflows</span>
            <input
              type="search"
              value={query}
              onChange={event => setQuery(event.target.value)}
              placeholder="Search by name, purpose, or node kind…"
              autoFocus
            />
          </label>

          {groups.length === 0 && <p className="library-empty">Nothing matches “{query}”.</p>}

          {groups.map(group => (
            <section key={group.title} className="library-group">
              <h3>{group.title} <span>{group.items.length}</span></h3>
              <div className="library-grid">
                {group.items.map(graph => {
                  const gates = graph.kinds.filter(kind => GATE_KINDS.has(kind)).length
                  return (
                    <button
                      key={graph.id}
                      type="button"
                      className={graph.id === currentId ? 'library-card current' : 'library-card'}
                      onClick={() => onOpen(graph.id)}
                      aria-current={graph.id === currentId}
                    >
                      <span className="library-card-name">{graph.name}</span>
                      <span className="library-card-description">
                        {graph.description || 'No description recorded.'}
                      </span>
                      <span className="library-card-kinds">
                        {graph.kinds.map(kind => (
                          <span key={kind} className={`kind-badge kind-${kind}`}>{kind.replaceAll('_', ' ')}</span>
                        ))}
                      </span>
                      <span className="library-card-meta">
                        {graph.nodeCount} nodes · {graph.edgeCount} edges ·{' '}
                        {gates > 0 ? `${gates} gate kind${gates > 1 ? 's' : ''}` : 'no gates'} ·{' '}
                        {graph.revision === 0 ? 'unsaved' : `rev ${graph.revision}`}
                      </span>
                    </button>
                  )
                })}
              </div>
            </section>
          ))}
        </div>
      </div>
    </div>
  )
}
