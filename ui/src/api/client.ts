import { parseGraphDocument } from '../model/graph'
import type {
  DocBody,
  DocSummary,
  GraphDocument,
  GraphSummary,
  LibraryResponse,
  ReplayVerification,
  RunCommand,
  RunDocument,
  RunStatus,
  RunSummary,
  SequenceCopyResponse,
  SequenceDiagnostic,
  SequenceTemplateDetail,
  SequenceTemplateSummary,
  QualitySummary,
  ShadowAggregate,
  ShadowSampleRow,
  TelemetrySnapshot,
  UsageSampleRow,
  UsageSummary,
} from '../types'

const GRAPHS_URL = '/api/graphs'
const COMPILE_URL = '/api/skills/compile'
const EXPORT_URL = '/api/skills/export'
const RUNS_URL = '/api/runs'
const SEQUENCES_URL = '/api/sequences'
const RUN_STATUSES = new Set<RunStatus>(['running', 'succeeded', 'failed', 'cancelled'])

export class ApiError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

export class GraphConflictError extends ApiError {
  constructor(message: string) {
    super(message, 409)
    this.name = 'GraphConflictError'
  }
}

async function responseMessage(response: Response): Promise<string> {
  const text = await response.text()
  if (!text) return `${response.status} ${response.statusText}`.trim()
  try {
    const body = JSON.parse(text) as { error?: unknown; message?: unknown }
    if (typeof body.message === 'string') return body.message
    if (typeof body.error === 'string') return body.error
  } catch {
    // Plain-text errors are valid API responses.
  }
  return text
}

async function graphResponse(response: Response): Promise<GraphDocument> {
  if (!response.ok) {
    const message = await responseMessage(response)
    if (response.status === 409) throw new GraphConflictError(message || 'The graph changed on the server.')
    throw new ApiError(message || 'The request failed.', response.status)
  }
  return parseGraphDocument(await response.json())
}

async function runResponse(response: Response): Promise<RunDocument> {
  if (!response.ok) {
    const message = await responseMessage(response)
    throw new ApiError(message || 'The run request failed.', response.status)
  }
  const body: unknown = await response.json()
  if (!isRunDocument(body)) throw new Error('The server returned an invalid run document.')
  return body
}

function isRunDocument(value: unknown): value is RunDocument {
  if (typeof value !== 'object' || value === null) return false
  const run = value as Partial<RunDocument>
  return run.schemaVersion === 'cortex-loom.run.v1'
    && typeof run.id === 'string'
    && typeof run.graphId === 'string'
    && Number.isSafeInteger(run.graphRevision)
    && Number.isSafeInteger(run.revision)
    && typeof run.status === 'string'
    && RUN_STATUSES.has(run.status as RunStatus)
    && Array.isArray(run.nodes)
    && Array.isArray(run.edges)
    && Array.isArray(run.evidence)
    && run.nodes.every(node => typeof node === 'object' && node !== null
      && typeof (node as { nodeId?: unknown }).nodeId === 'string'
      && typeof (node as { status?: unknown }).status === 'string')
    && run.edges.every(edge => typeof edge === 'object' && edge !== null
      && typeof (edge as { edgeId?: unknown }).edgeId === 'string'
      && typeof (edge as { status?: unknown }).status === 'string')
}

export async function loadGraph(id = 'default-control-plane', signal?: AbortSignal): Promise<GraphDocument> {
  return graphResponse(await fetch(`${GRAPHS_URL}/${encodeURIComponent(id)}`, {
    signal,
    headers: { Accept: 'application/json' },
  }))
}

export async function listGraphs(signal?: AbortSignal): Promise<GraphSummary[]> {
  const response = await fetch(GRAPHS_URL, { signal, headers: { Accept: 'application/json' } })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (!Array.isArray(body) || body.some(item => {
    if (typeof item !== 'object' || item === null) return true
    const summary = item as Partial<GraphSummary>
    return typeof summary.id !== 'string'
      || typeof summary.name !== 'string'
      || !Number.isSafeInteger(summary.revision)
      || !Number.isSafeInteger(summary.nodeCount)
      || !Number.isSafeInteger(summary.edgeCount)
      || typeof summary.description !== 'string'
      || typeof summary.origin !== 'string'
      || !['bundled', 'imported', 'local'].includes(summary.originKind as string)
      || !Array.isArray(summary.kinds)
      || (summary.templateId !== undefined && summary.templateId !== null && typeof summary.templateId !== 'string')
  })) {
    throw new Error('The server returned an invalid graph list.')
  }
  return body as GraphSummary[]
}

export async function saveGraph(document: GraphDocument): Promise<GraphDocument> {
  return graphResponse(await fetch(`/api/graphs/${encodeURIComponent(document.id)}`, {
    method: 'PUT',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify(document),
  }))
}

