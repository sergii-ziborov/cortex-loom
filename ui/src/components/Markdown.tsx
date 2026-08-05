import { Fragment } from 'react'
import type { ReactNode } from 'react'

/**
 * A small Markdown reader for the documents the server bakes into the binary.
 *
 * It renders React elements only — there is no `dangerouslySetInnerHTML` and
 * no HTML passthrough, so a document can never inject markup into the editor.
 * Link targets are restricted to http(s) and in-document anchors; anything
 * else renders as plain text rather than a clickable scheme we did not vet.
 *
 * The supported subset is what the project's own documentation uses:
 * headings, fenced code, bullet and numbered lists, pipe tables, block quotes,
 * paragraphs, and inline code, bold and links.
 */

export type Block =
  | { type: 'heading'; level: number; text: string }
  | { type: 'code'; text: string }
  | { type: 'list'; ordered: boolean; items: string[] }
  | { type: 'table'; header: string[]; rows: string[][] }
  | { type: 'quote'; text: string }
  | { type: 'paragraph'; text: string }
  | { type: 'rule' }

const INLINE = /`([^`]+)`|\*\*([^*]+)\*\*|\[([^\]]+)\]\(([^)\s]+)\)/g

export function Markdown({ source }: { source: string }) {
  return <div className="markdown">{parse(source).map(renderBlock)}</div>
}

function renderBlock(block: Block, index: number): ReactNode {
  const key = `block-${index}`
  switch (block.type) {
    case 'heading': {
      const Tag = `h${Math.min(block.level + 1, 6)}` as 'h2'
      return <Tag key={key}>{inline(block.text)}</Tag>
    }
    case 'code':
      return <pre key={key}><code>{block.text}</code></pre>
    case 'quote':
      return <blockquote key={key}>{inline(block.text)}</blockquote>
    case 'rule':
      return <hr key={key} />
    case 'list':
      return block.ordered
        ? <ol key={key}>{block.items.map((item, at) => <li key={at}>{inline(item)}</li>)}</ol>
        : <ul key={key}>{block.items.map((item, at) => <li key={at}>{inline(item)}</li>)}</ul>
    case 'table':
      return (
        <div className="markdown-scroll" key={key}>
          <table>
            <thead>
              <tr>{block.header.map((cell, at) => <th key={at}>{inline(cell)}</th>)}</tr>
            </thead>
            <tbody>
              {block.rows.map((row, at) => (
                <tr key={at}>{row.map((cell, column) => <td key={column}>{inline(cell)}</td>)}</tr>
              ))}
            </tbody>
          </table>
        </div>
      )
    default:
      return <p key={key}>{inline(block.text)}</p>
  }
}

function inline(text: string): ReactNode {
  const parts: ReactNode[] = []
  let cursor = 0
  INLINE.lastIndex = 0
  for (let match = INLINE.exec(text); match; match = INLINE.exec(text)) {
    if (match.index > cursor) parts.push(text.slice(cursor, match.index))
    const key = `inline-${match.index}`
    if (match[1] !== undefined) parts.push(<code key={key}>{match[1]}</code>)
    else if (match[2] !== undefined) parts.push(<strong key={key}>{match[2]}</strong>)
    else parts.push(link(match[3], match[4], key))
    cursor = match.index + match[0].length
  }
  if (cursor < text.length) parts.push(text.slice(cursor))
  return <Fragment>{parts}</Fragment>
}

function link(label: string, href: string, key: string): ReactNode {
  if (!/^(https?:\/\/|#|\/)/i.test(href)) return <Fragment key={key}>{label}</Fragment>
  const external = /^https?:/i.test(href)
  return (
    <a key={key} href={href} target={external ? '_blank' : undefined} rel={external ? 'noreferrer' : undefined}>
      {label}
    </a>
  )
}

/** Exported for tests: the block structure a document reduces to. */
export function parseMarkdown(source: string): Block[] {
  return parse(source)
}

function parse(source: string): Block[] {
  const lines = source.replace(/\r\n/g, '\n').split('\n')
  const blocks: Block[] = []
  let index = 0
  while (index < lines.length) {
    const line = lines[index]
    if (!line.trim()) { index += 1; continue }
    if (line.startsWith('```')) { index = readCode(lines, index, blocks); continue }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line)
    if (heading) {
      blocks.push({ type: 'heading', level: heading[1].length, text: heading[2].trim() })
      index += 1
      continue
    }
    if (/^(-{3,}|\*{3,}|_{3,})$/.test(line.trim())) { blocks.push({ type: 'rule' }); index += 1; continue }
    if (line.trimStart().startsWith('|')) { index = readTable(lines, index, blocks); continue }
    if (listMarker(line)) { index = readList(lines, index, blocks); continue }
    if (line.trimStart().startsWith('> ')) { index = readQuote(lines, index, blocks); continue }
    index = readParagraph(lines, index, blocks)
  }
  return blocks
}

