import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { EDGE_KINDS } from '../types'
import type { GraphDocument, GraphEdge } from '../types'

interface EdgeInspectorProps {
  edge: GraphEdge
  graph: GraphDocument
  onDelete: () => void
  onUpdate: (previousId: string, edge: GraphEdge) => void
}

export function EdgeInspector({ edge, graph, onDelete, onUpdate }: EdgeInspectorProps) {
  const [draft, setDraft] = useState(edge)
  const [error, setError] = useState('')

  useEffect(() => {
    setDraft(edge)
    setError('')
  }, [edge.id])

  const apply = (event: FormEvent) => {
    event.preventDefault()
    const id = draft.id.trim()
    if (!id) return setError('Edge ID is required.')
    if (draft.from === draft.to) return setError('A node cannot connect to itself.')
    if (graph.edges.some(item => item.id === id && item.id !== edge.id)) return setError(`Edge ID “${id}” already exists.`)
    setError('')
    onUpdate(edge.id, { ...draft, id, label: draft.label.trim(), condition: draft.condition?.trim() || null })
  }

  return (
    <form className="inspector-form" onSubmit={apply}>
      <div className="inspector-heading">
        <div><p className="eyebrow">Connection</p><h2>{edge.label || edge.id}</h2></div>
        <span className={`kind-badge edge-${edge.kind}`}>{edge.kind}</span>
      </div>
      <label className="field"><span>ID</span><input value={draft.id} onChange={event => setDraft(current => ({ ...current, id: event.target.value }))} /></label>
      <div className="form-grid two-columns">
        <label className="field"><span>From</span>
          <select value={draft.from} onChange={event => setDraft(current => ({ ...current, from: event.target.value }))}>
            {graph.nodes.map(node => <option key={node.id} value={node.id}>{node.label}</option>)}
          </select>
        </label>
        <label className="field"><span>To</span>
          <select value={draft.to} onChange={event => setDraft(current => ({ ...current, to: event.target.value }))}>
            {graph.nodes.map(node => <option key={node.id} value={node.id}>{node.label}</option>)}
          </select>
        </label>
      </div>
      <label className="field"><span>Kind</span>
        <select value={draft.kind} onChange={event => setDraft(current => ({ ...current, kind: event.target.value as GraphEdge['kind'] }))}>
          {EDGE_KINDS.map(kind => <option key={kind} value={kind}>{kind}</option>)}
        </select>
      </label>
      <label className="field"><span>Label</span><input value={draft.label} onChange={event => setDraft(current => ({ ...current, label: event.target.value }))} /></label>
      <label className="field"><span>Condition</span>
        <textarea rows={5} value={draft.condition ?? ''} onChange={event => setDraft(current => ({ ...current, condition: event.target.value }))} placeholder="Optional routing condition" />
      </label>
      {error && <p className="form-error" role="alert">{error}</p>}
      <div className="inspector-actions">
        <button type="submit" className="primary-button">Apply changes</button>
        <button type="button" className="danger-button" onClick={() => {
          if (window.confirm(`Delete edge “${edge.label || edge.id}”?`)) onDelete()
        }}>Delete edge</button>
      </div>
    </form>
  )
}
