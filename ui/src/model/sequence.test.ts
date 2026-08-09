import { describe, expect, it } from 'vitest'
import { compareSequences, defaultSequenceIdentity, hasBlockingDiagnostics } from './sequence'
import type { GraphDocument, SequenceDiagnostic } from '../types'

function graph(label = 'Inspect source'): GraphDocument {
  return {
    schemaVersion: 'cortex-loom.graph.v1',
    id: 'my-debugging',
    name: 'My debugging',
    revision: 1,
    nodes: [{
      id: 'inspect', kind: 'deterministic', label, description: '',
      position: { x: 0, y: 0 }, provenance: [], config: {},
    }],
    edges: [],
    metadata: { 'sequence.templateId': 'root-cause-debugging' },
  }
}

describe('editable sequences', () => {
  it('creates a stable user-facing identity from a template', () => {
    expect(defaultSequenceIdentity('Root Cause Debugging')).toEqual({
      graphId: 'my-root-cause-debugging',
      name: 'My Root Cause Debugging',
    })
  })

  it('compares a user copy without merging it', () => {
    const template = graph()
    const edited = graph('Inspect the smallest failing path')
    edited.nodes.push({
      id: 'note', kind: 'deterministic', label: 'Record observation', description: '',
      position: { x: 10, y: 10 }, provenance: [], config: {},
    })
    expect(compareSequences(template, edited)).toEqual({
      addedNodes: ['Record observation'],
      removedNodes: [],
      changedNodes: ['Inspect the smallest failing path'],
      addedEdges: 0,
      removedEdges: 0,
    })
  })

  it('blocks a run only for error diagnostics', () => {
    const warning: SequenceDiagnostic = {
      code: 'missing_hint', severity: 'warning', message: 'Add a hint', nodeId: null,
    }
    expect(hasBlockingDiagnostics([warning])).toBe(false)
    expect(hasBlockingDiagnostics([{ ...warning, severity: 'error' }])).toBe(true)
  })
})
