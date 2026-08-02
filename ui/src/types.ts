export type JsonPrimitive = string | number | boolean | null
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue }

export const NODE_KINDS = [
  'input',
  'deterministic',
  'weavatrix',
  'skill',
  'local_model',
  'quality_gate',
  'upstream_agent',
  'output',
] as const

export type NodeKind = (typeof NODE_KINDS)[number]

export const EDGE_KINDS = [
  'sequence',
  'context',
  'tool',
  'success',
  'failure',
  'conditional',
  'fallback',
  'approval',
] as const

export type EdgeKind = (typeof EDGE_KINDS)[number]
export type ExecutionTarget = 'deterministic' | 'weavatrix' | 'ollama' | 'upstream' | 'human'
export type RiskLevel = 'low' | 'medium' | 'high' | 'critical'

export interface Position {
  x: number
  y: number
}

export interface ExecutionPolicy {
  target: ExecutionTarget
  risk: RiskLevel
  maxInputTokens: number
  maxOutputTokens: number
  requireEvidence: boolean
  requireUpstreamReview: boolean
  allowMutation: boolean
  modelProfile?: string | null
}

export interface Provenance {
  source: string
  locator: string
  digest?: string | null
}

export interface GraphNode {
  id: string
  kind: NodeKind
  label: string
  description: string
  position: Position
  execution?: ExecutionPolicy | null
  provenance: Provenance[]
  config: Record<string, JsonValue>
}

export interface GraphEdge {
  id: string
  from: string
  to: string
  kind: EdgeKind
  label: string
  condition?: string | null
}

export interface GraphDocument {
  schemaVersion: string
  id: string
  name: string
  revision: number
  nodes: GraphNode[]
  edges: GraphEdge[]
  metadata: Record<string, string>
}

export type GraphSelection =
  | { type: 'node'; id: string }
  | { type: 'edge'; id: string }
  | null

export type SavePhase = 'ready' | 'dirty' | 'saving' | 'saved' | 'conflict' | 'error'

export interface SaveState {
  phase: SavePhase
  message: string
}
