import { describe, expect, it } from 'vitest'
import { parseMarkdown } from './Markdown'

describe('parseMarkdown', () => {
  it('reads the block shapes the project documentation uses', () => {
    const blocks = parseMarkdown([
      '# Title',
      '',
      'A paragraph that',
      'wraps across lines.',
      '',
      '- first',
      '- second',
      '',
      '1. step one',
      '2. step two',
      '',
      '| arm | tokens |',
      '| --- | ---: |',
      '| naive | 63745 |',
      '',
      '> a quoted claim',
      '',
      '```powershell',
      'cargo run -p cortex-bench',
      '```',
    ].join('\n'))

    expect(blocks.map(block => block.type)).toEqual([
      'heading', 'paragraph', 'list', 'list', 'table', 'quote', 'code',
    ])
    expect(blocks[1]).toEqual({ type: 'paragraph', text: 'A paragraph that wraps across lines.' })
    expect(blocks[2]).toEqual({ type: 'list', ordered: false, items: ['first', 'second'] })
    expect(blocks[3]).toEqual({ type: 'list', ordered: true, items: ['step one', 'step two'] })
    expect(blocks[4]).toEqual({ type: 'table', header: ['arm', 'tokens'], rows: [['naive', '63745']] })
    expect(blocks[6]).toEqual({ type: 'code', text: 'cargo run -p cortex-bench' })
  })

  it('keeps fenced content out of the block parser', () => {
    // A heading or a list marker inside a fence is code, not structure.
    const blocks = parseMarkdown('```\n# not a heading\n- not a list\n```\n')
    expect(blocks).toEqual([{ type: 'code', text: '# not a heading\n- not a list' }])
  })

  it('keeps pipe text that is not a table', () => {
    const blocks = parseMarkdown('| a | b |\n| c | d |\n')
    expect(blocks).toHaveLength(1)
    expect(blocks[0].type).toBe('paragraph')
  })

  it('joins wrapped list continuations into the item above', () => {
    const blocks = parseMarkdown('- an item that\n  continues below\n- another\n')
    expect(blocks[0]).toEqual({
      type: 'list',
      ordered: false,
      items: ['an item that continues below', 'another'],
    })
  })
})