function readCode(lines: string[], start: number, blocks: Block[]): number {
  let index = start + 1
  const body: string[] = []
  while (index < lines.length && !lines[index].startsWith('```')) {
    body.push(lines[index])
    index += 1
  }
  blocks.push({ type: 'code', text: body.join('\n') })
  return index + 1
}

function listMarker(line: string): { ordered: boolean; text: string } | null {
  const bullet = /^\s*[-*+]\s+(.*)$/.exec(line)
  if (bullet) return { ordered: false, text: bullet[1] }
  const numbered = /^\s*\d+[.)]\s+(.*)$/.exec(line)
  return numbered ? { ordered: true, text: numbered[1] } : null
}

function readList(lines: string[], start: number, blocks: Block[]): number {
  const first = listMarker(lines[start])
  if (!first) return start + 1
  const items: string[] = []
  let index = start
  while (index < lines.length) {
    const marker = listMarker(lines[index])
    if (marker && marker.ordered === first.ordered) {
      items.push(marker.text)
      index += 1
      continue
    }
    // A wrapped continuation line belongs to the item above it.
    if (items.length > 0 && lines[index].startsWith('  ') && lines[index].trim() && !marker) {
      items[items.length - 1] += ` ${lines[index].trim()}`
      index += 1
      continue
    }
    break
  }
  blocks.push({ type: 'list', ordered: first.ordered, items })
  return index
}

function readTable(lines: string[], start: number, blocks: Block[]): number {
  const rows: string[][] = []
  let index = start
  while (index < lines.length && lines[index].trimStart().startsWith('|')) {
    rows.push(cells(lines[index]))
    index += 1
  }
  const [header, separator, ...body] = rows
  if (!separator || !separator.every(cell => /^:?-{2,}:?$/.test(cell.trim()))) {
    // Not a table after all: keep the text rather than dropping it.
    blocks.push({ type: 'paragraph', text: lines.slice(start, index).join(' ') })
    return index
  }
  blocks.push({ type: 'table', header, rows: body })
  return index
}

function cells(line: string): string[] {
  return line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map(cell => cell.trim())
}

function readQuote(lines: string[], start: number, blocks: Block[]): number {
  const body: string[] = []
  let index = start
  while (index < lines.length && lines[index].trimStart().startsWith('> ')) {
    body.push(lines[index].trimStart().slice(2))
    index += 1
  }
  blocks.push({ type: 'quote', text: body.join(' ') })
  return index
}

function readParagraph(lines: string[], start: number, blocks: Block[]): number {
  const body: string[] = []
  let index = start
  while (index < lines.length && lines[index].trim()
    && !lines[index].startsWith('```')
    && !/^#{1,6}\s/.test(lines[index])
    && !lines[index].trimStart().startsWith('|')
    && !lines[index].trimStart().startsWith('> ')
    && !listMarker(lines[index])) {
    body.push(lines[index].trim())
    index += 1
  }
  blocks.push({ type: 'paragraph', text: body.join(' ') })
  return index
}
