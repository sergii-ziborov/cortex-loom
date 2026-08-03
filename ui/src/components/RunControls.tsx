import { useEffect, useMemo, useState } from 'react'
import type {
  GraphDocument,
  HumanDecision,
  NodeOutcome,
  ReplayVerification,
  RunDocument,
  RunSummary,
} from '../types'

interface RunControlsProps {
  graph: GraphDocument
  runGraph: GraphDocument | null
  run: RunDocument | null
  runs: RunSummary[]
  replay: ReplayVerification | null
  busy: boolean
  dirty: boolean
  error: string
  onCreate: () => void
  onSelect: (id: string) => void
  onStartNode: (id: string) => void
  onSubmitEvidence: (
    nodeId: string,
    submittedBy: string,
    source: string,
    locator: string,
    summary: string,
    digest?: string,
  ) => void
  onCompleteNode: (
    id: string,
    outcome: NodeOutcome,
    selectedEdges: string[],
    evidenceIds: string[],
    detail?: string,
  ) => void
  onDecideHumanGate: (
    id: string,
    decision: HumanDecision,
    actor: string,
    reason: string,
    selectedEdges: string[],
    evidenceIds: string[],
  ) => void
  onTriggerRetry: (id: string, reason: string) => void
  onVerifyReplay: () => void
  onCancel: () => void
}

