import { useEffect, useState } from 'react'
import { exportMarkdown } from '../api/client'
import type { GraphDocument } from '../types'

interface ExportDialogProps {
  graph: GraphDocument
  onClose: () => void
}

const fileName = (graph: GraphDocument) => {
  const stem = graph.name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')
  return `${stem || 'skill'}.md`
}

export function ExportDialog({ graph, onClose }: ExportDialogProps) {
  const [markdown, setMarkdown] = useState('')
  const [error, setError] = useState('')
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    let active = true
    void exportMarkdown(graph)
      .then(value => {
        if (active) setMarkdown(value)
      })
      .catch(caught => {
        if (active) setError(caught instanceof Error ? caught.message : 'Export failed.')
      })
    return () => {
      active = false
    }
  }, [graph])

  const download = () => {
    const url = URL.createObjectURL(new Blob([markdown], { type: 'text/markdown;charset=utf-8' }))
    const link = document.createElement('a')
    link.href = url
    link.download = fileName(graph)
    link.click()
    URL.revokeObjectURL(url)
  }

  const copy = async () => {
    await navigator.clipboard.writeText(markdown)
    setCopied(true)
  }

  return (
    <div className="dialog-backdrop" role="presentation" onMouseDown={event => {
      if (event.currentTarget === event.target) onClose()
    }}>
      <section className="dialog" role="dialog" aria-modal="true" aria-labelledby="export-title">
        <div className="dialog-header">
          <div><p className="eyebrow">Skill compiler</p><h2 id="export-title">Export Markdown</h2></div>
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close dialog">×</button>
        </div>
        {error ? <p className="form-error" role="alert">{error}</p> : (
          <textarea
            className="markdown-input"
            aria-label="Generated skill Markdown"
            readOnly
            spellCheck={false}
            value={markdown}
            placeholder="Generating Markdown…"
          />
        )}
        <div className="dialog-actions">
          <button type="button" className="tool-button" onClick={onClose}>Close</button>
          <button type="button" className="tool-button" onClick={() => void copy()} disabled={!markdown}>
            {copied ? 'Copied' : 'Copy'}
          </button>
          <button type="button" className="primary-button" onClick={download} disabled={!markdown}>Download</button>
        </div>
      </section>
    </div>
  )
}
