import { useCallback, useEffect, useRef, useState } from 'react'
import { GraphConflictError, listGraphs, loadGraph, saveGraph } from '../api/client'
import type { GraphDocument, GraphSummary, SaveState } from '../types'

const messageOf = (error: unknown): string => error instanceof Error ? error.message : 'Unknown error'

export interface GraphDocumentState {
  graph: GraphDocument | null
  graphs: GraphSummary[]
  loadError: string
  loading: boolean
  dirty: boolean
  saveState: SaveState
  editGraph: (update: (current: GraphDocument) => GraphDocument) => void
  replaceGraph: (document: GraphDocument, dirty?: boolean) => void
  selectGraph: (id: string) => Promise<void>
  reload: () => Promise<void>
  /// Re-read the graph list without touching the open document, for when
  /// something else added graphs — a library import, say.
  refreshList: () => Promise<void>
  save: () => Promise<void>
}

export function useGraphDocument(): GraphDocumentState {
  const [graph, setGraph] = useState<GraphDocument | null>(null)
  const [graphs, setGraphs] = useState<GraphSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState('')
  const [dirty, setDirty] = useState(false)
  const [saveState, setSaveState] = useState<SaveState>({ phase: 'ready', message: 'Loaded' })
  const graphRef = useRef<GraphDocument | null>(null)
  const editVersion = useRef(0)
  const selectedId = useRef('default-control-plane')

  const refreshList = useCallback(async () => {
    setGraphs(await listGraphs())
  }, [])

  const replaceGraph = useCallback((document: GraphDocument, nextDirty = false) => {
    graphRef.current = document
    selectedId.current = document.id
    setGraph(document)
    setDirty(nextDirty)
    editVersion.current += nextDirty ? 1 : 0
    setSaveState(nextDirty
      ? { phase: 'dirty', message: 'Unsaved changes' }
      : { phase: 'ready', message: 'Loaded' })
  }, [])

  const reload = useCallback(async () => {
    setLoading(true)
    setLoadError('')
    try {
      const document = await loadGraph(selectedId.current)
      replaceGraph(document)
      await refreshList()
    } catch (error) {
      setLoadError(messageOf(error))
    } finally {
      setLoading(false)
    }
  }, [refreshList, replaceGraph])

  const selectGraph = useCallback(async (id: string) => {
    setLoading(true)
    setLoadError('')
    try {
      const document = await loadGraph(id)
      replaceGraph(document)
    } catch (error) {
      setLoadError(messageOf(error))
    } finally {
      setLoading(false)
    }
  }, [replaceGraph])

  useEffect(() => {
    const controller = new AbortController()
    let active = true
    setLoading(true)
    Promise.all([loadGraph('default-control-plane', controller.signal), listGraphs(controller.signal)])
      .then(([document, summaries]) => {
        if (active) {
          replaceGraph(document)
          setGraphs(summaries)
        }
      })
      .catch(error => {
        if (active && (error as { name?: string }).name !== 'AbortError') setLoadError(messageOf(error))
      })
      .finally(() => {
        if (active) setLoading(false)
      })
    return () => {
      active = false
      controller.abort()
    }
  }, [replaceGraph])

  const editGraph = useCallback((update: (current: GraphDocument) => GraphDocument) => {
    setGraph(current => {
      if (!current) return current
      const next = update(current)
      graphRef.current = next
      return next
    })
    editVersion.current += 1
    setDirty(true)
    setSaveState({ phase: 'dirty', message: 'Unsaved changes' })
  }, [])

  const save = useCallback(async () => {
    const snapshot = graphRef.current
    if (!snapshot) return
    const savedVersion = editVersion.current
    setSaveState({ phase: 'saving', message: 'Saving…' })
    try {
      const stored = await saveGraph(snapshot)
      if (editVersion.current === savedVersion) {
        graphRef.current = stored
        setGraph(stored)
        setGraphs(current => {
          const previous = current.find(item => item.id === stored.id)
          const summary: GraphSummary = {
            id: stored.id,
            name: stored.name,
            revision: stored.revision,
            nodeCount: stored.nodes.length,
            edgeCount: stored.edges.length,
            description: stored.metadata.description ?? previous?.description ?? '',
            // Provenance belongs to the server's view of the document; a save
            // must not silently relabel where a workflow came from.
            origin: previous?.origin ?? 'local',
            originKind: previous?.originKind ?? 'local',
            kinds: [...new Set(stored.nodes.map(node => node.kind))].sort(),
          }
          const remaining = current.filter(item => item.id !== stored.id)
          return [summary, ...remaining]
        })
        setDirty(false)
        setSaveState({ phase: 'saved', message: `Saved revision ${stored.revision}` })
      } else {
        setGraph(current => {
          if (!current) return stored
          const rebased = { ...current, revision: stored.revision }
          graphRef.current = rebased
          return rebased
        })
        setDirty(true)
        setSaveState({ phase: 'dirty', message: 'New changes are not saved' })
      }
    } catch (error) {
      if (error instanceof GraphConflictError) {
        setSaveState({ phase: 'conflict', message: error.message || 'Revision conflict. Reload before saving.' })
      } else {
        setSaveState({ phase: 'error', message: messageOf(error) })
      }
    }
  }, [])

  return {
    graph,
    graphs,
    loadError,
    loading,
    dirty,
    saveState,
    editGraph,
    replaceGraph,
    reload,
    refreshList,
    save,
    selectGraph,
  }
}
