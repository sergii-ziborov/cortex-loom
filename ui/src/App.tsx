import { useCallback, useEffect, useState } from 'react'
import type { CSSProperties } from 'react'
import { AppHeader } from './components/AppHeader'
import { GraphCanvas } from './components/GraphCanvas'
import { GraphToolbar } from './components/GraphToolbar'
import { RunControls } from './components/RunControls'
import { ExportDialog } from './components/ExportDialog'
import { HelpPanel } from './components/HelpPanel'
import { ImportDialog } from './components/ImportDialog'
import { LibraryDialog } from './components/LibraryDialog'
import { LibraryPanel } from './components/LibraryPanel'
import { Inspector } from './components/Inspector'
import { TelemetryPanel } from './components/TelemetryPanel'
import { InspectorResizeHandle } from './components/InspectorResizeHandle'
import { useGraphDocument } from './hooks/useGraphDocument'
import { useRunDocument } from './hooks/useRunDocument'
import { useResizableInspector } from './hooks/useResizableInspector'
import { addEdge, addNode, deleteEdge, deleteNode, setNodePosition, updateEdge, updateNode } from './model/graph'
import { CANVAS_HEIGHT, CANVAS_WIDTH, NODE_HEIGHT, NODE_WIDTH, clampPosition, layoutDocument } from './model/layout'
import type { GraphDocument, GraphEdge, GraphNode, GraphSelection, Position } from './types'

type ConnectState = undefined | null | string

