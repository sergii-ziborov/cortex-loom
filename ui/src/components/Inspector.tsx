import { DocumentInspector } from './DocumentInspector'
import { EdgeInspector } from './EdgeInspector'
import { NodeInspector } from './NodeInspector'
import type { GraphDocument, GraphEdge, GraphNode, GraphSelection } from '../types'

interface InspectorProps {
  graph: GraphDocument
  selection: GraphSelection
  onDeleteEdge: (edgeId: string) => void
  onDeleteNode: (nodeId: string) => void
  onUpdateDocument: (document: GraphDocument) => void
  onUpdateEdge: (previousId: string, edge: GraphEdge) => void
  onUpdateNode: (previousId: string, node: GraphNode) => void
}

export function Inspector(props: InspectorProps) {
  const node = props.selection?.type === 'node'
    ? props.graph.nodes.find(item => item.id === props.selection?.id)
    : undefined
  const edge = props.selection?.type === 'edge'
    ? props.graph.edges.find(item => item.id === props.selection?.id)
    : undefined
  return (
    <aside className="inspector" aria-label="Graph inspector">
      {node ? (
        <NodeInspector
          key={node.id}
          graph={props.graph}
          node={node}
          onUpdate={props.onUpdateNode}
          onDelete={() => props.onDeleteNode(node.id)}
        />
      ) : edge ? (
        <EdgeInspector
          key={edge.id}
          graph={props.graph}
          edge={edge}
          onUpdate={props.onUpdateEdge}
          onDelete={() => props.onDeleteEdge(edge.id)}
        />
      ) : (
        <DocumentInspector graph={props.graph} onUpdate={props.onUpdateDocument} />
      )}
    </aside>
  )
}