export async function listSequenceTemplates(signal?: AbortSignal): Promise<SequenceTemplateSummary[]> {
  const response = await fetch(`${SEQUENCES_URL}/templates`, {
    signal,
    headers: { Accept: 'application/json' },
  })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (!Array.isArray(body) || body.some(item => !isSequenceTemplate(item))) {
    throw new Error('The server returned an invalid sequence template list.')
  }
  return body as SequenceTemplateSummary[]
}

export async function loadSequenceTemplate(
  id: string,
  signal?: AbortSignal,
): Promise<SequenceTemplateDetail> {
  const response = await fetch(`${SEQUENCES_URL}/templates/${encodeURIComponent(id)}`, {
    signal,
    headers: { Accept: 'application/json' },
  })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (!isSequenceTemplate(body) || typeof (body as { markdown?: unknown }).markdown !== 'string'
    || typeof (body as { graph?: unknown }).graph !== 'object') {
    throw new Error('The server returned an invalid sequence template.')
  }
  return { ...(body as SequenceTemplateDetail), graph: parseGraphDocument((body as SequenceTemplateDetail).graph) }
}

export async function copySequenceTemplate(
  templateId: string,
  graphId: string,
  name: string,
): Promise<SequenceCopyResponse> {
  const response = await fetch(`${SEQUENCES_URL}/templates/${encodeURIComponent(templateId)}/copy`, {
    method: 'POST',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify({ graphId, name }),
  })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (typeof body !== 'object' || body === null || typeof (body as { created?: unknown }).created !== 'boolean') {
    throw new Error('The server returned an invalid sequence copy.')
  }
  return {
    created: (body as { created: boolean }).created,
    graph: parseGraphDocument((body as { graph: unknown }).graph),
  }
}

export async function lintSequence(
  graph: GraphDocument,
  signal?: AbortSignal,
): Promise<SequenceDiagnostic[]> {
  const response = await fetch(`${SEQUENCES_URL}/lint`, {
    method: 'POST',
    signal,
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify(graph),
  })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (!Array.isArray(body) || body.some(item => {
    if (typeof item !== 'object' || item === null) return true
    const diagnostic = item as Partial<SequenceDiagnostic>
    return typeof diagnostic.code !== 'string'
      || typeof diagnostic.message !== 'string'
      || !['warning', 'error'].includes(diagnostic.severity ?? '')
  })) {
    throw new Error('The server returned invalid sequence diagnostics.')
  }
  return body as SequenceDiagnostic[]
}

function isSequenceTemplate(value: unknown): value is SequenceTemplateSummary {
  if (typeof value !== 'object' || value === null) return false
  const template = value as Partial<SequenceTemplateSummary>
  return typeof template.id === 'string'
    && typeof template.title === 'string'
    && typeof template.description === 'string'
    && typeof template.changelog === 'string'
    && typeof template.version === 'object'
    && template.version !== null
    && typeof template.activation === 'object'
    && template.activation !== null
}

export async function compileMarkdown(name: string, markdown: string): Promise<GraphDocument> {
  return graphResponse(await fetch(COMPILE_URL, {
    method: 'POST',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify({ source: name, markdown }),
  }))
}

export async function exportMarkdown(graph: GraphDocument): Promise<string> {
  const response = await fetch(EXPORT_URL, {
    method: 'POST',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify(graph),
  })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (typeof body !== 'object' || body === null || !('markdown' in body)
    || typeof (body as { markdown?: unknown }).markdown !== 'string') {
    throw new Error('The server returned an invalid Markdown export.')
  }
  return (body as { markdown: string }).markdown
}

export async function listRuns(graphId: string, signal?: AbortSignal): Promise<RunSummary[]> {
  const query = new URLSearchParams({ graphId, limit: '50' })
  const response = await fetch(`${RUNS_URL}?${query}`, {
    signal,
    headers: { Accept: 'application/json' },
  })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (!Array.isArray(body) || body.some(item => {
    if (typeof item !== 'object' || item === null) return true
    const run = item as Partial<RunSummary>
    return typeof run.id !== 'string'
      || typeof run.graphId !== 'string'
      || !Number.isSafeInteger(run.graphRevision)
      || !Number.isSafeInteger(run.revision)
      || typeof run.status !== 'string'
      || !RUN_STATUSES.has(run.status as RunStatus)
  })) {
    throw new Error('The server returned an invalid run list.')
  }
  return body as RunSummary[]
}

export async function loadRun(id: string, signal?: AbortSignal): Promise<RunDocument> {
  return runResponse(await fetch(`${RUNS_URL}/${encodeURIComponent(id)}`, {
    signal,
    headers: { Accept: 'application/json' },
  }))
}

