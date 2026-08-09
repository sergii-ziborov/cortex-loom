import { useEffect, useMemo, useState } from 'react'
import {
  copySequenceTemplate,
  lintSequence,
  listSequenceTemplates,
  loadSequenceTemplate,
} from '../api/client'
import { compareSequences, defaultSequenceIdentity, hasBlockingDiagnostics } from '../model/sequence'
import type {
  GraphDocument,
  GraphSummary,
  SequenceDiagnostic,
  SequenceTemplateDetail,
  SequenceTemplateSummary,
} from '../types'

interface SequenceStudioProps {
  graphs: GraphSummary[]
  current: GraphDocument
  onClose: () => void
  onOpen: (id: string) => void
  onUse: (id: string) => void
}

const messageOf = (error: unknown) => error instanceof Error ? error.message : 'Something went wrong.'

export function SequenceStudio({ graphs, current, onClose, onOpen, onUse }: SequenceStudioProps) {
  const [tab, setTab] = useState<'templates' | 'mine'>('templates')
  const [templates, setTemplates] = useState<SequenceTemplateSummary[]>([])
  const [selectedId, setSelectedId] = useState('')
  const [detail, setDetail] = useState<SequenceTemplateDetail | null>(null)
  const [graphId, setGraphId] = useState('')
  const [name, setName] = useState('')
  const [diagnostics, setDiagnostics] = useState<SequenceDiagnostic[] | null>(null)
  const [compareOpen, setCompareOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    const controller = new AbortController()
    listSequenceTemplates(controller.signal)
      .then(items => {
        setTemplates(items)
        setSelectedId(current.metadata['sequence.templateId'] ?? items[0]?.id ?? '')
      })
      .catch(caught => {
        if ((caught as { name?: string }).name !== 'AbortError') setError(messageOf(caught))
      })
    return () => controller.abort()
  }, [])

  useEffect(() => {
    if (!selectedId) return
    const controller = new AbortController()
    setDetail(null)
    setError('')
    loadSequenceTemplate(selectedId, controller.signal)
      .then(template => {
        setDetail(template)
        const identity = defaultSequenceIdentity(template.title)
        setGraphId(identity.graphId)
        setName(identity.name)
      })
      .catch(caught => {
        if ((caught as { name?: string }).name !== 'AbortError') setError(messageOf(caught))
      })
    return () => controller.abort()
  }, [selectedId])

  useEffect(() => {
    const handler = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose() }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [onClose])

  const mySequences = graphs.filter(graph => graph.templateId)
  const steps = useMemo(() => detail?.graph.nodes
    .filter(node => node.config.role === 'workflow_step')
    .sort((left, right) => Number(left.config.order ?? 0) - Number(right.config.order ?? 0)) ?? [], [detail])
  const comparable = detail?.id === current.metadata['sequence.templateId']
  const comparison = comparable && detail ? compareSequences(detail.graph, current) : null

  const useTemplate = async () => {
    if (!detail || !graphId.trim() || !name.trim()) return
    setBusy(true)
    setError('')
    try {
      const result = await copySequenceTemplate(detail.id, graphId.trim(), name.trim())
      onUse(result.graph.id)
    } catch (caught) {
      setError(messageOf(caught))
    } finally {
      setBusy(false)
    }
  }

  const testCurrent = async () => {
    setBusy(true)
    setError('')
    try {
      setDiagnostics(await lintSequence(current))
    } catch (caught) {
      setError(messageOf(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="dialog-backdrop" role="dialog" aria-modal="true" aria-label="Sequence studio">
      <div className="dialog sequence-studio">
        <header className="dialog-header sequence-header">
          <div>
            <span className="eyebrow">Editable methodology</span>
            <h2>Sequences</h2>
            <p>Start from a safe Cortex template, then make the copy yours.</p>
          </div>
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close">×</button>
        </header>

        <div className="sequence-tabs" role="tablist">
          <button type="button" role="tab" aria-selected={tab === 'templates'} onClick={() => setTab('templates')}>
            Templates <span>7</span>
          </button>
          <button type="button" role="tab" aria-selected={tab === 'mine'} onClick={() => setTab('mine')}>
            My sequences <span>{mySequences.length}</span>
          </button>
        </div>

        {error && <p className="sequence-message error" role="alert">{error}</p>}

        {tab === 'templates' ? (
          <div className="sequence-layout">
            <nav className="sequence-template-list" aria-label="Sequence templates">
              {templates.map(template => (
                <button
                  key={template.id}
                  type="button"
                  className={selectedId === template.id ? 'selected' : ''}
                  onClick={() => setSelectedId(template.id)}
                >
                  <span className="sequence-template-icon" aria-hidden="true">{template.activation.mutation ? '↗' : '◎'}</span>
                  <span><strong>{template.title}</strong><small>{template.description}</small></span>
                </button>
              ))}
            </nav>

            <section className="sequence-preview" aria-live="polite">
              {!detail ? <div className="sequence-loading">Loading template…</div> : (
                <>
                  <div className="sequence-preview-heading">
                    <div>
                      <span className="version-chip">v{detail.version.major}.{detail.version.minor}.{detail.version.patch}</span>
                      <h3>{detail.title}</h3>
                      <p>{detail.description}</p>
                    </div>
                    <span className={detail.activation.mutation ? 'risk-chip mutation' : 'risk-chip'}>
                      {detail.activation.mutation ? 'Can change files' : 'Read-only by default'}
                    </span>
                  </div>
                  <div className="sequence-cues">
                    {detail.activation.intents.slice(0, 4).map(intent => <span key={intent}>{intent}</span>)}
                  </div>
                  <ol className="sequence-steps">
                    {steps.map(step => (
                      <li key={step.id}>
                        <span className={`kind-dot kind-${step.kind}`} />
                        <div><strong>{step.label}</strong><small>{step.kind.replaceAll('_', ' ')}</small></div>
                      </li>
                    ))}
                  </ol>

                  <div className="sequence-copy-card">
                    <div><strong>Use and edit</strong><small>The template stays unchanged. Your copy is fully editable.</small></div>
                    <label>Name<input value={name} onChange={event => setName(event.target.value)} maxLength={512} /></label>
                    <label>ID<input value={graphId} onChange={event => setGraphId(event.target.value)} maxLength={256} /></label>
                    <button className="primary-button" type="button" disabled={busy || !name.trim() || !graphId.trim()} onClick={() => void useTemplate()}>
                      {busy ? 'Creating…' : 'Use and edit'}
                    </button>
                  </div>

                  {comparable && (
                    <div className="sequence-compare">
                      <button type="button" onClick={() => setCompareOpen(open => !open)}>
                        {compareOpen ? 'Hide comparison' : 'Compare with my open copy'}
                      </button>
                      {compareOpen && comparison && (
                        <div className="sequence-diff">
                          <strong>No automatic merge</strong>
                          <span>Added steps: {comparison.addedNodes.length}</span>
                          <span>Changed steps: {comparison.changedNodes.length}</span>
                          <span>Removed steps: {comparison.removedNodes.length}</span>
                          <span>Edge changes: +{comparison.addedEdges} / −{comparison.removedEdges}</span>
                        </div>
                      )}
                    </div>
                  )}

                  <details className="sequence-advanced">
                    <summary>Advanced · raw JSON</summary>
                    <pre>{JSON.stringify(detail.graph, null, 2)}</pre>
                  </details>
                </>
              )}
            </section>
          </div>
        ) : (
          <section className="my-sequences">
            <div className="my-sequences-toolbar">
              <div><h3>Your editable sequences</h3><p>Drafts can be saved even when a safety test still has findings.</p></div>
              {current.metadata['sequence.templateId'] && (
                <button type="button" disabled={busy} onClick={() => void testCurrent()}>Test open sequence</button>
              )}
            </div>
            {diagnostics && (
              <div className={hasBlockingDiagnostics(diagnostics) ? 'sequence-test blocked' : 'sequence-test passed'}>
                <strong>{hasBlockingDiagnostics(diagnostics) ? 'Run blocked' : 'Ready to run'}</strong>
                <span>{diagnostics.length === 0 ? 'No safety findings.' : `${diagnostics.length} finding(s).`}</span>
                {diagnostics.map(item => <small key={`${item.code}-${item.nodeId}`}>{item.message}</small>)}
              </div>
            )}
            <div className="my-sequence-grid">
              {mySequences.map(graph => (
                <article key={graph.id} className={graph.id === current.id ? 'current' : ''}>
                  <span className="eyebrow">{graph.templateId}</span>
                  <h4>{graph.name}</h4>
                  <p>{graph.nodeCount} steps and supporting nodes · revision {graph.revision}</p>
                  <button type="button" onClick={() => onOpen(graph.id)}>{graph.id === current.id ? 'Open now' : 'Open and edit'}</button>
                </article>
              ))}
              {mySequences.length === 0 && (
                <div className="sequence-empty"><strong>No copies yet</strong><span>Choose a template to create your first editable sequence.</span><button type="button" onClick={() => setTab('templates')}>Browse templates</button></div>
              )}
            </div>
          </section>
        )}
      </div>
    </div>
  )
}
