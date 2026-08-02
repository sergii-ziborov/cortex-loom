import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react'
import { edgeGeometry } from '../model/geometry'
import { CANVAS_HEIGHT, CANVAS_WIDTH, NODE_HEIGHT, NODE_WIDTH, clampPosition } from '../model/layout'
import type { GraphDocument, GraphNode, GraphSelection, Position } from '../types'

interface GraphCanvasProps {
  graph: GraphDocument
  selection: GraphSelection
  connectActive: boolean
  connectFrom: string | null
  zoom: number
  onActivateNode: (nodeId: string) => void
  onMoveNode: (nodeId: string, position: Position) => void
  onSelect: (selection: GraphSelection) => void
}

interface DragState {
  id: string
  offsetX: number
  offsetY: number
  moved: boolean
}

const kindLabel = (kind: string) => kind.replaceAll('_', ' ')
const shortText = (value: string, length: number) => value.length > length ? `${value.slice(0, length - 1)}…` : value

export function GraphCanvas({
  graph,
  selection,
  connectActive,
  connectFrom,
  zoom,
  onActivateNode,
  onMoveNode,
  onSelect,
}: GraphCanvasProps) {
  const svgRef = useRef<SVGSVGElement>(null)
  const layerRef = useRef<SVGGElement>(null)
  const dragRef = useRef<DragState | null>(null)
  const draggedClick = useRef(false)
  const [, renderDrag] = useState(0)
  const nodeMap = useMemo(() => new Map(graph.nodes.map(node => [node.id, node])), [graph.nodes])
  const scale = zoom / 100
  const transform = `translate(${CANVAS_WIDTH / 2} ${CANVAS_HEIGHT / 2}) scale(${scale}) translate(${-CANVAS_WIDTH / 2} ${-CANVAS_HEIGHT / 2})`

  const clientPoint = useCallback((clientX: number, clientY: number): Position | null => {
    const svg = svgRef.current
    const layer = layerRef.current
    if (!svg || !layer) return null
    const matrix = layer.getScreenCTM()?.inverse()
    if (!matrix) return null
    const point = svg.createSVGPoint()
    point.x = clientX
    point.y = clientY
    const converted = point.matrixTransform(matrix)
    return { x: converted.x, y: converted.y }
  }, [])

  useEffect(() => {
    const move = (event: PointerEvent) => {
      const drag = dragRef.current
      if (!drag) return
      const point = clientPoint(event.clientX, event.clientY)
      if (!point) return
      drag.moved = true
      onMoveNode(drag.id, clampPosition({ x: point.x - drag.offsetX, y: point.y - drag.offsetY }))
    }
    const end = () => {
      const drag = dragRef.current
      if (!drag) return
      draggedClick.current = drag.moved
      dragRef.current = null
      renderDrag(value => value + 1)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', end)
    window.addEventListener('pointercancel', end)
    return () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', end)
      window.removeEventListener('pointercancel', end)
    }
  }, [clientPoint, onMoveNode])

  const startDrag = (node: GraphNode, event: ReactPointerEvent<SVGGElement>) => {
    if (event.button !== 0 || connectActive) return
    const point = clientPoint(event.clientX, event.clientY)
    if (!point) return
    event.preventDefault()
    event.stopPropagation()
    onSelect({ type: 'node', id: node.id })
    dragRef.current = {
      id: node.id,
      offsetX: point.x - node.position.x,
      offsetY: point.y - node.position.y,
      moved: false,
    }
    renderDrag(value => value + 1)
  }

  const activateNode = (nodeId: string) => {
    if (draggedClick.current) {
      draggedClick.current = false
      return
    }
    onActivateNode(nodeId)
  }

  const nodeKeyDown = (node: GraphNode, event: ReactKeyboardEvent<SVGGElement>) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      onActivateNode(node.id)
      return
    }
    const step = event.shiftKey ? 24 : 8
    const offset = event.key === 'ArrowLeft' ? { x: -step, y: 0 }
      : event.key === 'ArrowRight' ? { x: step, y: 0 }
        : event.key === 'ArrowUp' ? { x: 0, y: -step }
          : event.key === 'ArrowDown' ? { x: 0, y: step }
            : null
    if (offset && !connectActive) {
      event.preventDefault()
      onSelect({ type: 'node', id: node.id })
      onMoveNode(node.id, clampPosition({
        x: node.position.x + offset.x,
        y: node.position.y + offset.y,
      }))
    }
  }

  return (
    <div className="canvas-wrap">
      <p id="canvas-help" className="sr-only">
        Tab to a node or edge. Press Enter to select. Use arrow keys to move a node.
      </p>
      <svg
        ref={svgRef}
        className="graph-canvas"
        viewBox={`0 0 ${CANVAS_WIDTH} ${CANVAS_HEIGHT}`}
        role="application"
        aria-label={`${graph.name} graph canvas`}
        aria-describedby="canvas-help"
        onPointerDown={event => {
          if (event.currentTarget === event.target) onSelect(null)
        }}
      >
        <defs>
          <pattern id="dot-grid" width="28" height="28" patternUnits="userSpaceOnUse">
            <circle cx="1" cy="1" r="1" className="grid-dot" />
          </pattern>
          <marker id="arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
            <path d="M 0 0 L 10 5 L 0 10 z" className="arrow-head" />
          </marker>
        </defs>
        <rect
          width={CANVAS_WIDTH}
          height={CANVAS_HEIGHT}
          className="canvas-background"
          onPointerDown={() => onSelect(null)}
        />
        <rect width={CANVAS_WIDTH} height={CANVAS_HEIGHT} fill="url(#dot-grid)" pointerEvents="none" />
        <g ref={layerRef} transform={transform}>
          <g className="edges-layer">
            {graph.edges.map(edge => {
              const geometry = edgeGeometry(edge, nodeMap)
              if (!geometry) return null
              const selected = selection?.type === 'edge' && selection.id === edge.id
              return (
                <g
                  key={edge.id}
                  className={`graph-edge edge-${edge.kind}${selected ? ' selected' : ''}`}
                  role="button"
                  tabIndex={0}
                  aria-label={`Edge ${edge.label || edge.id}, ${kindLabel(edge.kind)}, from ${edge.from} to ${edge.to}`}
                  onClick={event => { event.stopPropagation(); onSelect({ type: 'edge', id: edge.id }) }}
                  onKeyDown={event => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault()
                      onSelect({ type: 'edge', id: edge.id })
                    }
                  }}
                >
                  <path d={geometry.path} className="edge-hit" />
                  <path d={geometry.path} className="edge-line" markerEnd="url(#arrow)" />
                  {edge.label && (
                    <g transform={`translate(${geometry.label.x} ${geometry.label.y})`} pointerEvents="none">
                      <rect x={-Math.min(88, edge.label.length * 3.7 + 9)} y={-11} width={Math.min(176, edge.label.length * 7.4 + 18)} height={22} rx={6} className="edge-label-bg" />
                      <text className="edge-label" textAnchor="middle" dominantBaseline="middle">{shortText(edge.label, 24)}</text>
                    </g>
                  )}
                </g>
              )
            })}
          </g>
          <g className="nodes-layer">
            {graph.nodes.map(node => {
              const selected = selection?.type === 'node' && selection.id === node.id
              const connecting = connectFrom === node.id
              return (
                <g
                  key={node.id}
                  transform={`translate(${node.position.x} ${node.position.y})`}
                  className={`graph-node kind-${node.kind}${selected ? ' selected' : ''}${connecting ? ' connecting' : ''}`}
                  role="button"
                  tabIndex={0}
                  aria-label={`${node.label}, ${kindLabel(node.kind)} node. ${connectActive ? 'Select for connection.' : 'Use arrow keys to move.'}`}
                  onPointerDown={event => startDrag(node, event)}
                  onClick={event => { event.stopPropagation(); activateNode(node.id) }}
                  onKeyDown={event => nodeKeyDown(node, event)}
                >
                  <rect width={NODE_WIDTH} height={NODE_HEIGHT} rx={13} className="node-shadow" />
                  <rect width={NODE_WIDTH} height={NODE_HEIGHT} rx={13} className="node-card" />
                  <rect width="5" height={NODE_HEIGHT} rx="2.5" className="node-accent" />
                  <text x="17" y="24" className="node-kind">{kindLabel(node.kind)}</text>
                  <text x="17" y="49" className="node-title">{shortText(node.label, 25)}</text>
                  <text x="17" y="70" className="node-description">
                    {shortText(node.description || node.id, 31)}
                  </text>
                  <circle cx={NODE_WIDTH - 15} cy="15" r="4" className="node-port" />
                </g>
              )
            })}
          </g>
        </g>
      </svg>
      {graph.nodes.length === 0 && (
        <div className="canvas-empty">
          <strong>No nodes yet</strong>
          <span>Add a node or import a Markdown skill.</span>
        </div>
      )}
    </div>
  )
}
