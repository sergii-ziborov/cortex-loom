import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { NODE_KINDS } from '../types'
import type { ExecutionPolicy, GraphDocument, GraphNode, JsonValue, Provenance } from '../types'

interface NodeInspectorProps {
  graph: GraphDocument
  node: GraphNode
  onDelete: () => void
  onUpdate: (previousId: string, node: GraphNode) => void
}

interface NodeDraft {
  id: string
  kind: GraphNode['kind']
  label: string
  description: string
  execution: string
  provenance: string
  config: string
}

const pretty = (value: unknown) => JSON.stringify(value, null, 2)
const objectValue = (value: unknown): value is Record<string, JsonValue> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

function draftOf(node: GraphNode): NodeDraft {
  return {
    id: node.id,
    kind: node.kind,
    label: node.label,
    description: node.description,
    execution: pretty(node.execution ?? null),
    provenance: pretty(node.provenance),
    config: pretty(node.config),
  }
}

export function NodeInspector({ graph, node, onDelete, onUpdate }: NodeInspectorProps) {
  const [draft, setDraft] = useState<NodeDraft>(() => draftOf(node))
  const [error, setError] = useState('')

  useEffect(() => {
    setDraft(draftOf(node))
    setError('')
  }, [node.id])

  const field = <Key extends keyof NodeDraft>(key: Key, value: NodeDraft[Key]) =>
    setDraft(current => ({ ...current, [key]: value }))

  const apply = (event: FormEvent) => {
    event.preventDefault()
    setError('')
    try {
      const id = draft.id.trim()
      if (!id || !draft.label.trim()) throw new Error('Node ID and label are required.')
      if (graph.nodes.some(item => item.id === id && item.id !== node.id)) throw new Error(`Node ID “${id}” already exists.`)
      const executionValue: unknown = JSON.parse(draft.execution)
      const provenanceValue: unknown = JSON.parse(draft.provenance)
      const configValue: unknown = JSON.parse(draft.config)
      if (executionValue !== null && (typeof executionValue !== 'object' || Array.isArray(executionValue))) {
        throw new Error('Execution must be an object or null.')
      }
      if (!Array.isArray(provenanceValue)) throw new Error('Provenance must be a JSON array.')
      if (!objectValue(configValue)) throw new Error('Config must be a JSON object.')
      onUpdate(node.id, {
        ...node,
        id,
        kind: draft.kind,
        label: draft.label.trim(),
        description: draft.description,
        execution: executionValue as ExecutionPolicy | null,
        provenance: provenanceValue as Provenance[],
        config: configValue,
      })
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Unable to update node.')
    }
  }

  return (
    <form className="inspector-form" onSubmit={apply}>
      <div className="inspector-heading">
        <div><p className="eyebrow">Node</p><h2>{node.label}</h2></div>
        <span className={`kind-badge kind-${node.kind}`}>{node.kind.replaceAll('_', ' ')}</span>
      </div>
      <div className="form-grid two-columns">
        <label className="field"><span>ID</span><input value={draft.id} onChange={event => field('id', event.target.value)} /></label>
        <label className="field"><span>Kind</span>
          <select value={draft.kind} onChange={event => field('kind', event.target.value as GraphNode['kind'])}>
            {NODE_KINDS.map(kind => <option key={kind} value={kind}>{kind.replaceAll('_', ' ')}</option>)}
          </select>
        </label>
      </div>
      <label className="field"><span>Label</span><input value={draft.label} onChange={event => field('label', event.target.value)} /></label>
      <label className="field"><span>Description</span><textarea rows={3} value={draft.description} onChange={event => field('description', event.target.value)} /></label>
      <details className="json-section">
        <summary>Execution policy</summary>
        <textarea rows={9} spellCheck={false} value={draft.execution} onChange={event => field('execution', event.target.value)} />
      </details>
      <details className="json-section">
        <summary>Provenance</summary>
        <textarea rows={7} spellCheck={false} value={draft.provenance} onChange={event => field('provenance', event.target.value)} />
      </details>
      <details className="json-section">
        <summary>Config</summary>
        <textarea rows={7} spellCheck={false} value={draft.config} onChange={event => field('config', event.target.value)} />
      </details>
      {error && <p className="form-error" role="alert">{error}</p>}
      <div className="inspector-actions">
        <button type="submit" className="primary-button">Apply changes</button>
        <button type="button" className="danger-button" onClick={() => {
          if (window.confirm(`Delete node “${node.label}” and its edges?`)) onDelete()
        }}>Delete node</button>
      </div>
    </form>
  )
}
