import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react'
import { edgeGeometry } from '../model/geometry'
import {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  NODE_HEIGHT,
  NODE_WIDTH,
  canvasViewport,
  clampPosition,
  viewportForNodes,
} from '../model/layout'
import type { GraphDocument, GraphNode, GraphSelection, Position, RunDocument } from '../types'

interface GraphCanvasProps {
  graph: GraphDocument
  run: RunDocument | null
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
  run,
  selection,
  connectActive,
  connectFrom,
  zoom,
  onActivateNode,
  onMoveNode,
  onSelect,
}: GraphCanvasProps) {
  const svgRef = useRef<SVGSVGElement>(null)
  const wrapRef = useRef<HTMLDivElement>(null)
  const layerRef = useRef<SVGGElement>(null)
  const dragRef = useRef<DragState | null>(null)
  const draggedClick = useRef(false)
  const [, renderDrag] = useState(0)
  const [baseViewport, setBaseViewport] = useState(() => canvasViewport(CANVAS_WIDTH, CANVAS_HEIGHT))
  const viewport = useMemo(
    () => viewportForNodes(baseViewport, graph.nodes),
    [baseViewport, graph.nodes],
  )
  const nodeMap = useMemo(() => new Map(graph.nodes.map(node => [node.id, node])), [graph.nodes])
  const nodeRunMap = useMemo(() => new Map(run?.nodes.map(node => [node.nodeId, node]) ?? []), [run])
  const edgeRunMap = useMemo(() => new Map(run?.edges.map(edge => [edge.edgeId, edge]) ?? []), [run])
  const scale = zoom / 100
  const transform = `translate(${CANVAS_WIDTH / 2} 0) scale(${scale}) translate(${-CANVAS_WIDTH / 2} 0)`

  useEffect(() => {
    const wrap = wrapRef.current
    if (!wrap) return
    const update = () => {
      const bounds = wrap.getBoundingClientRect()
      setBaseViewport(canvasViewport(bounds.width, bounds.height))
    }
    update()
    const observer = new ResizeObserver(update)
    observer.observe(wrap)
    return () => observer.disconnect()
  }, [])

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
      onMoveNode(drag.id, clampPosition(
        { x: point.x - drag.offsetX, y: point.y - drag.offsetY },
        viewport.width,
        viewport.height,
        viewport.x,
        viewport.y,
      ))
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
  }, [clientPoint, onMoveNode, viewport])

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
      onMoveNode(node.id, clampPosition(
        {
          x: node.position.x + offset.x,
          y: node.position.y + offset.y,
        },
        viewport.width,
        viewport.height,
        viewport.x,
        viewport.y,
      ))
    }
  }

  return (
    <div ref={wrapRef} className="canvas-wrap">
      <p id="canvas-help" className="sr-only">
        Tab to a node or edge. Press Enter to select. Use arrow keys to move a node.
      </p>
      <svg
        ref={svgRef}
        className="graph-canvas"
        viewBox={`${viewport.x} ${viewport.y} ${viewport.width} ${viewport.height}`}
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
          <clipPath id="node-card-shape">
            <rect width={NODE_WIDTH} height={NODE_HEIGHT} rx="14" />
          </clipPath>
        </defs>
        <rect
          x={viewport.x}
          y={viewport.y}
          width={viewport.width}
          height={viewport.height}
          className="canvas-background"
          onPointerDown={() => onSelect(null)}
        />
        <rect
          x={viewport.x}
          y={viewport.y}
          width={viewport.width}
          height={viewport.height}
          fill="url(#dot-grid)"
          pointerEvents="none"
        />
        <g ref={layerRef} transform={transform}>
          <g className="edges-layer">
            {graph.edges.map(edge => {
              const geometry = edgeGeometry(edge, nodeMap)
              if (!geometry) return null
              const selected = selection?.type === 'edge' && selection.id === edge.id
              const runStatus = edgeRunMap.get(edge.id)?.status
              return (
                <g
                  key={edge.id}
                  className={`graph-edge edge-${edge.kind}${runStatus ? ` run-${runStatus}` : ''}${selected ? ' selected' : ''}`}
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
                  <path d={geometry.path} className="edge-focus" />
                  <path d={geometry.path} className="edge-line" markerEnd="url(#arrow)" />
                  {edge.label && (
                    <g transform={`translate(${geometry.label.x} ${geometry.label.y})`} pointerEvents="none">
                      <text className="edge-label" textAnchor="middle" dominantBaseline="middle">{shortText(edge.label, 20)}</text>
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
              const runState = nodeRunMap.get(node.id)
              return (
                <g
                  key={node.id}
                  transform={`translate(${node.position.x} ${node.position.y})`}
                  className={`graph-node kind-${node.kind}${runState ? ` run-${runState.status}` : ''}${selected ? ' selected' : ''}${connecting ? ' connecting' : ''}`}
                  role="button"
                  tabIndex={0}
                  aria-label={`${node.label}, ${kindLabel(node.kind)} node${runState ? `, run state ${runState.status}` : ''}. ${connectActive ? 'Select for connection.' : 'Use arrow keys to move.'}`}
                  onPointerDown={event => startDrag(node, event)}
                  onClick={event => { event.stopPropagation(); activateNode(node.id) }}
                  onKeyDown={event => nodeKeyDown(node, event)}
                >
                  <rect width={NODE_WIDTH} height={NODE_HEIGHT} rx="14" className="node-shadow" />
                  <rect width={NODE_WIDTH} height={NODE_HEIGHT} rx="14" className="node-card" />
                  <rect width="6" height={NODE_HEIGHT} className="node-accent" clipPath="url(#node-card-shape)" />
                  <text x="19" y="26" className="node-kind">{kindLabel(node.kind)}</text>
                  <text x="19" y="58" className="node-title">{shortText(node.label, 19)}</text>
                  <text x="19" y="83" className="node-description">
                    {shortText(node.description || node.id, 26)}
                  </text>
                  {runState && (
                    <text x={NODE_WIDTH - 17} y="83" className="node-run-status" textAnchor="end">
                      {runState.status}{runState.attempt > 0 ? ` · #${runState.attempt}` : ''}
                    </text>
                  )}
                  <circle cx={NODE_WIDTH - 17} cy="17" r="4" className="node-port" />
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
