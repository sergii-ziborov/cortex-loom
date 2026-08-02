import type { GraphDocument } from '../types'

interface AppHeaderProps {
  graph: GraphDocument
  theme: 'dark' | 'light'
  onToggleTheme: () => void
}

export function AppHeader({ graph, theme, onToggleTheme }: AppHeaderProps) {
  return (
    <header className="app-header">
      <div className="brand-mark" aria-hidden="true">CL</div>
      <div className="brand-copy">
        <strong>Cortex Loom</strong>
        <span>{graph.name}</span>
      </div>
      <div className="header-spacer" />
      <span className="revision-chip" title={`Schema ${graph.schemaVersion}`}>rev {graph.revision}</span>
      <button
        className="icon-button"
        type="button"
        onClick={onToggleTheme}
        aria-label={`Use ${theme === 'dark' ? 'light' : 'dark'} theme`}
        title={`Use ${theme === 'dark' ? 'light' : 'dark'} theme`}
      >
        {theme === 'dark' ? '☀' : '☾'}
      </button>
    </header>
  )
}
