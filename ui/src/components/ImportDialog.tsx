import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { compileMarkdown } from '../api/client'
import type { GraphDocument } from '../types'

interface ImportDialogProps {
  onClose: () => void
  onImport: (document: GraphDocument) => void
}

export function ImportDialog({ onClose, onImport }: ImportDialogProps) {
  const [name, setName] = useState('Imported skill')
  const [markdown, setMarkdown] = useState('')
  const [error, setError] = useState('')
  const [compiling, setCompiling] = useState(false)

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !compiling) onClose()
    }
    window.addEventListener('keydown', closeOnEscape)
    return () => window.removeEventListener('keydown', closeOnEscape)
  }, [compiling, onClose])

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!name.trim() || !markdown.trim()) {
      setError('Name and Markdown are required.')
      return
    }
    setCompiling(true)
    setError('')
    try {
      onImport(await compileMarkdown(name.trim(), markdown))
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Compilation failed.')
    } finally {
      setCompiling(false)
    }
  }

  return (
    <div className="dialog-backdrop" role="presentation" onMouseDown={event => {
      if (event.currentTarget === event.target && !compiling) onClose()
    }}>
      <section className="dialog" role="dialog" aria-modal="true" aria-labelledby="import-title">
        <div className="dialog-header">
          <div>
            <p className="eyebrow">Skill compiler</p>
            <h2 id="import-title">Import Markdown</h2>
          </div>
          <button className="icon-button" type="button" onClick={onClose} disabled={compiling} aria-label="Close dialog">×</button>
        </div>
        <form onSubmit={event => void submit(event)}>
          <label className="field">
            <span>Name</span>
            <input autoFocus value={name} onChange={event => setName(event.target.value)} />
          </label>
          <label className="field">
            <span>Markdown</span>
            <textarea
              className="markdown-input"
              value={markdown}
              onChange={event => setMarkdown(event.target.value)}
              placeholder="# Skill name&#10;&#10;Describe the workflow…"
              spellCheck={false}
            />
          </label>
          {error && <p className="form-error" role="alert">{error}</p>}
          <div className="dialog-actions">
            <button type="button" className="tool-button" onClick={onClose} disabled={compiling}>Cancel</button>
            <button type="submit" className="primary-button" disabled={compiling}>
              {compiling ? 'Compiling…' : 'Compile graph'}
            </button>
          </div>
        </form>
      </section>
    </div>
  )
}
