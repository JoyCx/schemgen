// API client for SchemGen2 backend

import { conversionFields } from './settingsDefaults.js'

const BASE = '/api'

function appendFields(form, fields) {
  for (const [k, v] of Object.entries(fields)) {
    if (v !== undefined && v !== null) form.append(k, String(v))
  }
}

// Where a finished schematic is copied: only when the toggle is on and a
// folder was typed.
function outputFields(settings) {
  const autoSave = !!settings.auto_save
  return {
    output_dir: autoSave ? (settings.output_dir || '').trim() : '',
    auto_save: autoSave ? 'true' : 'false',
  }
}

async function postForm(path, form, fallbackError, signal) {
  const res = await fetch(`${BASE}${path}`, { method: 'POST', body: form, signal })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: fallbackError }))
    throw new Error(err.error || `HTTP ${res.status}`)
  }
  return res.json()
}

export async function uploadAndConvert(file, settings) {
  const form = new FormData()
  form.append('file', file)
  appendFields(form, { ...conversionFields(settings), ...outputFields(settings) })
  return postForm('/convert', form, 'Upload failed')
}

export async function uploadAndConvertBatch(files, settings) {
  const form = new FormData()
  for (const f of files) form.append('files', f)
  appendFields(form, {
    ...conversionFields(settings),
    ...outputFields(settings),
    threads: settings.threads,
  })
  return postForm('/convert-batch', form, 'Batch upload failed')
}

export async function pollProgress(jobId) {
  const res = await fetch(`${BASE}/progress/${jobId}`)
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}`)
  }
  return res.json()
}

export function downloadUrl(jobId) {
  return `${BASE}/download/${jobId}`
}

export async function fetchPalette() {
  const res = await fetch(`${BASE}/palette`)
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

export async function fetchHealth() {
  const res = await fetch(`${BASE}/health`)
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

export async function fetchLitematicPreview(file, settings, signal) {
  const form = new FormData()
  form.append('file', file)
  appendFields(form, { ...conversionFields(settings), schematic_name: 'preview' })
  return postForm('/preview', form, 'Preview failed', signal)
}

// ---- Output folder ---------------------------------------------------------

// Validate (and create) a folder path typed in the UI.
// Resolves to { ok, path } or { ok: false, error } — a bad path is not an exception.
export async function checkOutputDir(path) {
  const res = await fetch(`${BASE}/output-dir/check`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path }),
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

// Likely Litematica schematic folders on this machine.
export async function fetchOutputDirSuggestions() {
  const res = await fetch(`${BASE}/output-dir/suggestions`)
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  const data = await res.json()
  return data.suggestions || []
}

// Copy an already-converted schematic into a folder (no re-conversion).
export async function saveToFolder(jobId, path) {
  const res = await fetch(`${BASE}/save/${jobId}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path }),
  })
  const data = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`)
  return data
}

// Show a finished schematic in the OS file manager, selected.
export async function revealJob(jobId) {
  const res = await fetch(`${BASE}/reveal/${jobId}`, { method: 'POST' })
  const data = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`)
  return data
}

// Open the output folder itself in the OS file manager.
export async function revealFolder(path) {
  const res = await fetch(`${BASE}/reveal-folder`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path }),
  })
  const data = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`)
  return data
}

// Host info — notably what to call the file manager in button labels.
export async function fetchSystemInfo() {
  const res = await fetch(`${BASE}/system`)
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}