function initialTheme(): 'dark' | 'light' {
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

export default function App() {
  const graphState = useGraphDocument()
  const runState = useRunDocument(graphState.graph)
  const inspectorSize = useResizableInspector()
  const [selection, setSelection] = useState<GraphSelection>(null)
  const [connectState, setConnectState] = useState<ConnectState>(undefined)
  const [zoom, setZoom] = useState(100)
  const [theme, setTheme] = useState<'dark' | 'light'>(initialTheme)
  const [importOpen, setImportOpen] = useState(false)
  const [exportOpen, setExportOpen] = useState(false)
  const [telemetryOpen, setTelemetryOpen] = useState(false)
  const [helpOpen, setHelpOpen] = useState(false)
  const [libraryOpen, setLibraryOpen] = useState(false)
  const [browserOpen, setBrowserOpen] = useState(false)
  const graph = graphState.graph

  useEffect(() => {
    document.documentElement.dataset.theme = theme
  }, [theme])

  const moveNode = useCallback((nodeId: string, position: Position) => {
    graphState.editGraph(current => setNodePosition(current, nodeId, position))
  }, [graphState.editGraph])

  if (!graph) {
    return (
      <main className="boot-screen">
        <div className="brand-mark large" aria-hidden="true">CL</div>
        {graphState.loading ? (
          <><h1>Loading control plane</h1><div className="loader" aria-label="Loading" /></>
        ) : (
          <>
            <h1>Control plane unavailable</h1>
            <p>{graphState.loadError || 'The graph could not be loaded.'}</p>
            <button className="primary-button" type="button" onClick={() => void graphState.reload()}>Try again</button>
          </>
        )}
      </main>
    )
  }

  const addNewNode = () => {
    const offset = (graph.nodes.length % 5) * 18
    const result = addNode(graph, clampPosition({
      x: (CANVAS_WIDTH - NODE_WIDTH) / 2 + offset,
      y: (CANVAS_HEIGHT - NODE_HEIGHT) / 2 + offset,
    }))
    graphState.editGraph(() => result.document)
    setSelection({ type: 'node', id: result.node.id })
    setConnectState(undefined)
  }

  const activateNode = (nodeId: string) => {
    if (connectState === undefined) {
      setSelection({ type: 'node', id: nodeId })
      return
    }
    if (connectState === null) {
      setConnectState(nodeId)
      setSelection({ type: 'node', id: nodeId })
      return
    }
    if (connectState === nodeId) {
      setConnectState(null)
      return
    }
    const result = addEdge(graph, connectState, nodeId)
    graphState.editGraph(() => result.document)
    setSelection({ type: 'edge', id: result.edge.id })
    setConnectState(undefined)
  }

  const updateSelectedNode = (previousId: string, node: GraphNode) => {
    graphState.editGraph(current => updateNode(current, previousId, node))
    setSelection({ type: 'node', id: node.id.trim() })
  }

  const updateSelectedEdge = (previousId: string, edge: GraphEdge) => {
    graphState.editGraph(current => updateEdge(current, previousId, edge))
    setSelection({ type: 'edge', id: edge.id.trim() })
  }

  const reload = () => {
    if (graphState.dirty && !window.confirm('Discard unsaved changes and reload the server graph?')) return
    setSelection(null)
    setConnectState(undefined)
    void graphState.reload()
  }

  const importGraph = (document: GraphDocument) => {
    graphState.replaceGraph(document, true)
    setSelection(null)
    setConnectState(undefined)
    setImportOpen(false)
  }

  const connectMessage = connectState === null
    ? 'Choose a source node'
    : connectState === undefined
      ? ''
      : `From “${graph.nodes.find(node => node.id === connectState)?.label ?? connectState}” — choose a target`

  return (
    <div className="app-shell">
      <AppHeader
        graph={graph}
        theme={theme}
        onToggleTheme={() => setTheme(value => value === 'dark' ? 'light' : 'dark')}
        onOpenTelemetry={() => setTelemetryOpen(true)}
        onOpenHelp={() => setHelpOpen(true)}
      />
      <GraphToolbar
        connectActive={connectState !== undefined}
        connectMessage={connectMessage}
        dirty={graphState.dirty}
        graphId={graph.id}
        graphs={graphState.graphs}
        saveState={graphState.saveState}
        zoom={zoom}
        onAddNode={addNewNode}
        onAutoLayout={() => {
          graphState.editGraph(layoutDocument)
          setSelection(null)
          setConnectState(undefined)
        }}
        onConnect={() => setConnectState(value => value === undefined ? null : undefined)}
        onExport={() => setExportOpen(true)}
        onImport={() => setImportOpen(true)}
        onImportLibrary={() => setLibraryOpen(true)}
        onBrowseLibrary={() => setBrowserOpen(true)}
        onReload={reload}
        onSelectGraph={id => {
          if (graphState.dirty && !window.confirm('Discard unsaved changes and switch graphs?')) return
          setSelection(null)
          setConnectState(undefined)
          void graphState.selectGraph(id)
        }}
        onSave={() => void graphState.save()}
        onZoom={setZoom}
      />
      <RunControls
        graph={graph}
        runGraph={runState.runGraph}
        run={runState.run}
        runs={runState.runs}
        replay={runState.replay}
        busy={runState.busy}
        dirty={graphState.dirty}
        error={runState.error}
        onCreate={() => void runState.create()}
        onSelect={id => void runState.select(id)}
        onStartNode={id => void runState.startNode(id)}
        onSubmitEvidence={(id, actor, source, locator, summary, digest) => {
          void runState.submitEvidence(id, actor, source, locator, summary, digest)
        }}
        onCompleteNode={(id, outcome, edges, evidence, detail) => {
          void runState.completeNode(id, outcome, edges, evidence, detail)
        }}
        onDecideHumanGate={(id, decision, actor, reason, edges, evidence) => {
          void runState.decideHumanGate(id, decision, actor, reason, edges, evidence)
        }}
        onTriggerRetry={(id, reason) => void runState.triggerRetry(id, reason)}
        onVerifyReplay={() => void runState.verifyReplay()}
        onCancel={() => void runState.cancel()}
      />
      <main
        className="editor-main"
        aria-busy={graphState.saveState.phase === 'saving'}
        style={{ '--inspector-width': `${inspectorSize.width}px` } as CSSProperties}
      >
        <section className="canvas-panel" aria-label="Graph workspace">
          <GraphCanvas
            graph={graph}
            run={!graphState.dirty
              && runState.run?.graphId === graph.id
              && runState.run.graphRevision === graph.revision
              ? runState.run
              : null}
            selection={selection}
            connectActive={connectState !== undefined}
            connectFrom={typeof connectState === 'string' ? connectState : null}
            zoom={zoom}
            onActivateNode={activateNode}
            onMoveNode={moveNode}
            onSelect={setSelection}
          />
        </section>
        <InspectorResizeHandle
          width={inspectorSize.width}
          onPointerDown={inspectorSize.startResize}
          onResizeBy={inspectorSize.resizeBy}
          onReset={inspectorSize.reset}
        />
        <Inspector
          graph={graph}
          selection={selection}
          onDeleteNode={nodeId => {
            graphState.editGraph(current => deleteNode(current, nodeId))
            setSelection(null)
            if (connectState === nodeId) setConnectState(undefined)
          }}
          onDeleteEdge={edgeId => {
            graphState.editGraph(current => deleteEdge(current, edgeId))
            setSelection(null)
          }}
          onUpdateDocument={document => graphState.editGraph(() => document)}
          onUpdateNode={updateSelectedNode}
          onUpdateEdge={updateSelectedEdge}
        />
      </main>
      {importOpen && <ImportDialog onClose={() => setImportOpen(false)} onImport={importGraph} />}
      {exportOpen && <ExportDialog graph={graph} onClose={() => setExportOpen(false)} />}
      {telemetryOpen && <TelemetryPanel onClose={() => setTelemetryOpen(false)} />}
      {helpOpen && <HelpPanel onClose={() => setHelpOpen(false)} />}
      {browserOpen && (
        <LibraryPanel
          graphs={graphState.graphs}
          currentId={graph.id}
          onClose={() => setBrowserOpen(false)}
          onOpen={id => {
            if (graphState.dirty && !window.confirm('Discard unsaved changes and open this workflow?')) return
            setSelection(null)
            setConnectState(undefined)
            setBrowserOpen(false)
            void graphState.selectGraph(id)
          }}
          onImportLibrary={() => { setBrowserOpen(false); setLibraryOpen(true) }}
        />
      )}
      {libraryOpen && (
        <LibraryDialog
          onClose={() => setLibraryOpen(false)}
          // Imported graphs appear in the selector without disturbing the
          // document currently open, saved or not.
          onImported={() => void graphState.refreshList()}
        />
      )}
    </div>
  )
}
