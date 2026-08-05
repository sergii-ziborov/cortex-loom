import type { GraphDocument } from '../types'

interface AppHeaderProps {
  graph: GraphDocument
  theme: 'dark' | 'light'
  onToggleTheme: () => void
  onOpenTelemetry: () => void
  onOpenHelp: () => void
}

export function AppHeader({ graph, theme, onToggleTheme, onOpenTelemetry, onOpenHelp }: AppHeaderProps) {
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
      <button
        className="ghost-button"
        type="button"
        onClick={onOpenHelp}
        title="Node and edge reference, run rules, and the full design documentation"
      >
        Help
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
