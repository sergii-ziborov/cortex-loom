import { EDGE_KINDS, NODE_KINDS } from '../types'
import type { GraphDocument, GraphEdge, GraphNode, JsonValue, Position } from '../types'

const objectValue = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const jsonValue = (value: unknown): value is JsonValue => {
  if (value === null || ['string', 'number', 'boolean'].includes(typeof value)) return true
  if (Array.isArray(value)) return value.every(jsonValue)
  return objectValue(value) && Object.values(value).every(jsonValue)
}

const positionValue = (value: unknown): value is Position =>
  objectValue(value)
  && typeof value.x === 'number'
  && Number.isFinite(value.x)
  && typeof value.y === 'number'
  && Number.isFinite(value.y)

const nodeValue = (value: unknown): value is GraphNode => {
  if (!objectValue(value)) return false
  return typeof value.id === 'string'
    && NODE_KINDS.includes(value.kind as GraphNode['kind'])
    && typeof value.label === 'string'
    && typeof value.description === 'string'
    && positionValue(value.position)
    && Array.isArray(value.provenance)
    && objectValue(value.config)
    && Object.values(value.config).every(jsonValue)
}

const edgeValue = (value: unknown): value is GraphEdge => {
  if (!objectValue(value)) return false
  return typeof value.id === 'string'
    && typeof value.from === 'string'
    && typeof value.to === 'string'
    && EDGE_KINDS.includes(value.kind as GraphEdge['kind'])
    && typeof value.label === 'string'
    && (value.condition === undefined || value.condition === null || typeof value.condition === 'string')
}

export function isGraphDocument(value: unknown): value is GraphDocument {
  if (!objectValue(value) || !objectValue(value.metadata)) return false
  return typeof value.schemaVersion === 'string'
    && typeof value.id === 'string'
    && typeof value.name === 'string'
    && Number.isSafeInteger(value.revision)
    && (value.revision as number) >= 0
    && Array.isArray(value.nodes)
    && value.nodes.every(nodeValue)
    && Array.isArray(value.edges)
    && value.edges.every(edgeValue)
    && Object.values(value.metadata).every(item => typeof item === 'string')
}

export function parseGraphDocument(value: unknown): GraphDocument {
  const candidate = objectValue(value) && 'graph' in value ? value.graph : value
  if (!isGraphDocument(candidate)) throw new Error('The server returned an invalid graph document.')
  return candidate
}

function uniqueId(prefix: string, ids: ReadonlySet<string>): string {
  if (!ids.has(prefix)) return prefix
  let suffix = 2
  while (ids.has(`${prefix}-${suffix}`)) suffix += 1
  return `${prefix}-${suffix}`
}

export function addNode(document: GraphDocument, position: Position): { document: GraphDocument; node: GraphNode } {
  const node: GraphNode = {
    id: uniqueId('new-node', new Set(document.nodes.map(item => item.id))),
    kind: 'deterministic',
    label: 'New node',
    description: '',
    position,
    execution: null,
    provenance: [],
    config: {},
  }
  return { document: { ...document, nodes: [...document.nodes, node] }, node }
}

export function updateNode(document: GraphDocument, previousId: string, node: GraphNode): GraphDocument {
  const id = node.id.trim()
  if (!id) throw new Error('Node ID is required.')
  if (document.nodes.some(item => item.id === id && item.id !== previousId)) {
    throw new Error(`Node ID “${id}” already exists.`)
  }
  if (!Number.isFinite(node.position.x) || !Number.isFinite(node.position.y)) {
    throw new Error('Node position must be finite.')
  }
  const nextNode = { ...node, id }
  return {
    ...document,
    nodes: document.nodes.map(item => item.id === previousId ? nextNode : item),
    edges: document.edges.map(edge => ({
      ...edge,
      from: edge.from === previousId ? id : edge.from,
      to: edge.to === previousId ? id : edge.to,
    })),
  }
}

export function setNodePosition(document: GraphDocument, nodeId: string, position: Position): GraphDocument {
  return {
    ...document,
    nodes: document.nodes.map(node => node.id === nodeId ? { ...node, position } : node),
  }
}

export function deleteNode(document: GraphDocument, nodeId: string): GraphDocument {
  return {
    ...document,
    nodes: document.nodes.filter(node => node.id !== nodeId),
    edges: document.edges.filter(edge => edge.from !== nodeId && edge.to !== nodeId),
  }
}

export function addEdge(document: GraphDocument, from: string, to: string): { document: GraphDocument; edge: GraphEdge } {
  if (from === to) throw new Error('A node cannot connect to itself.')
  const nodeIds = new Set(document.nodes.map(node => node.id))
  if (!nodeIds.has(from) || !nodeIds.has(to)) throw new Error('Both edge endpoints must exist.')
  const edge: GraphEdge = {
    id: uniqueId('edge', new Set(document.edges.map(item => item.id))),
    from,
    to,
    kind: 'sequence',
    label: '',
    condition: null,
  }
  return { document: { ...document, edges: [...document.edges, edge] }, edge }
}

export function updateEdge(document: GraphDocument, previousId: string, edge: GraphEdge): GraphDocument {
  const id = edge.id.trim()
  if (!id) throw new Error('Edge ID is required.')
  if (edge.from === edge.to) throw new Error('A node cannot connect to itself.')
  if (document.edges.some(item => item.id === id && item.id !== previousId)) {
    throw new Error(`Edge ID “${id}” already exists.`)
  }
  const nodeIds = new Set(document.nodes.map(node => node.id))
  if (!nodeIds.has(edge.from) || !nodeIds.has(edge.to)) throw new Error('Both edge endpoints must exist.')
  return {
    ...document,
    edges: document.edges.map(item => item.id === previousId ? { ...edge, id } : item),
  }
}

export function deleteEdge(document: GraphDocument, edgeId: string): GraphDocument {
  return { ...document, edges: document.edges.filter(edge => edge.id !== edgeId) }
}

export function graphProblems(document: GraphDocument): string[] {
  const problems: string[] = []
  const nodeIds = new Set<string>()
  const edgeIds = new Set<string>()
  if (!document.id.trim()) problems.push('Graph ID is required.')
  if (!document.name.trim()) problems.push('Graph name is required.')
  for (const node of document.nodes) {
    if (!node.id.trim()) problems.push('Every node needs an ID.')
    else if (nodeIds.has(node.id)) problems.push(`Duplicate node ID: ${node.id}`)
    nodeIds.add(node.id)
  }
  for (const edge of document.edges) {
    if (edgeIds.has(edge.id)) problems.push(`Duplicate edge ID: ${edge.id}`)
    edgeIds.add(edge.id)
    if (edge.from === edge.to) problems.push(`Self edge is not allowed: ${edge.id}`)
    if (!nodeIds.has(edge.from) || !nodeIds.has(edge.to)) problems.push(`Missing endpoint on edge: ${edge.id}`)
  }
  return problems
}