export async function loadRunGraph(id: string, signal?: AbortSignal): Promise<GraphDocument> {
  return graphResponse(await fetch(`${RUNS_URL}/${encodeURIComponent(id)}/graph`, {
    signal,
    headers: { Accept: 'application/json' },
  }))
}

export async function createRun(
  id: string,
  graphId: string,
  graphRevision: number,
): Promise<RunDocument> {
  return runResponse(await fetch(RUNS_URL, {
    method: 'POST',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify({ id, graphId, graphRevision }),
  }))
}

export async function applyRunCommand(id: string, command: RunCommand): Promise<RunDocument> {
  return runResponse(await fetch(`${RUNS_URL}/${encodeURIComponent(id)}/commands`, {
    method: 'POST',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify(command),
  }))
}

export async function verifyRunReplay(id: string): Promise<ReplayVerification> {
  const response = await fetch(`${RUNS_URL}/${encodeURIComponent(id)}/replay`, {
    method: 'POST',
    headers: { Accept: 'application/json' },
  })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (typeof body !== 'object' || body === null) {
    throw new Error('The server returned an invalid replay verification.')
  }
  const result = body as Partial<ReplayVerification>
  if (typeof result.matchesPersisted !== 'boolean'
    || !Number.isSafeInteger(result.persistedRevision)
    || !Number.isSafeInteger(result.replayedRevision)
    || !Number.isSafeInteger(result.eventCount)
    || typeof result.runStatus !== 'string'
    || !RUN_STATUSES.has(result.runStatus as RunStatus)) {
    throw new Error('The server returned an invalid replay verification.')
  }
  return result as ReplayVerification
}

// --- Model interaction telemetry -------------------------------------------
// Read-only: the UI never writes telemetry, it only shows what the host
// recorded while the model interacted with the deterministic pipeline.

async function getJson<T>(url: string): Promise<T> {
  const response = await fetch(url, { headers: { accept: 'application/json' } })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  return (await response.json()) as T
}

// --- Methodology library import ---------------------------------------------
// Preview never writes. Import stores only graphs whose id is not taken, so
// running it twice cannot overwrite a workflow the user has since edited.

async function libraryResponse(url: string, path: string): Promise<LibraryResponse> {
  const response = await fetch(url, {
    method: 'POST',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify({ path }),
  })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (typeof body !== 'object' || body === null
    || !Array.isArray((body as Partial<LibraryResponse>).skills)
    || !Array.isArray((body as Partial<LibraryResponse>).skipped)
    || !Array.isArray((body as Partial<LibraryResponse>).notices)) {
    throw new Error('The server returned an invalid library report.')
  }
  return body as LibraryResponse
}

export async function previewLibrary(path: string): Promise<LibraryResponse> {
  return libraryResponse('/api/skills/library/preview', path)
}

export async function importLibrary(path: string): Promise<LibraryResponse> {
  return libraryResponse('/api/skills/library/import', path)
}

// --- In-app documentation ---------------------------------------------------
// The server returns Markdown verbatim from documents baked into the binary,
// so the reader works with no network and nothing to render but text.

export async function listDocs(signal?: AbortSignal): Promise<DocSummary[]> {
  const response = await fetch('/api/docs', { signal, headers: { accept: 'application/json' } })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (!Array.isArray(body) || body.some(item => {
    if (typeof item !== 'object' || item === null) return true
    const doc = item as Partial<DocSummary>
    return typeof doc.id !== 'string'
      || typeof doc.title !== 'string'
      || typeof doc.summary !== 'string'
      || !Number.isSafeInteger(doc.length)
  })) {
    throw new Error('The server returned an invalid document list.')
  }
  return body as DocSummary[]
}

export async function readDoc(id: string, signal?: AbortSignal): Promise<DocBody> {
  const response = await fetch(`/api/docs/${encodeURIComponent(id)}`, {
    signal,
    headers: { accept: 'application/json' },
  })
  if (!response.ok) throw new ApiError(await responseMessage(response), response.status)
  const body: unknown = await response.json()
  if (typeof body !== 'object' || body === null
    || typeof (body as Partial<DocBody>).markdown !== 'string'
    || typeof (body as Partial<DocBody>).title !== 'string') {
    throw new Error('The server returned an invalid document.')
  }
  return body as DocBody
}

export async function fetchTelemetry(): Promise<TelemetrySnapshot> {
  const [usage, quality, shadow, shadowSamples, usageSamples] = await Promise.all([
    getJson<UsageSummary>('/api/usage/summary'),
    getJson<QualitySummary>('/api/usage/quality'),
    getJson<ShadowAggregate[]>('/api/shadow/metrics'),
    getJson<ShadowSampleRow[]>('/api/shadow/samples?limit=12'),
    getJson<UsageSampleRow[]>('/api/usage/samples?limit=12'),
  ])
  return { usage, quality, shadow, shadowSamples, usageSamples }
}
