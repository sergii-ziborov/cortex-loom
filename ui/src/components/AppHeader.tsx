import type { GraphDocument } from '../types'

interface AppHeaderProps {
  graph: GraphDocument
  theme: 'dark' | 'light'
  onToggleTheme: () => void
  onOpenTelemetry: () => void
}

export function AppHeader({ graph, theme, onToggleTheme, onOpenTelemetry }: AppHeaderProps) {
  return (
    <header className="app-header">
      <div className="brand-mark" aria-hidden="true">CL</div>
      <div className="brand-copy">
        <strong>Cortex Loom</strong>
        <span>{graph.name}</span>
      </div>
      <div className="header-spacer" />
      <button
        className="ghost-button"
        type="button"
        onClick={onOpenTelemetry}
        title="How the model interacted: routing, evidence budget, shadow comparison"
      >
        Model interaction
      </button>
      <span className="revision-chip" title={`Schema ${graph.schemaVersion}`}>rev {graph.revision}</span>
      <button
        className="icon-button"
        type="button"
        onClick={onToggleTheme}
        aria-label={`Use ${theme === 'dark' ? 'light' : 'dark'} theme`}
        title={`Use ${theme === 'dark' ? 'light' : 'dark'} theme`}
      >
        {theme === 'dark' ? 'â˜€' : 'â˜¾'}
      </button>
    </header>
  )
}
