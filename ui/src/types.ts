export type JsonPrimitive = string | number | boolean | null
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue }

export const NODE_KINDS = [
  'input',
  'deterministic',
  'weavatrix',
  'skill',
  'agent_task',
  'local_model',
  'quality_gate',
  'human_gate',
  'test_gate',
  'review_gate',
  'evidence_gate',
  'branch',
  'retry',
  'handoff',
  'terminal',
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
  'requires',
  'reject',
  'blocks',
  'escalates',
  'invalidates',
  'supersedes',
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

export interface GraphSummary {
  id: string
  name: string
  revision: number
  nodeCount: number
  edgeCount: number
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

export type RunStatus = 'running' | 'succeeded' | 'failed' | 'cancelled'
export type NodeRunStatus = 'pending' | 'ready' | 'running' | 'succeeded' | 'failed' | 'skipped' | 'cancelled'
export type EdgeRunStatus = 'dormant' | 'pending' | 'traversed' | 'not_taken'
export type NodeOutcome = 'succeeded' | 'failed'
export type HumanDecision = 'approved' | 'rejected'

export interface EvidenceSubmission {
  id: string
  nodeId: string
  attempt: number
  submittedBy: string
  source: string
  locator: string
  digest?: string | null
  summary: string
  submittedAt: number
}

export interface HumanDecisionRecord {
  decision: HumanDecision
  actor: string
  reason: string
  evidenceIds: string[]
  decidedAt: number
}

export interface NodeRunState {
  nodeId: string
  status: NodeRunStatus
  attempt: number
  activatedBy: string[]
  evidenceIds: string[]
  detail?: string | null
  humanDecision?: HumanDecisionRecord | null
}

export interface EdgeRunState {
  edgeId: string
  status: EdgeRunStatus
}

export interface RunDocument {
  schemaVersion: string
  id: string
  graphId: string
  graphRevision: number
  revision: number
  status: RunStatus
  nodes: NodeRunState[]
  edges: EdgeRunState[]
  evidence: EvidenceSubmission[]
  createdAt: number
  updatedAt: number
}

export interface RunSummary {
  id: string
  graphId: string
  graphRevision: number
  revision: number
  status: RunStatus
  updatedAt: number
  readyCount: number
  runningCount: number
}

export type RunCommand =
  | { action: 'start_node'; expectedRevision: number; nodeId: string }
  | {
      action: 'submit_evidence'
      expectedRevision: number
      nodeId: string
      evidenceId: string
      submittedBy: string
      source: string
      locator: string
      digest?: string | null
      summary: string
    }
  | {
      action: 'complete_node'
      expectedRevision: number
      nodeId: string
      outcome: NodeOutcome
      selectedEdgeIds: string[]
      evidenceIds: string[]
      detail?: string | null
    }
  | {
      action: 'decide_human_gate'
      expectedRevision: number
      nodeId: string
      decision: HumanDecision
      actor: string
      reason: string
      selectedEdgeIds: string[]
      evidenceIds: string[]
    }
  | {
      action: 'trigger_retry'
      expectedRevision: number
      retryNodeId: string
      reason: string
    }
  | { action: 'cancel'; expectedRevision: number; reason: string }

export interface ReplayVerification {
  matchesPersisted: boolean
  persistedRevision: number
  replayedRevision: number
  eventCount: number
  runStatus: RunStatus
}
