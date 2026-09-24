// The web UI's settings: the server's schema turned into form state, and form
// state turned back into the `settings` object the API takes.
//
// Form state is keyed exactly like the API, so a field the UI leaves alone
// converts exactly as if it had not been sent. The one UI-only key is
// `auto_save`: whether `output_dir` is used at all.

import type { Field, Schema } from './types'

export type Settings = Record<string, unknown>

export const AUTO_SAVE = 'auto_save'

/** Every field at its default, plus `auto_save` off. */
export function defaultsFrom(schema: Schema): Settings {
  const out: Settings = { [AUTO_SAVE]: false }
  for (const f of schema.fields) out[f.key] = f.default ?? (f.nullable ? null : '')
  if (typeof out.target !== 'string' || !out.target) out.target = schema.default_target
  if (out.output_dir == null) out.output_dir = ''
  return out
}

/** Whether a field applies given the others — `when` in the schema. */
export function isShown(field: Field, settings: Settings): boolean {
  if (!field.when) return true
  return settings[field.when.field] === field.when.equals
}

function toNumber(value: unknown, fallback: number): number {
  if (typeof value === 'string' && value.trim() === '') return fallback
  const n = Number(value)
  return Number.isFinite(n) ? n : fallback
}

/** A field's value as the API wants it. */
export function apiValue(field: Field, value: unknown, settings: Settings): unknown {
  switch (field.type) {
    case 'int':
      return Math.round(toNumber(value, Number(field.default)))
    case 'float': {
      if (field.nullable && (value == null || String(value).trim() === '')) return null
      const fallback = field.default == null ? 0 : Number(field.default)
      const n = toNumber(value, fallback)
      // "0" is how a form says "derive it" for a nullable size.
      return field.nullable && n === 0 ? null : n
    }
    case 'bool':
      return !!value
    case 'direction': {
      const v = Array.isArray(value) ? value.map(Number) : []
      return v.length === 3 && v.every(Number.isFinite) ? v : field.default
    }
    case 'folder': {
      const dir = settings[AUTO_SAVE] ? String(value ?? '').trim() : ''
      return dir || null
    }
    default:
      return value == null ? field.default : String(value)
  }
}

/** The `settings` object for `POST /api/jobs` (with `job`) or
 *  `POST /api/preview` (without: a preview has no output folder or threads). */
export function toApiSettings(
  settings: Settings,
  schema: Schema,
  { job = false }: { job?: boolean } = {},
): Settings {
  const out: Settings = {}
  for (const field of schema.fields) {
    if (field.scope === 'job' && !job) continue
    out[field.key] = apiValue(field, settings[field.key], settings)
  }
  return out
}

/** A string that changes exactly when the schematic would: the
 *  conversion-scope settings as the API would receive them. */
export function conversionKey(settings: Settings, schema: Schema): string {
  const fields = schema.fields.filter((f) => f.scope === 'conversion')
  return JSON.stringify(fields.map((f) => apiValue(f, settings[f.key], settings)))
}

// ---- Remembered choices ----------------------------------------------------

const PREFS_KEY = 'schemgen2.output'

/** Machine-level choices — where schematics go, which version and format —
 *  are remembered between visits; the rest start from the defaults. */
const REMEMBERED = ['output_dir', AUTO_SAVE, 'target', 'format'] as const

export function loadRemembered(schema: Schema): Settings {
  try {
    const raw = localStorage.getItem(PREFS_KEY)
    if (!raw) return {}
    const saved = JSON.parse(raw) as Settings
    const out: Settings = {}
    for (const key of REMEMBERED) {
      if (saved[key] === undefined) continue
      // A remembered target or format this server no longer offers is dropped.
      const field = schema.fields.find((f) => f.key === key)
      if (field?.choices && !field.choices.some((c) => c.value === saved[key])) continue
      out[key] = saved[key]
    }
    return out
  } catch {
    return {}
  }
}

export function saveRemembered(settings: Settings): void {
  try {
    const out: Settings = {}
    for (const key of REMEMBERED) out[key] = settings[key]
    localStorage.setItem(PREFS_KEY, JSON.stringify(out))
  } catch {
    /* private mode — not worth surfacing */
  }
}

// ---- Display ---------------------------------------------------------------

/** How many decimals a step implies: 0.01 → 2. */
export function decimals(step?: number): number {
  if (!step || step >= 1) return 0
  return Math.min(4, Math.max(0, Math.ceil(-Math.log10(step) - 1e-9)))
}

/** A value rounded to its field's step, for display. The schema's f32
 *  defaults (0.3199999928…) come out as the number that was meant. */
export function formatNumber(value: unknown, field: Pick<Field, 'step' | 'type'>): string {
  const n = Number(value)
  if (!Number.isFinite(n)) return ''
  if (field.type === 'int') return String(Math.round(n))
  return String(Number(n.toFixed(decimals(field.step ?? 0.01))))
}
