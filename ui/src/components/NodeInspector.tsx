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
  execution: ExecutionPolicy | null
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
    execution: node.execution ? { ...node.execution } : null,
    provenance: pretty(node.provenance),
    config: pretty(node.config),
  }
}

const targetFor = (kind: GraphNode['kind']): ExecutionPolicy['target'] => {
  if (kind === 'local_model') return 'ollama'
  if (kind === 'weavatrix') return 'weavatrix'
  if (kind === 'upstream_agent' || kind === 'agent_task') return 'upstream'
  if (kind === 'human_gate' || kind === 'review_gate') return 'human'
  return 'deterministic'
}

const defaultExecution = (kind: GraphNode['kind']): ExecutionPolicy => {
  const target = targetFor(kind)
  return {
    target,
    risk: 'low',
    maxInputTokens: 8192,
    maxOutputTokens: 1024,
    requireEvidence: ['weavatrix', 'ollama', 'upstream'].includes(target),
    requireUpstreamReview: target === 'ollama',
    allowMutation: false,
    modelProfile: target === 'ollama' ? 'local-medium' : null,
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
      const provenanceValue: unknown = JSON.parse(draft.provenance)
      const configValue: unknown = JSON.parse(draft.config)
      if (draft.execution && (draft.execution.maxInputTokens < 1 || draft.execution.maxOutputTokens < 1)) {
        throw new Error('Execution token budgets must be greater than zero.')
      }
      if (draft.execution?.allowMutation && !['upstream', 'human'].includes(draft.execution.target)) {
        throw new Error('Only upstream or human targets may receive mutation authority.')
      }
      if (draft.execution?.target === 'ollama' && !draft.execution.requireUpstreamReview) {
        throw new Error('Ollama output must require upstream review.')
      }
      if (!Array.isArray(provenanceValue)) throw new Error('Provenance must be a JSON array.')
      if (!objectValue(configValue)) throw new Error('Config must be a JSON object.')
      onUpdate(node.id, {
        ...node,
        id,
        kind: draft.kind,
        label: draft.label.trim(),
        description: draft.description,
        execution: draft.execution,
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
      <fieldset className="policy-section">
        <legend>Execution policy</legend>
        <label className="checkbox-field">
          <input
            type="checkbox"
            checked={draft.execution !== null}
            onChange={event => field('execution', event.target.checked ? defaultExecution(draft.kind) : null)}
          />
          <span>Executable process node</span>
        </label>
        {draft.execution && (
          <>
            <div className="form-grid two-columns">
              <label className="field"><span>Target</span>
                <select
                  value={draft.execution.target}
                  onChange={event => field('execution', {
                    ...draft.execution!,
                    target: event.target.value as ExecutionPolicy['target'],
                    requireUpstreamReview: event.target.value === 'ollama' || draft.execution!.requireUpstreamReview,
                    allowMutation: ['upstream', 'human'].includes(event.target.value) && draft.execution!.allowMutation,
                  })}
                >
                  {['deterministic', 'weavatrix', 'ollama', 'upstream', 'human'].map(target => (
                    <option key={target} value={target}>{target}</option>
                  ))}
                </select>
              </label>
              <label className="field"><span>Risk</span>
                <select
                  value={draft.execution.risk}
                  onChange={event => field('execution', { ...draft.execution!, risk: event.target.value as ExecutionPolicy['risk'] })}
                >
                  {['low', 'medium', 'high', 'critical'].map(risk => <option key={risk} value={risk}>{risk}</option>)}
                </select>
              </label>
              <label className="field"><span>Max input tokens</span>
                <input
                  type="number"
                  min={1}
                  value={draft.execution.maxInputTokens}
                  onChange={event => field('execution', { ...draft.execution!, maxInputTokens: Number(event.target.value) })}
                />
              </label>
              <label className="field"><span>Max output tokens</span>
                <input
                  type="number"
                  min={1}
                  value={draft.execution.maxOutputTokens}
                  onChange={event => field('execution', { ...draft.execution!, maxOutputTokens: Number(event.target.value) })}
                />
              </label>
            </div>
            <label className="field"><span>Model profile</span>
              <input
                value={draft.execution.modelProfile ?? ''}
                onChange={event => field('execution', { ...draft.execution!, modelProfile: event.target.value || null })}
                placeholder="local-small, local-medium, upstream-strong"
              />
            </label>
            {([
              ['requireEvidence', 'Require evidence IDs'],
              ['requireUpstreamReview', 'Require upstream review'],
              ['allowMutation', 'Allow mutation'],
            ] as const).map(([key, label]) => (
              <label className="checkbox-field" key={key}>
                <input
                  type="checkbox"
                  checked={draft.execution![key]}
                  disabled={(key === 'allowMutation' && !['upstream', 'human'].includes(draft.execution!.target))
                    || (key === 'requireUpstreamReview' && draft.execution!.target === 'ollama')}
                  onChange={event => field('execution', { ...draft.execution!, [key]: event.target.checked })}
                />
                <span>{label}</span>
              </label>
            ))}
          </>
        )}
      </fieldset>
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
