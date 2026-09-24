// API client for the SchemGen2 server (API v2 — see docs/api.md).

import { toApiSettings } from './settingsDefaults.js'

const BASE = '/api'
const TOKEN_KEY = 'schemgen2.token'

// A server started with --token prints a URL carrying ?token=…; take it from
// the address bar once, keep it for this tab, and drop it from the URL so it
// is not bookmarked or shared by accident.
function initToken() {
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

function authHeaders(extra = {}) {
  return token ? { ...extra, Authorization: `Bearer ${token}` } : extra
}

// For URLs the browser fetches on its own (EventSource, links, <img>), which
// cannot carry a header.
export function withToken(url) {
  if (!token) return url
  return `${url}${url.includes('?') ? '&' : '?'}token=${encodeURIComponent(token)}`
}

async function request(path, { method = 'GET', body, json, signal, fallbackError } = {}) {
  const headers = authHeaders(json !== undefined ? { 'Content-Type': 'application/json' } : {})
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers,
    body: json !== undefined ? JSON.stringify(json) : body,
    signal,
  })
  if (res.status === 204) return null
  const data = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error(data.error || fallbackError || `HTTP ${res.status}`)
  return data
}

// ---- Server ----------------------------------------------------------------

export const fetchHealth = () => request('/health')
export const fetchSchema = () => request('/schema')
export const fetchPalette = () => request('/palette')
// Host info — notably what to call the file manager in button labels.
export const fetchSystemInfo = () => request('/system')

// ---- Jobs ------------------------------------------------------------------

// Upload one or more models with shared settings. Resolves to
// { jobs: [{ job_id, filename, name }], output_dir, … }.
export async function startJobs(files, settings) {
  const form = new FormData()
  for (const f of files) form.append('files', f)
  form.append('settings', JSON.stringify(toApiSettings(settings, { withOutput: true })))
  return request('/jobs', { method: 'POST', body: form, fallbackError: 'Upload failed' })
}

export const fetchJob = (jobId) => request(`/jobs/${jobId}`)
export const cancelJob = (jobId) => request(`/jobs/${jobId}`, { method: 'DELETE' })

const FINISHED = new Set(['done', 'error', 'cancelled'])

// Follow a job until it finishes, calling onUpdate with every new state (the
// same object GET /api/jobs/{id} returns). Uses server-sent events and falls
// back to polling if they are unavailable. Returns a function that stops.
export function watchJob(jobId, onUpdate) {
  let stopped = false
  let source = null
  let timer = null

  const deliver = (view) => {
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

  const stop = () => {
    stopped = true
    source?.close()
    clearTimeout(timer)
  }

  if (typeof EventSource === 'undefined') {
    poll()
    return stop
  }
  source = new EventSource(withToken(`${BASE}/jobs/${jobId}/events`))
  for (const event of ['progress', 'done', 'error', 'cancelled']) {
    source.addEventListener(event, (e) => {
      try {
        deliver(JSON.parse(e.data))
      } catch {
        /* ignore a malformed frame */
      }
    })
  }
  // An event stream the network (or a proxy) breaks is not worth fighting:
  // switch to polling for the rest of this job.
  source.onerror = () => {
    if (stopped) return
    source.close()
    source = null
    poll()
  }
  return stop
}

export const downloadUrl = (jobId) => withToken(`${BASE}/jobs/${jobId}/download`)
export const thumbnailUrl = (jobId) => withToken(`${BASE}/jobs/${jobId}/thumbnail.png`)

// Copy an already-converted schematic into a folder (no re-conversion).
export const saveToFolder = (jobId, path) =>
  request(`/jobs/${jobId}/save`, { method: 'POST', json: { path } })

// Show a finished schematic in the OS file manager, selected.
export const revealJob = (jobId) => request(`/jobs/${jobId}/reveal`, { method: 'POST' })

// ---- Preview ---------------------------------------------------------------

// Decode the packed block list: base64 of little-endian Int32 quadruples
// x, y, z, palette index.
export function decodeBlocks(base64) {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return new Int32Array(bytes.buffer)
}

// Convert at preview resolution without writing a file. Resolves to the
// server's response with `blocks` decoded into an Int32Array.
export async function fetchPreview(file, settings, signal) {
  const form = new FormData()
  form.append('file', file)
  form.append('settings', JSON.stringify(toApiSettings(settings)))
  const data = await request('/preview', {
    method: 'POST',
    body: form,
    signal,
    fallbackError: 'Preview failed',
  })
  return { ...data, blocks: decodeBlocks(data.blocks) }
}

// ---- Output folder ---------------------------------------------------------

// Validate a folder path typed in the UI. Resolves to { ok, path, exists } or
// { ok: false, error } — a bad path is not an exception.
export const checkOutputDir = (path) =>
  request('/output-dir/check', { method: 'POST', json: { path } })

// Likely Litematica schematic folders on this machine.
export async function fetchOutputDirSuggestions() {
  const data = await request('/output-dir/suggestions')
  return data.suggestions || []
}

// Open the output folder itself in the OS file manager.
export const revealFolder = (path) => request('/reveal-folder', { method: 'POST', json: { path } })
