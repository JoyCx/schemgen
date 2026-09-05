// API client for SchemGen2 backend

const BASE = '/api'

export async function uploadAndConvert(file, options = {}) {
  const form = new FormData()
  form.append('file', file)

  const defaults = {
    max_size: '128',
    voxel_size: '',
    ram_limit: '4.0',
    dither: 'true',
    color_sampling: 'true',
    brightness: '0',
    contrast: '1',
    saturation: '1',
    no_color_block: 'white',
    schematic_name: '',
    output_dir: '',
    auto_save: 'false',
    light_dir: '0.35,0.85,0.40',
    light_ambient: '0.32',
    light_gloss: '0.5',
    specular: '1.1',
    highlight_rejection: '0.75',
    highlight_recovery: '1',
    delight: '0',
  }

  const params = { ...defaults, ...options }
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null) {
      form.append(k, String(v))
    }
  }

  const res = await fetch(`${BASE}/convert`, { method: 'POST', body: form })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: 'Upload failed' }))
    throw new Error(err.error || `HTTP ${res.status}`)
  }
  return res.json()
}

export async function uploadAndConvertBatch(files, options = {}) {
  const form = new FormData()
  for (const f of files) {
    form.append('files', f)
  }

  const defaults = {
    max_size: '128',
    voxel_size: '',
    ram_limit: '4.0',
    threads: '4',
    dither: 'true',
    color_sampling: 'true',
    brightness: '0',
    contrast: '1',
    saturation: '1',
    no_color_block: 'white',
    output_dir: '',
    auto_save: 'false',
    light_dir: '0.35,0.85,0.40',
    light_ambient: '0.32',
    light_gloss: '0.5',
    specular: '1.1',
    highlight_rejection: '0.75',
    highlight_recovery: '1',
    delight: '0',
  }

  const params = { ...defaults, ...options }
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null) {
      form.append(k, String(v))
    }
  }

  const res = await fetch(`${BASE}/convert-batch`, { method: 'POST', body: form })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: 'Batch upload failed' }))
    throw new Error(err.error || `HTTP ${res.status}`)
  }
  return res.json()
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

export async function fetchLitematicPreview(file, options = {}, signal) {
  const form = new FormData()
  form.append('file', file)
  const defaults = {
    max_size: '48', voxel_size: '', ram_limit: '4.0', dither: 'true', color_sampling: 'true',
    brightness: '0', contrast: '1', saturation: '1', no_color_block: 'white', schematic_name: 'preview',
    light_dir: '0.35,0.85,0.40',
    light_ambient: '0.32',
    light_gloss: '0.5',
    specular: '1.1',
    highlight_rejection: '0.75',
    highlight_recovery: '1',
    delight: '0',
  }
  for (const [k, v] of Object.entries({ ...defaults, ...options })) {
    if (v !== undefined && v !== null) form.append(k, String(v))
  }
  const res = await fetch(`${BASE}/preview`, { method: 'POST', body: form, signal })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: `HTTP ${res.status}` }))
    throw new Error(err.error || `HTTP ${res.status}`)
  }
  return res.json()
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
