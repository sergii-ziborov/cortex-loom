import type { SaveState } from '../types'

interface GraphToolbarProps {
  connectActive: boolean
  connectMessage: string
  dirty: boolean
  saveState: SaveState
  zoom: number
  onAddNode: () => void
  onAutoLayout: () => void
  onConnect: () => void
  onImport: () => void
  onReload: () => void
  onSave: () => void
  onZoom: (zoom: number) => void
}

export function GraphToolbar(props: GraphToolbarProps) {
  const { connectActive, connectMessage, dirty, saveState, zoom } = props
  const saveDisabled = !dirty || saveState.phase === 'saving'
  return (
    <div className="graph-toolbar" aria-label="Graph controls">
      <div className="toolbar-group">
        <button type="button" className="primary-button" onClick={props.onAddNode}>+ Node</button>
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
      </div>
      {connectActive && <span className="connect-hint" role="status">{connectMessage}</span>}
      <div className="toolbar-spacer" />
      <div className="zoom-controls" aria-label="Canvas zoom">
        <button type="button" onClick={() => props.onZoom(Math.max(50, zoom - 10))} aria-label="Zoom out">−</button>
        <button type="button" onClick={() => props.onZoom(100)} aria-label="Reset zoom">{zoom}%</button>
        <button type="button" onClick={() => props.onZoom(Math.min(180, zoom + 10))} aria-label="Zoom in">+</button>
      </div>
      <button type="button" className="tool-button" onClick={props.onReload}>Reload</button>
      <button type="button" className="save-button" onClick={props.onSave} disabled={saveDisabled}>
        {saveState.phase === 'saving' ? 'Saving…' : 'Save'}
      </button>
      <span className={`save-status ${saveState.phase}`} role="status" title={saveState.message}>
        <span aria-hidden="true" />{saveState.message}
      </span>
    </div>
  )
}
