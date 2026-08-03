import { describe, expect, it } from 'vitest'
import {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  NODE_HEIGHT,
  NODE_WIDTH,
  autoLayout,
  canvasViewport,
  clampPosition,
  viewportForNodes,
} from './layout'
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

  it('folds a long workflow into spacious columns', () => {
    const nodes = Array.from({ length: 6 }, (_, index) => node(`node-${index}`))
    const edges = nodes.slice(1).map((item, index) => edge(`e-${index}`, nodes[index].id, item.id))
    const positions = autoLayout(nodes, edges)
    expect(positions['node-1'].x - positions['node-0'].x).toBeGreaterThanOrEqual(72 + NODE_WIDTH)
    expect(positions['node-5'].x).toBe(positions['node-4'].x)
    expect(positions['node-5'].y).toBeGreaterThan(positions['node-4'].y)
  })
})

describe('clampPosition', () => {
  it('clamps negative and oversized coordinates', () => {
    expect(clampPosition({ x: -100, y: 9999 })).toEqual({
      x: 12,
      y: CANVAS_HEIGHT - NODE_HEIGHT - 12,
    })
  })

  it('uses responsive viewport bounds without distorting coordinates', () => {
    expect(canvasViewport(CANVAS_WIDTH, CANVAS_HEIGHT)).toEqual({
      x: 0,
      y: 0,
      width: CANVAS_WIDTH,
      height: CANVAS_HEIGHT,
    })
    const tall = canvasViewport(800, 1000)
    expect(tall.width / tall.height).toBeCloseTo(0.8)
    expect(tall.x).toBe(0)
    expect(tall.y).toBeLessThan(0)
    expect(clampPosition(
      { x: -1000, y: -1000 },
      tall.width,
      tall.height,
      tall.x,
      tall.y,
    )).toEqual({ x: 12, y: tall.y + 12 })
  })

  it('keeps nodes moved above the canonical canvas visible after resize', () => {
    const moved = node('moved')
    moved.position = { x: 120, y: -180 }
    const viewport = viewportForNodes(canvasViewport(1400, 600), [moved])
    expect(viewport.y).toBeLessThanOrEqual(-204)
    expect(viewport.y + viewport.height).toBeGreaterThanOrEqual(CANVAS_HEIGHT)
    expect(viewport.width / viewport.height).toBeCloseTo(1400 / 600)
  })
})
