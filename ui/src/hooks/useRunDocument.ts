import { useCallback, useEffect, useRef, useState } from 'react'
import {
  ApiError,
  applyRunCommand,
  createRun,
  listRuns,
  loadRun,
  loadRunGraph,
  verifyRunReplay,
} from '../api/client'
import type {
  GraphDocument,
  HumanDecision,
  NodeOutcome,
  ReplayVerification,
  RunCommand,
  RunDocument,
  RunSummary,
} from '../types'

export function useRunDocument(graph: GraphDocument | null) {
  const [runs, setRuns] = useState<RunSummary[]>([])
  const [run, setRun] = useState<RunDocument | null>(null)
  const [runGraph, setRunGraph] = useState<GraphDocument | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [replay, setReplay] = useState<ReplayVerification | null>(null)
  const requestGeneration = useRef(0)
  const graphId = graph?.id
  const graphRevision = graph?.revision

  const isCurrent = useCallback(
    (generation: number) => requestGeneration.current === generation,
    [],
  )

  const refreshRuns = useCallback(async (signal?: AbortSignal, generation = requestGeneration.current) => {
    if (!graphId) return
    const summaries = await listRuns(graphId, signal)
    if (isCurrent(generation)) setRuns(summaries)
    return summaries
  }, [graphId, isCurrent])

  useEffect(() => {
    const controller = new AbortController()
    const generation = ++requestGeneration.current
    setRuns([])
    setRun(null)
    setRunGraph(null)
    setBusy(false)
    setError('')
    setReplay(null)
    void refreshRuns(controller.signal, generation)
      .then(async summaries => {
        if (isCurrent(generation) && summaries?.[0]) {
          const [loadedRun, snapshot] = await Promise.all([
            loadRun(summaries[0].id, controller.signal),
            loadRunGraph(summaries[0].id, controller.signal),
          ])
          if (isCurrent(generation)) {
            setRun(loadedRun)
            setRunGraph(snapshot)
          }
        }
      })
      .catch(cause => {
        if (!controller.signal.aborted && isCurrent(generation)) setError(errorMessage(cause))
      })
    return () => controller.abort()
  }, [graphId, graphRevision, isCurrent, refreshRuns])

  const select = useCallback(async (id: string) => {
    const generation = ++requestGeneration.current
    setBusy(true)
    setError('')
    setReplay(null)
    try {
      const [loadedRun, snapshot] = await Promise.all([loadRun(id), loadRunGraph(id)])
      if (!graphId || loadedRun.graphId !== graphId || snapshot.id !== graphId) {
        throw new Error('Selected run does not belong to the active graph.')
      }
      if (isCurrent(generation)) {
        setRun(loadedRun)
        setRunGraph(snapshot)
      }
    } catch (cause) {
      if (isCurrent(generation)) setError(errorMessage(cause))
    } finally {
      if (isCurrent(generation)) setBusy(false)
    }
  }, [graphId, isCurrent])

  const create = useCallback(async () => {
    if (!graph) return
    const generation = ++requestGeneration.current
    setBusy(true)
    setError('')
    try {
      const id = `run-${Date.now().toString(36)}-${crypto.randomUUID().slice(0, 8)}`
      const created = await createRun(id, graph.id, graph.revision)
      if (isCurrent(generation)) {
        setRun(created)
        setRunGraph(graph)
        setReplay(null)
      }
      await refreshRuns(undefined, generation)
    } catch (cause) {
      if (isCurrent(generation)) setError(errorMessage(cause))
    } finally {
      if (isCurrent(generation)) setBusy(false)
    }
  }, [graph, isCurrent, refreshRuns])

  const apply = useCallback(async (command: RunCommand) => {
    if (!run) return
    const generation = ++requestGeneration.current
    const runId = run.id
    setBusy(true)
    setError('')
    try {
      const updated = await applyRunCommand(run.id, command)
      if (isCurrent(generation)) {
        setRun(updated)
        setReplay(null)
      }
      await refreshRuns(undefined, generation)
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 409) {
        try {
          const reloaded = await loadRun(runId)
          if (isCurrent(generation)) setRun(reloaded)
          await refreshRuns(undefined, generation)
          if (isCurrent(generation)) {
            setError('Run changed on the server. Current state was reloaded.')
          }
        } catch (reloadCause) {
          if (isCurrent(generation)) setError(errorMessage(reloadCause))
        }
      } else {
        if (isCurrent(generation)) setError(errorMessage(cause))
      }
    } finally {
      if (isCurrent(generation)) setBusy(false)
    }
  }, [isCurrent, run, refreshRuns])

  const startNode = useCallback((nodeId: string) => {
    if (!run) return Promise.resolve()
    return apply({ action: 'start_node', expectedRevision: run.revision, nodeId })
  }, [apply, run])

  const completeNode = useCallback((
    nodeId: string,
    outcome: NodeOutcome,
    selectedEdgeIds: string[],
    evidenceIds: string[],
    detail?: string,
  ) => {
    if (!run) return Promise.resolve()
    return apply({
      action: 'complete_node',
      expectedRevision: run.revision,
      nodeId,
      outcome,
      selectedEdgeIds,
      evidenceIds,
      detail: detail || null,
    })
  }, [apply, run])

  const submitEvidence = useCallback((
    nodeId: string,
    submittedBy: string,
    source: string,
    locator: string,
    summary: string,
    digest?: string,
  ) => {
    if (!run) return Promise.resolve()
    return apply({
      action: 'submit_evidence',
      expectedRevision: run.revision,
      nodeId,
      evidenceId: `evidence-${crypto.randomUUID()}`,
      submittedBy,
      source,
      locator,
      digest: digest || null,
      summary,
    })
  }, [apply, run])

  const decideHumanGate = useCallback((
    nodeId: string,
    decision: HumanDecision,
    actor: string,
    reason: string,
    selectedEdgeIds: string[],
    evidenceIds: string[],
  ) => {
    if (!run) return Promise.resolve()
    return apply({
      action: 'decide_human_gate',
      expectedRevision: run.revision,
      nodeId,
      decision,
      actor,
      reason,
      selectedEdgeIds,
      evidenceIds,
    })
  }, [apply, run])

  const triggerRetry = useCallback((retryNodeId: string, reason: string) => {
    if (!run) return Promise.resolve()
    return apply({
      action: 'trigger_retry',
      expectedRevision: run.revision,
      retryNodeId,
      reason,
    })
  }, [apply, run])

  const verifyReplay = useCallback(async () => {
    if (!run) return
    const generation = ++requestGeneration.current
    setBusy(true)
    setError('')
    try {
      const verification = await verifyRunReplay(run.id)
      if (isCurrent(generation)) setReplay(verification)
    } catch (cause) {
      if (isCurrent(generation)) {
        setReplay(null)
        setError(errorMessage(cause))
      }
    } finally {
      if (isCurrent(generation)) setBusy(false)
    }
  }, [isCurrent, run])

  const cancel = useCallback(() => {
    if (!run) return Promise.resolve()
    return apply({
      action: 'cancel',
      expectedRevision: run.revision,
      reason: 'Cancelled from the graph UI',
    })
  }, [apply, run])

  return {
    runs,
    run,
    runGraph,
    busy,
    error,
    replay,
    create,
    select,
    startNode,
    submitEvidence,
    completeNode,
    decideHumanGate,
    triggerRetry,
    verifyReplay,
    cancel,
  }
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : 'Run operation failed.'
}
