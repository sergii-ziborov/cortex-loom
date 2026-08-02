import { useCallback, useEffect, useRef, useState } from 'react'
import { GraphConflictError, loadGraph, saveGraph } from '../api/client'
import type { GraphDocument, SaveState } from '../types'

const messageOf = (error: unknown): string => error instanceof Error ? error.message : 'Unknown error'

export interface GraphDocumentState {
  graph: GraphDocument | null
  loadError: string
  loading: boolean
  dirty: boolean
  saveState: SaveState
  editGraph: (update: (current: GraphDocument) => GraphDocument) => void
  replaceGraph: (document: GraphDocument, dirty?: boolean) => void
  reload: () => Promise<void>
  save: () => Promise<void>
}

export function useGraphDocument(): GraphDocumentState {
  const [graph, setGraph] = useState<GraphDocument | null>(null)
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState('')
  const [dirty, setDirty] = useState(false)
  const [saveState, setSaveState] = useState<SaveState>({ phase: 'ready', message: 'Loaded' })
  const graphRef = useRef<GraphDocument | null>(null)
  const editVersion = useRef(0)

  const replaceGraph = useCallback((document: GraphDocument, nextDirty = false) => {
    graphRef.current = document
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
      const document = await loadGraph()
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
    loadGraph(controller.signal)
      .then(document => {
        if (active) replaceGraph(document)
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

  return { graph, loadError, loading, dirty, saveState, editGraph, replaceGraph, reload, save }
}
