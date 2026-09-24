// Default conversion settings — the one place the web UI defines them — and
// the mapping from the UI's settings to the API's.
//
// These mirror the server's defaults (GET /api/schema), so a field the UI
// leaves at its default converts exactly as if it had not been sent at all.

import { LIGHT_DEFAULTS, lightVector } from './lighting.js'

// The server's default key light. LIGHT_DEFAULTS holds it rounded to whole
// degrees for the sliders, so at those angles the exact vector is sent.
export const DEFAULT_LIGHT_DIR = [0.35, 0.85, 0.4]

export const SETTINGS_DEFAULTS = {
  max_size: '128',
  voxel_size: '',
  ram_limit: '4.0',
  threads: '4',
  dither: true,
  color_sampling: true,
  brightness: 0,
  contrast: 1,
  saturation: 1,
  no_color_block: 'white',
  schematic_name: '',
  ...LIGHT_DEFAULTS,
}

const DEFAULT_BLOCKS = {
  white: 'minecraft:white_concrete',
  netherrack: 'minecraft:netherrack',
}

function number(value, fallback) {
  const n = Number(value)
  return Number.isFinite(n) ? n : fallback
}

function lightDir(s) {
  const az = number(s.light_azimuth, LIGHT_DEFAULTS.light_azimuth)
  const el = number(s.light_elevation, LIGHT_DEFAULTS.light_elevation)
  if (az === LIGHT_DEFAULTS.light_azimuth && el === LIGHT_DEFAULTS.light_elevation) {
    return DEFAULT_LIGHT_DIR
  }
  return lightVector(az, el)
}

// The `settings` object POST /api/jobs and /api/preview take, built from the
// UI's settings. `withOutput` adds the job-level fields (output folder,
// parallel conversions) a preview has no use for.
export function toApiSettings(settings = {}, { withOutput = false } = {}) {
  const s = { ...SETTINGS_DEFAULTS, ...settings }
  const voxel = String(s.voxel_size ?? '').trim()
  const api = {
    max_size: Math.round(number(s.max_size, 128)),
    voxel_size: voxel === '' ? null : number(voxel, null),
    ram_limit: number(s.ram_limit, 4),
    dither: !!s.dither,
    color_sampling: !!s.color_sampling,
    brightness: number(s.brightness, 0),
    contrast: number(s.contrast, 1),
    saturation: number(s.saturation, 1),
    default_block: DEFAULT_BLOCKS[s.no_color_block] || DEFAULT_BLOCKS.white,
    light_dir: lightDir(s),
    light_ambient: number(s.light_ambient, LIGHT_DEFAULTS.light_ambient),
    light_gloss: number(s.light_gloss, LIGHT_DEFAULTS.light_gloss),
    specular: number(s.specular, LIGHT_DEFAULTS.specular),
    highlight_rejection: number(s.highlight_rejection, LIGHT_DEFAULTS.highlight_rejection),
    highlight_recovery: number(s.highlight_recovery, LIGHT_DEFAULTS.highlight_recovery),
    delight: number(s.delight, LIGHT_DEFAULTS.delight),
    schematic_name: String(s.schematic_name || ''),
  }
  if (s.target) api.target = s.target
  if (withOutput) {
    const dir = s.auto_save ? String(s.output_dir || '').trim() : ''
    api.output_dir = dir || null
    api.threads = Math.max(1, Math.round(number(s.threads, 4)))
  }
  return api
}
