import { parseGraphDocument } from '../model/graph'
import type { GraphDocument } from '../types'

const GRAPH_URL = '/api/graphs/default-control-plane'
const COMPILE_URL = '/api/skills/compile'

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

export async function loadGraph(signal?: AbortSignal): Promise<GraphDocument> {
  return graphResponse(await fetch(GRAPH_URL, { signal, headers: { Accept: 'application/json' } }))
}

export async function saveGraph(document: GraphDocument): Promise<GraphDocument> {
  return graphResponse(await fetch(GRAPH_URL, {
    method: 'PUT',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify(document),
  }))
}

export async function compileMarkdown(name: string, markdown: string): Promise<GraphDocument> {
  return graphResponse(await fetch(COMPILE_URL, {
    method: 'POST',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, markdown }),
  }))
}
