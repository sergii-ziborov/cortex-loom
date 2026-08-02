import { describe, expect, it } from 'vitest'
import { CANVAS_HEIGHT, CANVAS_WIDTH, NODE_HEIGHT, NODE_WIDTH, autoLayout, clampPosition } from './layout'
import type { GraphEdge, GraphNode } from '../types'

const node = (id: string, y = 0): GraphNode => ({
  id,
  kind: 'deterministic',
  label: id,
  description: '',
  position: { x: 0, y },
  execution: null,
  provenance: [],
  config: {},
})

const edge = (id: string, from: string, to: string): GraphEdge => ({
  id,
  from,
  to,
  kind: 'sequence',
  label: '',
  condition: null,
})

describe('autoLayout', () => {
  it('places a DAG from left to right by dependency layer', () => {
    const positions = autoLayout(
      [node('input'), node('work'), node('output')],
      [edge('one', 'input', 'work'), edge('two', 'work', 'output')],
    )
    expect(positions.input.x).toBeLessThan(positions.work.x)
    expect(positions.work.x).toBeLessThan(positions.output.x)
  })

  it('keeps every position inside canvas bounds', () => {
    const nodes = Array.from({ length: 9 }, (_, index) => node(`node-${index}`, index * 12))
    const positions = autoLayout(nodes, nodes.slice(1).map((item, index) => edge(`e-${index}`, nodes[0].id, item.id)))
    expect(Object.keys(positions)).toHaveLength(nodes.length)
    for (const position of Object.values(positions)) {
      expect(position.x).toBeGreaterThanOrEqual(12)
      expect(position.y).toBeGreaterThanOrEqual(12)
      expect(position.x).toBeLessThanOrEqual(CANVAS_WIDTH - NODE_WIDTH - 12)
      expect(position.y).toBeLessThanOrEqual(CANVAS_HEIGHT - NODE_HEIGHT - 12)
    }
  })

  it('lays out every member of a cycle', () => {
    const nodes = [node('a'), node('b')]
    const positions = autoLayout(nodes, [edge('ab', 'a', 'b'), edge('ba', 'b', 'a')])
    expect(Object.keys(positions).sort()).toEqual(['a', 'b'])
    expect(Number.isFinite(positions.a.x)).toBe(true)
    expect(Number.isFinite(positions.b.y)).toBe(true)
  })
})

describe('clampPosition', () => {
  it('clamps negative and oversized coordinates', () => {
    expect(clampPosition({ x: -100, y: 9999 })).toEqual({
      x: 12,
      y: CANVAS_HEIGHT - NODE_HEIGHT - 12,
    })
  })
})
