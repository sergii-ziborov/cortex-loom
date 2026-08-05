import { useEffect, useState } from 'react'
import { importLibrary, previewLibrary } from '../api/client'
import type { LibraryResponse } from '../types'

interface LibraryDialogProps {
  onClose: () => void
  onImported: () => void
}

/**
 * Import a methodology library from a local checkout.
 *
 * Nothing is fetched from the network and nothing is copied into this
 * project: the user points at a directory they already have. Preview is
 * mandatory in practice because import is only offered once a preview has
 * succeeded — the licence sitting beside somebody else's skills is shown
 * before anything is stored, not after.
 */
export function LibraryDialog({ onClose, onImported }: LibraryDialogProps) {
  const [path, setPath] = useState('')
  const [report, setReport] = useState<LibraryResponse | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    const handler = (event: KeyboardEvent) => { if (event.key === 'Escape' && !busy) onClose() }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [busy, onClose])

  const run = async (action: 'preview' | 'import') => {
    if (!path.trim()) return
    setBusy(true)
    setError('')
    try {
      const result = action === 'preview'
        ? await previewLibrary(path.trim())
        : await importLibrary(path.trim())
      setReport(result)
      if (action === 'import') onImported()
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
      setReport(null)
    } finally {
      setBusy(false)
    }
  }

  const newlyStored = report?.skills.filter(skill => skill.stored).length ?? 0

  return (
    <div className="dialog-backdrop" role="dialog" aria-modal="true" aria-label="Import methodology library">
      <div className="dialog library-dialog">
        <header className="dialog-header">
          <h2>Import methodology library</h2>
          <div className="header-spacer" />
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close" disabled={busy}>×</button>
        </header>

        <form onSubmit={event => { event.preventDefault(); void run('preview') }}>
          <label>
            <span>Path to a local checkout</span>
            <input
              type="text"
              value={path}
              onChange={event => setPath(event.target.value)}
              placeholder="C:\\checkouts\\superpowers"
              autoFocus
            />
          </label>
          <p className="library-hint">
            Every <code>SKILL.md</code> under the path is compiled. A library that keeps
            Markdown files directly in its root works too, but only when no{' '}
            <code>SKILL.md</code> exists anywhere. Licence and notice files in the root are
            shown below — read them before importing.
          </p>

          {error && <p className="dialog-error">{error}</p>}

          {report && (
            <div className="library-report">
              <p className="library-summary">
                <strong>{report.skills.length}</strong> skills compiled from{' '}
                <code>{report.library}</code>
                {report.imported && <> — <strong>{newlyStored}</strong> stored, {report.skills.length - newlyStored} already present</>}
                {report.skipped.length > 0 && <> · {report.skipped.length} skipped</>}
              </p>

              {report.notices.length > 0 && (
                <section className="library-section">
                  <h3>Attribution found in the library root</h3>
                  {report.notices.map(notice => (
                    <details key={notice.source}>
                      <summary><code>{notice.source}</code></summary>
                      <pre>{notice.text}</pre>
                    </details>
                  ))}
                </section>
              )}
              {report.notices.length === 0 && (
                <p className="library-warning">
                  No licence or notice file was found in the root. Importing copies nothing
                  into this project, but the terms of the source still apply to whatever you
                  do with the result.
                </p>
              )}

              {report.skills.length > 0 && (
                <section className="library-section">
                  <h3>Skills</h3>
                  <table className="telemetry-table">
                    <thead>
                      <tr><th>Name</th><th>Source</th><th>Nodes</th><th>Status</th></tr>
                    </thead>
                    <tbody>
                      {report.skills.map(skill => (
                        <tr key={skill.id}>
                          <td>{skill.name}</td>
                          <td><code>{skill.source}</code></td>
                          <td>{skill.nodeCount}</td>
                          <td>
                            {skill.renamedFrom && <span className="library-note">id taken, stored as {skill.id}</span>}
                            {skill.stored === false && <span className="library-note">already present</span>}
                            {skill.stored === true && <span className="telemetry-ok">stored</span>}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </section>
              )}

              {report.skipped.length > 0 && (
                <section className="library-section">
                  <h3>Skipped</h3>
                  <ul className="library-skipped">
                    {report.skipped.map(skipped => (
                      <li key={skipped.source}><code>{skipped.source}</code> — {skipped.reason}</li>
                    ))}
                  </ul>
                </section>
              )}
            </div>
          )}

          <div className="dialog-actions">
            <button type="button" className="tool-button" onClick={onClose} disabled={busy}>Close</button>
            <button type="submit" className="tool-button" disabled={busy || !path.trim()}>
              {busy ? 'Reading…' : 'Preview'}
            </button>
            <button
              type="button"
              className="primary-button"
              onClick={() => void run('import')}
              disabled={busy || !report || report.skills.length === 0}
              title={report ? '' : 'Preview the library first'}
            >
              Import {report ? `${report.skills.length} skills` : ''}
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