export function RunControls(props: RunControlsProps) {
  const { graph, run } = props
  const snapshot = props.runGraph ?? graph
  const definition = (id: string) => snapshot.nodes.find(node => node.id === id)
  const readyNodes = run?.nodes.filter(node => node.status === 'ready') ?? []
  const startableNodes = readyNodes.filter(node => definition(node.nodeId)?.kind !== 'retry')
  const retryNodes = readyNodes.filter(node => definition(node.nodeId)?.kind === 'retry')
  const runningNodes = run?.nodes.filter(node => node.status === 'running') ?? []
  const [readyId, setReadyId] = useState('')
  const [retryId, setRetryId] = useState('')
  const [runningId, setRunningId] = useState('')
  const [edgeId, setEdgeId] = useState('')
  const [selectedEvidence, setSelectedEvidence] = useState<string[]>([])
  const [operator, setOperator] = useState('')
  const [source, setSource] = useState('manual')
  const [locator, setLocator] = useState('')
  const [digest, setDigest] = useState('')
  const [evidenceSummary, setEvidenceSummary] = useState('')
  const [detail, setDetail] = useState('')
  const [decisionReason, setDecisionReason] = useState('')
  const [retryReason, setRetryReason] = useState('')

  useEffect(() => {
    setReadyId(startableNodes[0]?.nodeId ?? '')
    setRetryId(retryNodes[0]?.nodeId ?? '')
  }, [run?.id, run?.revision])

  useEffect(() => {
    setRunningId(runningNodes[0]?.nodeId ?? '')
    setSelectedEvidence([])
    setLocator('')
    setDigest('')
    setEvidenceSummary('')
    setDetail('')
    setDecisionReason('')
  }, [run?.id, run?.revision])

  const runningState = runningNodes.find(node => node.nodeId === runningId)
  const runningDefinition = definition(runningId)
  const isHumanGate = runningDefinition?.kind === 'human_gate' || runningDefinition?.kind === 'review_gate'
  const currentEvidence = run?.evidence.filter(item =>
    item.nodeId === runningId && item.attempt === runningState?.attempt,
  ) ?? []
  const conditionalEdges = useMemo(
    () => snapshot.edges.filter(edge => edge.from === runningId && edge.kind === 'conditional'),
    [snapshot.edges, runningId],
  )
  const conditionalEdgeKey = conditionalEdges.map(edge => edge.id).join('\u0000')

  useEffect(
    () => setEdgeId(conditionalEdges.length === 1 ? conditionalEdges[0].id : ''),
    [conditionalEdgeKey, run?.id, runningId],
  )
  useEffect(
    () => setSelectedEvidence(currentEvidence.map(item => item.id)),
    [run?.revision, runningId],
  )

  const label = (id: string) => definition(id)?.label ?? id
  const selectedEdges = edgeId ? [edgeId] : []
  const active = run?.status === 'running'
  const canSubmitEvidence = Boolean(
    runningId && operator.trim() && source.trim() && locator.trim() && evidenceSummary.trim(),
  )

  const complete = (outcome: NodeOutcome) => {
    if (!runningId) return
    props.onCompleteNode(
      runningId,
      outcome,
      selectedEdges,
      selectedEvidence,
      detail.trim() || undefined,
    )
  }

  const decide = (decision: HumanDecision) => {
    if (!runningId) return
    props.onDecideHumanGate(
      runningId,
      decision,
      operator.trim(),
      decisionReason.trim(),
      selectedEdges,
      selectedEvidence,
    )
  }

  return (
    <section className="run-controls" aria-label="Run controls" aria-busy={props.busy}>
      <div className="run-primary">
        <strong>Run state</strong>
        <select
          value={run?.id ?? ''}
          onChange={event => props.onSelect(event.target.value)}
          disabled={props.busy || props.runs.length === 0}
          aria-label="Executable run"
        >
          {props.runs.length === 0 && <option value="">No runs</option>}
          {props.runs.map(item => (
            <option key={item.id} value={item.id}>{item.status} · r{item.revision} · {item.id}</option>
          ))}
        </select>
        <button
          type="button"
          className="primary-button"
          disabled={props.busy || props.dirty}
          title={props.dirty ? 'Save the graph before starting a run.' : ''}
          onClick={props.onCreate}
        >
          New run
        </button>
        {run && <span className={`run-chip ${run.status}`}>{run.status} · r{run.revision}</span>}
        {run && run.graphRevision !== graph.revision && (
          <span className="run-warning">snapshot graph r{run.graphRevision}</span>
        )}
        {run && (
          <button type="button" disabled={props.busy} onClick={props.onVerifyReplay}>
            Verify replay
          </button>
        )}
        {props.replay && (
          <span
            className={`replay-verdict ${props.replay.matchesPersisted ? 'verified' : 'mismatch'}`}
            role="status"
          >
            {props.replay.matchesPersisted
              ? `Replay verified · ${props.replay.eventCount} events`
              : `Replay mismatch · r${props.replay.replayedRevision}/${props.replay.persistedRevision}`}
          </span>
        )}
      </div>

      {active && (
        <div className="run-workbench">
          {(startableNodes.length > 0 || retryNodes.length > 0) && (
            <div className="run-action-group">
              <span className="run-action-label">Queue</span>
              {startableNodes.length > 0 && (
                <>
                  <select
                    value={readyId}
                    onChange={event => setReadyId(event.target.value)}
                    aria-label="Ready node"
                  >
                    {startableNodes.map(node => (
                      <option key={node.nodeId} value={node.nodeId}>{label(node.nodeId)}</option>
                    ))}
                  </select>
                  <button
                    type="button"
                    disabled={props.busy || !readyId}
                    onClick={() => props.onStartNode(readyId)}
                  >
                    Start node
                  </button>
                </>
              )}
              {retryNodes.length > 0 && (
                <>
                  <select
                    value={retryId}
                    onChange={event => setRetryId(event.target.value)}
                    aria-label="Ready retry"
                  >
                    {retryNodes.map(node => (
                      <option key={node.nodeId} value={node.nodeId}>{label(node.nodeId)}</option>
                    ))}
                  </select>
                  <input
                    value={retryReason}
                    onChange={event => setRetryReason(event.target.value)}
                    maxLength={16_384}
                    placeholder="Why retry?"
                    aria-label="Retry reason"
                  />
                  <button
                    type="button"
                    disabled={props.busy || !retryId || !retryReason.trim()}
                    onClick={() => props.onTriggerRetry(retryId, retryReason.trim())}
                  >
                    Retry target
                  </button>
                </>
              )}
            </div>
          )}

          {runningId && (
            <>
              <div className="run-action-group">
                <span className="run-action-label">Active</span>
                <select
                  value={runningId}
                  onChange={event => setRunningId(event.target.value)}
                  aria-label="Running node"
                >
                  {runningNodes.map(node => (
                    <option key={node.nodeId} value={node.nodeId}>
                      {label(node.nodeId)} · attempt {node.attempt}
                    </option>
                  ))}
                </select>
                {conditionalEdges.length > 0 && (
                  <select
                    value={edgeId}
                    onChange={event => setEdgeId(event.target.value)}
                    aria-label="Conditional edge"
                  >
                    {conditionalEdges.length > 1 && <option value="">Select branch…</option>}
                    {conditionalEdges.map(edge => (
                      <option key={edge.id} value={edge.id}>{edge.label || edge.condition || edge.id}</option>
                    ))}
                  </select>
                )}
                <input
                  value={operator}
                  onChange={event => setOperator(event.target.value)}
                  maxLength={2_048}
                  placeholder={isHumanGate ? 'Decision actor' : 'Evidence submitted by'}
                  aria-label={isHumanGate ? 'Decision actor' : 'Evidence submitted by'}
                />
              </div>

              <div className="run-action-group evidence-submission">
                <span className="run-action-label">Evidence</span>
                <input
                  value={source}
                  onChange={event => setSource(event.target.value)}
                  maxLength={2_048}
                  placeholder="Source"
                  aria-label="Evidence source"
                />
                <input
                  value={locator}
                  onChange={event => setLocator(event.target.value)}
                  maxLength={2_048}
                  placeholder="Locator or URI"
                  aria-label="Evidence locator"
                />
                <input
                  value={digest}
                  onChange={event => setDigest(event.target.value)}
                  maxLength={2_048}
                  placeholder="Digest (optional)"
                  aria-label="Evidence digest"
                />
                <input
                  value={evidenceSummary}
                  onChange={event => setEvidenceSummary(event.target.value)}
                  maxLength={8_192}
                  placeholder="What this proves"
                  aria-label="Evidence summary"
                />
                <button
                  type="button"
                  disabled={props.busy || !canSubmitEvidence}
                  onClick={() => props.onSubmitEvidence(
                    runningId,
                    operator.trim(),
                    source.trim(),
                    locator.trim(),
                    evidenceSummary.trim(),
                    digest.trim() || undefined,
                  )}
                >
                  Submit evidence
                </button>
              </div>

              <div className="run-action-group">
                <span className="run-action-label">{isHumanGate ? 'Decision' : 'Result'}</span>
                <select
                  multiple
                  value={selectedEvidence}
                  onChange={event => setSelectedEvidence(
                    Array.from(event.currentTarget.selectedOptions, option => option.value),
                  )}
                  disabled={currentEvidence.length === 0}
                  aria-label="Cited evidence"
                  title="Use Ctrl or Cmd to select multiple evidence submissions."
                >
                  {currentEvidence.length === 0 && <option value="">No evidence submitted</option>}
                  {currentEvidence.map(item => (
                    <option key={item.id} value={item.id}>{item.summary} · {item.source}</option>
                  ))}
                </select>
                {isHumanGate ? (
                  <>
                    <input
                      value={decisionReason}
                      onChange={event => setDecisionReason(event.target.value)}
                      maxLength={16_384}
                      placeholder="Decision reason"
                      aria-label="Decision reason"
                    />
                    <button
                      type="button"
                      disabled={props.busy || !operator.trim() || !decisionReason.trim()}
                      onClick={() => decide('approved')}
                    >
                      Approve
                    </button>
                    <button
                      type="button"
                      className="danger-button"
                      disabled={props.busy || !operator.trim() || !decisionReason.trim()}
                      onClick={() => decide('rejected')}
                    >
                      Reject
                    </button>
                  </>
                ) : (
                  <>
                    <input
                      value={detail}
                      onChange={event => setDetail(event.target.value)}
                      maxLength={16_384}
                      placeholder="Result detail"
                      aria-label="Result detail"
                    />
                    <button
                      type="button"
                      disabled={props.busy || (conditionalEdges.length > 0 && !edgeId)}
                      onClick={() => complete('succeeded')}
                    >
                      Succeed
                    </button>
                    <button type="button" disabled={props.busy} onClick={() => complete('failed')}>
                      Fail
                    </button>
                  </>
                )}
              </div>
            </>
          )}
          <button type="button" className="danger-button cancel-run" disabled={props.busy} onClick={props.onCancel}>
            Cancel run
          </button>
        </div>
      )}
      {props.error && <span className="run-error" role="alert">{props.error}</span>}
    </section>
  )
}
