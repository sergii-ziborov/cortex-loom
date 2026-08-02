import { describe, expect, it } from 'vitest'
import { addEdge, addNode, deleteNode, parseGraphDocument, updateNode } from './graph'
import type { GraphDocument, GraphNode } from '../types'

const makeNode = (id: string, x = 0, y = 0): GraphNode => ({
  id,
  kind: 'deterministic',
  label: id,
  description: '',
  position: { x, y },
  execution: null,
  provenance: [],
  config: {},
})

const makeGraph = (): GraphDocument => ({
  schemaVersion: 'cortex-loom.graph.v1',
  id: 'test',
  name: 'Test graph',
  revision: 4,
  nodes: [makeNode('a'), makeNode('b')],
  edges: [{ id: 'edge', from: 'a', to: 'b', kind: 'sequence', label: '', condition: null }],
  metadata: {},
})

describe('graph model', () => {
  it('creates unique canonical node IDs', () => {
    const graph = makeGraph()
    graph.nodes.push(makeNode('new-node'), makeNode('new-node-2'))
    const result = addNode(graph, { x: 20, y: 30 })
    expect(result.node).toMatchObject({
      id: 'new-node-3',
      kind: 'deterministic',
      position: { x: 20, y: 30 },
      provenance: [],
      config: {},
    })
    expect(result.document.nodes).toHaveLength(5)
  })

  it('rewires connected edges when a node ID changes', () => {
    const graph = makeGraph()
    const renamed = { ...graph.nodes[0], id: 'start', label: 'Start' }
    const result = updateNode(graph, 'a', renamed)
    expect(result.edges[0]).toMatchObject({ from: 'start', to: 'b' })
    expect(result.nodes.map(node => node.id)).toEqual(['start', 'b'])
  })

  it('deletes incident edges with a node', () => {
    const result = deleteNode(makeGraph(), 'a')
    expect(result.nodes.map(node => node.id)).toEqual(['b'])
    expect(result.edges).toEqual([])
  })

  it('rejects self-connections', () => {
    expect(() => addEdge(makeGraph(), 'a', 'a')).toThrow('cannot connect to itself')
  })

  it('parses direct and wrapped graph documents', () => {
    const graph = makeGraph()
    expect(parseGraphDocument(graph)).toBe(graph)
    expect(parseGraphDocument({ graph })).toBe(graph)
    expect(() => parseGraphDocument({ revision: 1 })).toThrow('invalid graph document')
  })
})
