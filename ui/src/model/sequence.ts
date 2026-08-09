import type { GraphDocument, SequenceComparison, SequenceDiagnostic } from '../types'

export function defaultSequenceIdentity(title: string): { graphId: string; name: string } {
  const slug = title
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
  return { graphId: `my-${slug || 'sequence'}`, name: `My ${title.trim() || 'sequence'}` }
}

export function hasBlockingDiagnostics(diagnostics: SequenceDiagnostic[]): boolean {
  return diagnostics.some(diagnostic => diagnostic.severity === 'error')
}

export function compareSequences(
  template: GraphDocument,
  current: GraphDocument,
): SequenceComparison {
  const templateNodes = new Map(template.nodes.map(node => [node.id, node]))
  const currentNodes = new Map(current.nodes.map(node => [node.id, node]))
  const addedNodes = current.nodes
    .filter(node => !templateNodes.has(node.id))
    .map(node => node.label)
  const removedNodes = template.nodes
    .filter(node => !currentNodes.has(node.id))
    .map(node => node.label)
  const changedNodes = current.nodes
    .filter(node => {
      const original = templateNodes.get(node.id)
      return original && JSON.stringify(original) !== JSON.stringify(node)
    })
    .map(node => node.label)
  const templateEdges = new Set(template.edges.map(edge => edge.id))
  const currentEdges = new Set(current.edges.map(edge => edge.id))
  return {
    addedNodes,
    removedNodes,
    changedNodes,
    addedEdges: current.edges.filter(edge => !templateEdges.has(edge.id)).length,
    removedEdges: template.edges.filter(edge => !currentEdges.has(edge.id)).length,
  }
}
