import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import type { GraphDocument } from '../types'

interface DocumentInspectorProps {
  graph: GraphDocument
  onUpdate: (document: GraphDocument) => void
}

export function DocumentInspector({ graph, onUpdate }: DocumentInspectorProps) {
  const [name, setName] = useState(graph.name)
  const [metadata, setMetadata] = useState(JSON.stringify(graph.metadata, null, 2))
  const [error, setError] = useState('')

  useEffect(() => {
    setName(graph.name)
    setMetadata(JSON.stringify(graph.metadata, null, 2))
    setError('')
  }, [graph.id, graph.revision])

  const apply = (event: FormEvent) => {
    event.preventDefault()
    try {
      if (!name.trim()) throw new Error('Graph name is required.')
      const parsed: unknown = JSON.parse(metadata)
      if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)
        || Object.values(parsed).some(value => typeof value !== 'string')) {
        throw new Error('Metadata must be a JSON object with string values.')
      }
      onUpdate({ ...graph, name: name.trim(), metadata: parsed as Record<string, string> })
      setError('')
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Unable to update graph.')
    }
  }

  return (
    <form className="inspector-form" onSubmit={apply}>
      <div className="inspector-heading">
        <div><p className="eyebrow">Graph document</p><h2>{graph.name}</h2></div>
      </div>
      <dl className="document-facts">
        <div><dt>ID</dt><dd>{graph.id}</dd></div>
        <div><dt>Schema</dt><dd>{graph.schemaVersion}</dd></div>
        <div><dt>Revision</dt><dd>{graph.revision}</dd></div>
        <div><dt>Shape</dt><dd>{graph.nodes.length} nodes · {graph.edges.length} edges</dd></div>
      </dl>
      <label className="field"><span>Name</span><input value={name} onChange={event => setName(event.target.value)} /></label>
      <label className="field"><span>Metadata</span>
        <textarea rows={9} spellCheck={false} value={metadata} onChange={event => setMetadata(event.target.value)} />
      </label>
      {error && <p className="form-error" role="alert">{error}</p>}
      <div className="inspector-actions"><button type="submit" className="primary-button">Apply changes</button></div>
      <p className="inspector-help">Select a node or edge on the canvas to edit it. Changes remain local until you save.</p>
    </form>
  )
}
