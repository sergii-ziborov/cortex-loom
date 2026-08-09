import type { GraphSummary, SaveState } from '../types'

interface GraphToolbarProps {
  connectActive: boolean
  connectMessage: string
  dirty: boolean
  graphId: string
  graphs: GraphSummary[]
  saveState: SaveState
  zoom: number
  onAddNode: () => void
  onAutoLayout: () => void
  onConnect: () => void
  onExport: () => void
  onImport: () => void
  onImportLibrary: () => void
  onBrowseLibrary: () => void
  onReload: () => void
  onSelectGraph: (id: string) => void
  onSave: () => void
  onZoom: (zoom: number) => void
}

export function GraphToolbar(props: GraphToolbarProps) {
  const { connectActive, connectMessage, dirty, saveState, zoom } = props
  const saveDisabled = !dirty || saveState.phase === 'saving'
  const mobileAction = (target: HTMLElement, action: () => void) => {
    action()
    target.closest('details')?.removeAttribute('open')
  }
  return (
    <div className="graph-toolbar" aria-label="Graph controls">
      <div className="toolbar-main">
        <label className="graph-picker">
          <span className="sr-only">Workflow graph</span>
          <select value={props.graphId} onChange={event => props.onSelectGraph(event.target.value)}>
            {props.graphs.map(graph => (
              <option key={graph.id} value={graph.id}>{graph.name} · r{graph.revision}</option>
            ))}
            {!props.graphs.some(graph => graph.id === props.graphId) && (
              <option value={props.graphId}>Unsaved · {props.graphId}</option>
            )}
          </select>
        </label>
        <button type="button" className="primary-button" onClick={props.onAddNode}>+ Node</button>
        <details className="toolbar-actions-menu">
          <summary>Actions</summary>
          <div className="compact-menu-panel">
            <button type="button" onClick={event => mobileAction(event.currentTarget, props.onBrowseLibrary)}>Library</button>
            <button type="button" onClick={event => mobileAction(event.currentTarget, props.onConnect)}>
              {connectActive ? 'Cancel link' : 'Connect nodes'}
            </button>
            <button type="button" onClick={event => mobileAction(event.currentTarget, props.onAutoLayout)}>Auto-layout</button>
            <button type="button" onClick={event => mobileAction(event.currentTarget, props.onImport)}>Import Markdown</button>
            <button type="button" onClick={event => mobileAction(event.currentTarget, props.onExport)}>Export Markdown</button>
            <button type="button" onClick={event => mobileAction(event.currentTarget, props.onImportLibrary)}>Import skill library</button>
            <button type="button" onClick={event => mobileAction(event.currentTarget, props.onReload)}>Reload graph</button>
          </div>
        </details>
      </div>
      <div className="toolbar-actions-desktop">
        <button
          type="button"
          className="tool-button"
          onClick={props.onBrowseLibrary}
          title="Browse every workflow with its purpose, shape, and provenance"
        >
          Library
        </button>
        <button
          type="button"
          className={connectActive ? 'tool-button active' : 'tool-button'}
          onClick={props.onConnect}
          aria-pressed={connectActive}
        >
          {connectActive ? 'Cancel link' : 'Connect'}
        </button>
        <button type="button" className="tool-button" onClick={props.onAutoLayout}>Auto-layout</button>
        <button type="button" className="tool-button" onClick={props.onImport}>Import Markdown</button>
        <button type="button" className="tool-button" onClick={props.onExport}>Export Markdown</button>
      </div>
      {connectActive && <span className="connect-hint" role="status">{connectMessage}</span>}
      <div className="toolbar-spacer" />
      <div className="zoom-controls" aria-label="Canvas zoom">
        <button type="button" onClick={() => props.onZoom(Math.max(50, zoom - 10))} aria-label="Zoom out">−</button>
        <button type="button" onClick={() => props.onZoom(100)} aria-label="Reset zoom">{zoom}%</button>
        <button type="button" onClick={() => props.onZoom(Math.min(180, zoom + 10))} aria-label="Zoom in">+</button>
      </div>
      <button type="button" className="tool-button toolbar-reload" onClick={props.onReload}>Reload</button>
      <button type="button" className="save-button" onClick={props.onSave} disabled={saveDisabled}>
        {saveState.phase === 'saving' ? 'Saving…' : 'Save'}
      </button>
      <span className={`save-status ${saveState.phase}`} role="status" title={saveState.message}>
        <span aria-hidden="true" />{saveState.message}
      </span>
    </div>
  )
}
