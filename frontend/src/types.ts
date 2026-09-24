// Shapes of what the SchemGen2 server sends (API v2, docs/api.md).

export type FieldType =
  'int' | 'float' | 'bool' | 'choice' | 'text' | 'folder' | 'block' | 'direction'

export interface Choice {
  value: string
  label: string
}

/** One setting, as `GET /api/schema` describes it. */
export interface Field {
  key: string
  type: FieldType
  group: string
  label: string
  help?: string
  default: unknown
  /** `conversion` settings change the schematic; `job` ones only how it is run. */
  scope: 'conversion' | 'job'
  min?: number
  max?: number
  step?: number
  /** The range a slider covers; typed values may go beyond it, up to min/max. */
  slider?: [number, number]
  unit?: string
  placeholder?: string
  nullable?: boolean
  advanced?: boolean
  choices?: Choice[]
  /** Only shown while another field has this value. */
  when?: { field: string; equals: unknown }
}

export interface Group {
  key: string
  label: string
  help?: string
}

export interface TargetInfo {
  id: string
  data_version: number
  schematic_version: number
  blocks: number
}

export interface FormatInfo {
  id: string
  label: string
  extension: string
}

export interface Schema {
  version: string
  groups: Group[]
  fields: Field[]
  targets: TargetInfo[]
  formats: FormatInfo[]
  default_target: string
  palette: { blocks: number; entries: number }
  limits: { max_upload_bytes: number }
}

export interface Health {
  status: string
  version: string
  api: number
  palette_entries: number
  palette_blocks: number
  target: string
  data_version: number
  schematic_version: number
  os: string
  auth: boolean
  voxelizer?: string
}

export interface TexturesInfo {
  source: 'jar' | 'folder' | null
  path?: string
  version?: string | null
  textures?: number
}

/** A block and how many of it. */
export interface Material {
  name: string
  count: number
}

export type JobStatus = 'queued' | 'running' | 'done' | 'error' | 'cancelled'

export interface JobResult {
  blocks: number
  unique_blocks: number
  dims: [number, number, number]
  seconds: number
  materials: Material[]
  target: string
  data_version: number
}

/** `GET /api/jobs/{id}`, and every server-sent event about the job. */
export interface JobView {
  id: string
  status: JobStatus
  progress: number
  stage: string
  message: string
  input_name: string
  name: string
  download_name: string
  format: string
  target: string
  created_ms: number
  finished_ms: number | null
  error: string | null
  result: JobResult | null
  saved_path: string | null
  save_error: string | null
  links: { download: string; thumbnail: string; events: string }
}

export interface StartedJob {
  job_id: string
  filename: string
  name: string
}

/** `POST /api/preview`, with the packed blocks decoded. */
export interface Preview {
  dims: [number, number, number]
  /** Block IDs; `blocks` indexes into this. */
  palette: string[]
  /** x, y, z, palette index — four per block. */
  blocks: Int32Array
  materials: Material[]
  count: number
  capped: boolean
  origin: [number, number, number]
  pitch: number
  target: string
  seconds: number
}

/** Block ID → sRGB color, `GET /api/palette`. */
export type PaletteColors = Record<string, [number, number, number]>

export interface Suggestion {
  path: string
  exists: boolean
  instance?: {
    name: string
    launcher: string
    mc_version?: string | null
    target?: string | null
  } | null
}

export type DirCheck = { ok: true; path: string; exists: boolean } | { ok: false; error: string }
