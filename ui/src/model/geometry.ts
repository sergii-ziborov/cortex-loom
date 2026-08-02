import { NODE_HEIGHT, NODE_WIDTH } from './layout'
import type { GraphEdge, GraphNode, Position } from '../types'

export interface EdgeGeometry {
  path: string
  label: Position
}

function cubicPoint(start: Position, first: Position, second: Position, end: Position, t: number): Position {
  const rest = 1 - t
  return {
    x: rest ** 3 * start.x + 3 * rest ** 2 * t * first.x + 3 * rest * t ** 2 * second.x + t ** 3 * end.x,
    y: rest ** 3 * start.y + 3 * rest ** 2 * t * first.y + 3 * rest * t ** 2 * second.y + t ** 3 * end.y,
  }
}

export function edgeGeometry(edge: GraphEdge, nodeMap: ReadonlyMap<string, GraphNode>): EdgeGeometry | null {
  const from = nodeMap.get(edge.from)
  const to = nodeMap.get(edge.to)
  if (!from || !to) return null
  const fromCenter = { x: from.position.x + NODE_WIDTH / 2, y: from.position.y + NODE_HEIGHT / 2 }
  const toCenter = { x: to.position.x + NODE_WIDTH / 2, y: to.position.y + NODE_HEIGHT / 2 }
  const dx = toCenter.x - fromCenter.x
  const dy = toCenter.y - fromCenter.y
  const horizontal = Math.abs(dx) >= Math.abs(dy)
  const start: Position = horizontal
    ? { x: fromCenter.x + Math.sign(dx || 1) * NODE_WIDTH / 2, y: fromCenter.y }
    : { x: fromCenter.x, y: fromCenter.y + Math.sign(dy || 1) * NODE_HEIGHT / 2 }
  const end: Position = horizontal
    ? { x: toCenter.x - Math.sign(dx || 1) * NODE_WIDTH / 2, y: toCenter.y }
    : { x: toCenter.x, y: toCenter.y - Math.sign(dy || 1) * NODE_HEIGHT / 2 }
  const bend = Math.max(48, Math.min(180, Math.hypot(dx, dy) * 0.42))
  const first = horizontal
    ? { x: start.x + Math.sign(dx || 1) * bend, y: start.y }
    : { x: start.x, y: start.y + Math.sign(dy || 1) * bend }
  const second = horizontal
    ? { x: end.x - Math.sign(dx || 1) * bend, y: end.y }
    : { x: end.x, y: end.y - Math.sign(dy || 1) * bend }
  return {
    path: `M ${start.x} ${start.y} C ${first.x} ${first.y}, ${second.x} ${second.y}, ${end.x} ${end.y}`,
    label: cubicPoint(start, first, second, end, 0.5),
  }
}
