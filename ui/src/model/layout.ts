import type { GraphDocument, GraphEdge, GraphNode, Position } from '../types'

export const CANVAS_WIDTH = 1440
export const CANVAS_HEIGHT = 760
export const NODE_WIDTH = 210
export const NODE_HEIGHT = 88

export function clampPosition(
  position: Position,
  width = CANVAS_WIDTH,
  height = CANVAS_HEIGHT,
): Position {
  return {
    x: Math.max(12, Math.min(width - NODE_WIDTH - 12, position.x)),
    y: Math.max(12, Math.min(height - NODE_HEIGHT - 12, position.y)),
  }
}

function assignLayers(nodes: GraphNode[], edges: GraphEdge[]): Map<string, number> {
  const ids = new Set(nodes.map(node => node.id))
  const outgoing = new Map<string, string[]>()
  const indegree = new Map<string, number>()
  const layers = new Map<string, number>()
  for (const id of ids) {
    outgoing.set(id, [])
    indegree.set(id, 0)
  }
  for (const edge of edges) {
    if (!ids.has(edge.from) || !ids.has(edge.to) || edge.from === edge.to) continue
    outgoing.get(edge.from)?.push(edge.to)
    indegree.set(edge.to, (indegree.get(edge.to) ?? 0) + 1)
  }
  const queue = nodes.filter(node => indegree.get(node.id) === 0).map(node => node.id)
  for (const id of queue) layers.set(id, 0)
  for (let index = 0; index < queue.length; index += 1) {
    const id = queue[index]
    for (const target of outgoing.get(id) ?? []) {
      layers.set(target, Math.max(layers.get(target) ?? 0, (layers.get(id) ?? 0) + 1))
      indegree.set(target, (indegree.get(target) ?? 0) - 1)
      if (indegree.get(target) === 0) queue.push(target)
    }
  }
  let fallbackLayer = Math.max(0, ...layers.values())
  for (const node of nodes) {
    if (!layers.has(node.id)) layers.set(node.id, fallbackLayer += 1)
  }
  return layers
}

export function autoLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  width = CANVAS_WIDTH,
  height = CANVAS_HEIGHT,
): Record<string, Position> {
  if (nodes.length === 0) return {}
  const layerOf = assignLayers(nodes, edges)
  const groups = new Map<number, GraphNode[]>()
  for (const node of nodes) {
    const layer = layerOf.get(node.id) ?? 0
    groups.set(layer, [...(groups.get(layer) ?? []), node])
  }
  const orderedLayers = [...groups.entries()].sort(([left], [right]) => left - right)
  const usableWidth = width - NODE_WIDTH - 64
  const xStep = orderedLayers.length > 1 ? usableWidth / (orderedLayers.length - 1) : 0
  const positions: Record<string, Position> = {}
  orderedLayers.forEach(([, layerNodes], layerIndex) => {
    const usableHeight = height - NODE_HEIGHT - 48
    const yStep = layerNodes.length > 1 ? usableHeight / (layerNodes.length - 1) : 0
    layerNodes
      .slice()
      .sort((left, right) => left.position.y - right.position.y || left.id.localeCompare(right.id))
      .forEach((node, nodeIndex) => {
        positions[node.id] = clampPosition({
          x: orderedLayers.length > 1 ? 32 + layerIndex * xStep : (width - NODE_WIDTH) / 2,
          y: layerNodes.length > 1 ? 24 + nodeIndex * yStep : (height - NODE_HEIGHT) / 2,
        }, width, height)
      })
  })
  return positions
}

export function layoutDocument(document: GraphDocument): GraphDocument {
  const positions = autoLayout(document.nodes, document.edges)
  return {
    ...document,
    nodes: document.nodes.map(node => ({ ...node, position: positions[node.id] ?? node.position })),
  }
}
