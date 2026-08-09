import type { GraphDocument } from '../types'

interface AppHeaderProps {
  graph: GraphDocument
  theme: 'dark' | 'light'
  onToggleTheme: () => void
  onOpenTelemetry: () => void
  onOpenHelp: () => void
}

export function AppHeader({ graph, theme, onToggleTheme, onOpenTelemetry, onOpenHelp }: AppHeaderProps) {
  const closeMobileMenu = (target: HTMLElement) => {
    target.closest('details')?.removeAttribute('open')
  }

  const themeIcon = (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      {theme === 'dark' ? (
        <>
          <circle cx="12" cy="12" r="3.25" />
          <path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M18.7 5.3l-1.4 1.4M6.7 17.3l-1.4 1.4" />
        </>
      ) : (
        <path d="M20 15.2A8.4 8.4 0 0 1 8.8 4a8.4 8.4 0 1 0 11.2 11.2Z" />
      )}
    </svg>
  )

  return (
    <header className="app-header">
      <div className="brand-mark" aria-hidden="true">CL</div>
      <div className="brand-copy">
        <strong>Cortex Loom</strong>
        <span>{graph.name}</span>
      </div>
      <div className="header-spacer" />
      <div className="header-actions-desktop">
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
          {themeIcon}
        </button>
      </div>
      <details className="header-actions-menu">
        <summary>Actions</summary>
        <div className="compact-menu-panel">
          <button
            type="button"
            onClick={event => { onOpenTelemetry(); closeMobileMenu(event.currentTarget) }}
          >
            Model interaction
          </button>
          <button
            type="button"
            onClick={event => { onOpenHelp(); closeMobileMenu(event.currentTarget) }}
          >
            Help
          </button>
          <button
            type="button"
            onClick={event => { onToggleTheme(); closeMobileMenu(event.currentTarget) }}
          >
            <span className="compact-menu-icon">{themeIcon}</span>
            Use {theme === 'dark' ? 'light' : 'dark'} theme
          </button>
          <span className="compact-menu-meta" title={`Schema ${graph.schemaVersion}`}>
            Revision {graph.revision} · {graph.schemaVersion}
          </span>
        </div>
      </details>
    </header>
  )
}
