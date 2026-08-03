import type { GraphDocument, GraphEdge, GraphNode, Position } from '../types'

export const CANVAS_WIDTH = 1500
export const CANVAS_HEIGHT = 760
export const NODE_WIDTH = 224
export const NODE_HEIGHT = 104

export interface CanvasViewport {
  x: number
  y: number
  width: number
  height: number
}

export function canvasViewport(clientWidth: number, clientHeight: number): CanvasViewport {
  if (clientWidth <= 0 || clientHeight <= 0) {
    return { x: 0, y: 0, width: CANVAS_WIDTH, height: CANVAS_HEIGHT }
  }
  const clientAspect = clientWidth / clientHeight
  const canvasAspect = CANVAS_WIDTH / CANVAS_HEIGHT
  const width = clientAspect >= canvasAspect ? CANVAS_HEIGHT * clientAspect : CANVAS_WIDTH
  const height = clientAspect >= canvasAspect ? CANVAS_HEIGHT : CANVAS_WIDTH / clientAspect
  return {
    x: (CANVAS_WIDTH - width) / 2,
    y: (CANVAS_HEIGHT - height) / 2,
    width,
    height,
  }
}

export function clampPosition(
  position: Position,
  width = CANVAS_WIDTH,
  height = CANVAS_HEIGHT,
  originX = 0,
  originY = 0,
): Position {
  return {
    x: Math.max(originX + 12, Math.min(originX + width - NODE_WIDTH - 12, position.x)),
    y: Math.max(originY + 12, Math.min(originY + height - NODE_HEIGHT - 12, position.y)),
  }
}

export function viewportForNodes(
  viewport: CanvasViewport,
  nodes: GraphNode[],
  padding = 24,
): CanvasViewport {
  if (nodes.length === 0) return viewport
  const right = viewport.x + viewport.width
  const bottom = viewport.y + viewport.height
  let x = Math.min(viewport.x, ...nodes.map(node => node.position.x - padding))
  let y = Math.min(viewport.y, ...nodes.map(node => node.position.y - padding))
  let maxX = Math.max(right, ...nodes.map(node => node.position.x + NODE_WIDTH + padding))
  let maxY = Math.max(bottom, ...nodes.map(node => node.position.y + NODE_HEIGHT + padding))
  const aspect = viewport.width / viewport.height
  const width = maxX - x
  const height = maxY - y
  if (width / height > aspect) {
    const expandedHeight = width / aspect
    y -= (expandedHeight - height) / 2
    maxY = y + expandedHeight
  } else {
    const expandedWidth = height * aspect
    x -= (expandedWidth - width) / 2
    maxX = x + expandedWidth
  }
  return { x, y, width: maxX - x, height: maxY - y }
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
  const minimumColumnGap = 72
  const maximumColumns = Math.max(
    1,
    Math.floor((width - 64 + minimumColumnGap) / (NODE_WIDTH + minimumColumnGap)),
  )
  const columnCount = Math.min(maximumColumns, orderedLayers.length)
  const columns = new Map<number, GraphNode[]>()
  orderedLayers.forEach(([, layerNodes], layerIndex) => {
    const column = Math.min(layerIndex, columnCount - 1)
    columns.set(column, [...(columns.get(column) ?? []), ...layerNodes])
  })
  const usableWidth = width - NODE_WIDTH - 64
  const xStep = columnCount > 1 ? usableWidth / (columnCount - 1) : 0
  const positions: Record<string, Position> = {}
  columns.forEach((columnNodes, columnIndex) => {
    const verticalMargin = Math.min(96, Math.max(24, (height - NODE_HEIGHT) / 4))
    const usableHeight = height - NODE_HEIGHT - verticalMargin * 2
    const yStep = columnNodes.length > 1 ? usableHeight / (columnNodes.length - 1) : 0
    columnNodes
      .slice()
      .sort((left, right) => left.position.y - right.position.y || left.id.localeCompare(right.id))
      .forEach((node, nodeIndex) => {
        positions[node.id] = clampPosition({
          x: columnCount > 1 ? 32 + columnIndex * xStep : (width - NODE_WIDTH) / 2,
          y: columnNodes.length > 1
            ? verticalMargin + nodeIndex * yStep
            : (height - NODE_HEIGHT) / 2,
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
