import { useCallback, useEffect, useState } from 'react'
import { listDocs, readDoc } from '../api/client'
import { Markdown } from './Markdown'
import { HELP_TOPICS } from './helpContent'
import type { DocBody, DocSummary } from '../types'

interface HelpPanelProps {
  onClose: () => void
}

type Selection =
  | { kind: 'topic'; id: string }
  | { kind: 'doc'; id: string }

/**
 * Help and documentation, both offline.
 *
 * The reference topics are compiled into the bundle and the design documents
 * are baked into the server binary, so a running instance can explain itself
 * with no network and no repository checkout.
 */
export function HelpPanel({ onClose }: HelpPanelProps) {
  const [selection, setSelection] = useState<Selection>({ kind: 'topic', id: HELP_TOPICS[0].id })
  const [docs, setDocs] = useState<DocSummary[]>([])
  const [doc, setDoc] = useState<DocBody | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    const controller = new AbortController()
    listDocs(controller.signal)
      .then(setDocs)
      .catch(cause => {
        if (!controller.signal.aborted) setError(message(cause))
      })
    return () => controller.abort()
  }, [])

  useEffect(() => {
    const handler = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose() }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [onClose])

  const openDoc = useCallback(async (id: string) => {
    setSelection({ kind: 'doc', id })
    setLoading(true)
    setError(null)
    try {
      setDoc(await readDoc(id))
    } catch (cause) {
      setError(message(cause))
      setDoc(null)
    } finally {
      setLoading(false)
    }
  }, [])

  const topic = selection.kind === 'topic'
    ? HELP_TOPICS.find(entry => entry.id === selection.id) ?? HELP_TOPICS[0]
    : null

  return (
    <div className="dialog-backdrop" role="dialog" aria-modal="true" aria-label="Help and documentation">
      <div className="dialog help-dialog">
        <header className="dialog-header">
          <h2>Help</h2>
          <div className="header-spacer" />
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close">×</button>
        </header>

        <div className="help-body">
          <nav className="help-nav" aria-label="Help topics">
            <p className="help-nav-heading">Reference</p>
            {HELP_TOPICS.map(entry => (
              <button
                key={entry.id}
                type="button"
                className={selection.kind === 'topic' && selection.id === entry.id ? 'help-nav-item active' : 'help-nav-item'}
                onClick={() => setSelection({ kind: 'topic', id: entry.id })}
              >
                {entry.title}
              </button>
            ))}
            <p className="help-nav-heading">Documentation</p>
            {docs.length === 0 && <p className="help-nav-empty">No documents available.</p>}
            {docs.map(entry => (
              <button
                key={entry.id}
                type="button"
                className={selection.kind === 'doc' && selection.id === entry.id ? 'help-nav-item active' : 'help-nav-item'}
                onClick={() => void openDoc(entry.id)}
                title={entry.summary}
              >
                {entry.title}
              </button>
            ))}
          </nav>

          <div className="help-content">
            {error && <p className="dialog-error">{error}</p>}
            {topic && (
              <article>
                <h3>{topic.title}</h3>
                <p className="help-lede">{topic.lede}</p>
                {topic.sections.map(section => (
                  <section className="help-section" key={section.heading}>
                    <h4>{section.heading}</h4>
                    {section.note && <p className="help-note">{section.note}</p>}
                    <dl className="help-list">
                      {section.entries.map(([term, meaning]) => (
                        <div className="help-entry" key={term}>
                          <dt><code>{term}</code></dt>
                          <dd>{meaning}</dd>
                        </div>
                      ))}
                    </dl>
                  </section>
                ))}
              </article>
            )}
            {selection.kind === 'doc' && loading && <div className="loader" aria-label="Loading" />}
            {selection.kind === 'doc' && doc && !loading && (
              <article>
                <Markdown source={doc.markdown} />
              </article>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause)
}
