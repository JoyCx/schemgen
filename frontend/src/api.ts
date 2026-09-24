// Client for the SchemGen2 server, API v2 (docs/api.md).

import type {
  DirCheck,
  Health,
  JobView,
  PaletteColors,
  Preview,
  Schema,
  StartedJob,
  Suggestion,
  TexturesInfo,
} from './types'

const BASE = '/api'
const TOKEN_KEY = 'schemgen2.token'

// A server started with --token prints a URL carrying ?token=…; take it from
// the address bar once, keep it for this tab, and drop it from the URL so it
// is not bookmarked or shared by accident.
function initToken(): string {
  try {
    const url = new URL(window.location.href)
    const fromUrl = url.searchParams.get('token')
    if (fromUrl) {
      sessionStorage.setItem(TOKEN_KEY, fromUrl)
      url.searchParams.delete('token')
      window.history.replaceState(null, '', url.toString())
    }
    return sessionStorage.getItem(TOKEN_KEY) || ''
  } catch {
    return ''
  }
}

const token = typeof window === 'undefined' ? '' : initToken()

function authHeaders(extra: Record<string, string> = {}): Record<string, string> {
  return token ? { ...extra, Authorization: `Bearer ${token}` } : extra
}

/** For URLs the browser fetches on its own (EventSource, links, images),
 *  which cannot carry a header. */
export function withToken(url: string): string {
  if (!token) return url
  return `${url}${url.includes('?') ? '&' : '?'}token=${encodeURIComponent(token)}`
}

interface RequestOptions {
  method?: string
  body?: BodyInit
  json?: unknown
  signal?: AbortSignal
  fallbackError?: string
}

export class ApiError extends Error {
  status: number
  constructor(message: string, status: number) {
    super(message)
    this.status = status
  }
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const { method = 'GET', body, json, signal, fallbackError } = options
  const headers = authHeaders(json !== undefined ? { 'Content-Type': 'application/json' } : {})
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers,
    body: json !== undefined ? JSON.stringify(json) : body,
    signal,
  })
  if (res.status === 204) return null as T
  const data = await res.json().catch(() => ({}))
  if (!res.ok) {
    throw new ApiError(data.error || fallbackError || `HTTP ${res.status}`, res.status)
  }
  return data as T
}

// ---- Server ----------------------------------------------------------------

export const fetchHealth = (signal?: AbortSignal) => request<Health>('/health', { signal })
export const fetchSchema = () => request<Schema>('/schema')
export const fetchPalette = (target?: string) =>
  request<PaletteColors>(target ? `/palette?target=${encodeURIComponent(target)}` : '/palette')
/** What this machine calls its file manager, for button labels. */
export const fetchSystemInfo = () => request<{ os: string; file_manager: string }>('/system')
export const fetchTexturesInfo = () => request<TexturesInfo>('/textures')

/** A block texture served from the user's own Minecraft install. */
export const blockTextureUrl = (file: string) => withToken(`${BASE}/textures/block/${file}.png`)

// ---- Jobs ------------------------------------------------------------------

export interface StartResponse {
  jobs: StartedJob[]
  output_dir?: string | null
  ignored?: string[]
}

/** Upload models to convert with shared settings (the API's `settings`
 *  object, already built). One request is one batch. */
export function startJobs(files: File[], settings: Record<string, unknown>) {
  const form = new FormData()
  for (const f of files) form.append('files', f)
  form.append('settings', JSON.stringify(settings))
  return request<StartResponse>('/jobs', {
    method: 'POST',
    body: form,
    fallbackError: 'Upload failed',
  })
}

export const fetchJob = (jobId: string) => request<JobView>(`/jobs/${jobId}`)
export const cancelJob = (jobId: string) =>
  request<JobView | null>(`/jobs/${jobId}`, { method: 'DELETE' })

const FINISHED = new Set(['done', 'error', 'cancelled'])

/** Follow a job until it finishes, calling `onUpdate` with every new state.
 *  Server-sent events, falling back to polling. Returns a function that stops. */
export function watchJob(jobId: string, onUpdate: (view: JobView) => void): () => void {
  let stopped = false
  let source: EventSource | null = null
  let timer: ReturnType<typeof setTimeout> | undefined

  const stop = () => {
    stopped = true
    source?.close()
    clearTimeout(timer)
  }

  const deliver = (view: JobView) => {
    if (stopped) return
    onUpdate(view)
    if (FINISHED.has(view.status)) stop()
  }

  const poll = async () => {
    if (stopped) return
    try {
      deliver(await fetchJob(jobId))
    } catch {
      /* transient — try again */
    }
    if (!stopped) timer = setTimeout(poll, 800)
  }

  if (typeof EventSource === 'undefined') {
    poll()
    return stop
  }
  source = new EventSource(withToken(`${BASE}/jobs/${jobId}/events`))
  for (const event of ['progress', 'done', 'error', 'cancelled']) {
    source.addEventListener(event, (e) => {
      try {
        deliver(JSON.parse((e as MessageEvent).data))
      } catch {
        /* ignore a malformed frame */
      }
    })
  }
  // An event stream the network (or a proxy) breaks is not worth fighting:
  // switch to polling for the rest of this job.
  source.onerror = () => {
    if (stopped) return
    source?.close()
    source = null
    poll()
  }
  return stop
}

export const downloadUrl = (jobId: string) => withToken(`${BASE}/jobs/${jobId}/download`)
export const thumbnailUrl = (jobId: string) => withToken(`${BASE}/jobs/${jobId}/thumbnail.png`)

/** Copy a finished schematic into a folder (no re-conversion). */
export const saveToFolder = (jobId: string, path: string) =>
  request<{ saved_path: string }>(`/jobs/${jobId}/save`, { method: 'POST', json: { path } })

/** Show a finished schematic in the OS file manager, selected. */
export const revealJob = (jobId: string) =>
  request<null>(`/jobs/${jobId}/reveal`, { method: 'POST' })

// ---- Preview ---------------------------------------------------------------

/** Decode the packed block list: base64 of little-endian Int32 quadruples
 *  x, y, z, palette index. */
export function decodeBlocks(base64: string): Int32Array {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return new Int32Array(bytes.buffer)
}

/** Convert at preview resolution without writing a file. */
export async function fetchPreview(
  file: File,
  settings: Record<string, unknown>,
  signal?: AbortSignal,
): Promise<Preview> {
  const form = new FormData()
  form.append('file', file)
  form.append('settings', JSON.stringify(settings))
  const data = await request<Omit<Preview, 'blocks'> & { blocks: string }>('/preview', {
    method: 'POST',
    body: form,
    signal,
    fallbackError: 'Preview failed',
  })
  return { ...data, blocks: decodeBlocks(data.blocks) }
}

// ---- Output folder ---------------------------------------------------------

/** Validate a typed folder path. A bad path is `{ ok: false }`, not an exception. */
export const checkOutputDir = (path: string) =>
  request<DirCheck>('/output-dir/check', { method: 'POST', json: { path } })

/** Likely Litematica schematic folders on this machine. */
export async function fetchOutputDirSuggestions(): Promise<Suggestion[]> {
  const data = await request<{ suggestions?: Suggestion[] }>('/output-dir/suggestions')
  return data.suggestions || []
}

/** Open a folder in the OS file manager. */
export const revealFolder = (path: string) =>
  request<null>('/reveal-folder', { method: 'POST', json: { path } })
