// The settings pane, drawn from GET /api/schema: its groups in order, each
// field by its type. docs/design.md is the written form of what this renders.

import { useId, useState, type ReactNode } from 'react'
import { ChevronDown, RotateCcw } from 'lucide-react'
import { formatNumber, isShown, type Settings } from '../settings'
import type { Field, Schema } from '../types'
import { DirectionField } from './fields/DirectionField'
import { FolderField } from './fields/FolderField'

/** Groups that start open; the rest wait until asked for. */
const OPEN_BY_DEFAULT = new Set(['size', 'color', 'target', 'output'])

interface Props {
  schema: Schema
  settings: Settings
  onChange: (key: string, value: unknown) => void
  onChangeMany: (values: Settings) => void
  onReset: (keys: string[]) => void
  fileManager: string
}

export function SettingsForm({
  schema,
  settings,
  onChange,
  onChangeMany,
  onReset,
  fileManager,
}: Props) {
  const [open, setOpen] = useState<Set<string>>(() => new Set(OPEN_BY_DEFAULT))
  const [advanced, setAdvanced] = useState(false)

  const toggle = (key: string) =>
    setOpen((all) => {
      const next = new Set(all)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })

  return (
    <form
      className="settings"
      onSubmit={(e) => e.preventDefault()}
      aria-label="Conversion settings"
    >
      {schema.groups.map((group) => {
        const fields = schema.fields.filter(
          (f) => f.group === group.key && (advanced || !f.advanced) && isShown(f, settings),
        )
        if (!fields.length) return null
        const isOpen = open.has(group.key)
        const panelId = `group-${group.key}`
        return (
          <section key={group.key} className={`settings-group${isOpen ? ' is-open' : ''}`}>
            <header className="settings-group-header">
              <button
                type="button"
                className="settings-group-toggle"
                aria-expanded={isOpen}
                aria-controls={panelId}
                onClick={() => toggle(group.key)}
              >
                <ChevronDown size={16} className="chevron" aria-hidden />
                {group.label}
              </button>
              {isOpen && (
                <button
                  type="button"
                  className="icon-button icon-button--quiet"
                  title={`Reset ${group.label.toLowerCase()} to the defaults`}
                  onClick={() => onReset(fields.map((f) => f.key))}
                >
                  <RotateCcw size={14} />
                </button>
              )}
            </header>
            {isOpen && (
              <div id={panelId} className="settings-group-body">
                {group.help && <p className="group-help">{group.help}</p>}
                {fields.map((field) => (
                  <FieldControl
                    key={field.key}
                    field={field}
                    schema={schema}
                    settings={settings}
                    onChange={onChange}
                    onChangeMany={onChangeMany}
                    fileManager={fileManager}
                  />
                ))}
              </div>
            )}
          </section>
        )
      })}
      <label className="switch settings-advanced">
        <input type="checkbox" checked={advanced} onChange={(e) => setAdvanced(e.target.checked)} />
        <span className="switch-track" aria-hidden />
        <span>Show advanced settings</span>
      </label>
    </form>
  )
}

interface FieldProps {
  field: Field
  schema: Schema
  settings: Settings
  onChange: (key: string, value: unknown) => void
  onChangeMany: (values: Settings) => void
  fileManager: string
}

function FieldControl({
  field,
  schema,
  settings,
  onChange,
  onChangeMany,
  fileManager,
}: FieldProps) {
  const id = useId()
  const value = settings[field.key]
  const help = field.help ? (
    <p className="field-help" id={`${id}-help`}>
      {field.help}
    </p>
  ) : null
  const describedBy = field.help ? `${id}-help` : undefined

  switch (field.type) {
    case 'bool':
      return (
        <div className="field field--bool">
          <label className="switch">
            <input
              type="checkbox"
              checked={!!value}
              onChange={(e) => onChange(field.key, e.target.checked)}
              aria-describedby={describedBy}
            />
            <span className="switch-track" aria-hidden />
            <span className="field-label">{field.label}</span>
          </label>
          {help}
        </div>
      )

    case 'int':
    case 'float':
      return field.slider || (field.min != null && field.max != null) ? (
        <RangeField
          field={field}
          value={value}
          onChange={onChange}
          id={id}
          help={help}
          describedBy={describedBy}
        />
      ) : (
        <div className="field">
          <label className="field-label" htmlFor={id}>
            {field.label}
            {field.unit && <span className="field-unit">{field.unit}</span>}
          </label>
          <input
            id={id}
            className="input"
            type="text"
            inputMode="decimal"
            value={value == null ? '' : String(value)}
            placeholder={field.placeholder}
            onChange={(e) => onChange(field.key, e.target.value)}
            aria-describedby={describedBy}
          />
          {help}
        </div>
      )

    case 'choice':
    case 'block': {
      const targets = field.key === 'target' ? new Map(schema.targets.map((t) => [t.id, t])) : null
      return (
        <div className="field">
          <label className="field-label" htmlFor={id}>
            {field.label}
          </label>
          <select
            id={id}
            className="input"
            value={String(value ?? field.default ?? '')}
            onChange={(e) => onChange(field.key, e.target.value)}
            aria-describedby={describedBy}
          >
            {(field.choices ?? []).map((c) => {
              const target = targets?.get(c.value)
              return (
                <option key={c.value} value={c.value}>
                  {c.label}
                  {target ? ` — ${target.blocks} blocks` : ''}
                  {c.value === field.default ? ' (default)' : ''}
                </option>
              )
            })}
          </select>
          {help}
        </div>
      )
    }

    case 'direction':
      return <DirectionField field={field} value={value} onChange={onChange} help={help} />

    case 'folder':
      return (
        <FolderField
          field={field}
          settings={settings}
          onChange={onChange}
          onChangeMany={onChangeMany}
          fileManager={fileManager}
          help={help}
        />
      )

    default:
      return (
        <div className="field">
          <label className="field-label" htmlFor={id}>
            {field.label}
          </label>
          <input
            id={id}
            className="input"
            type="text"
            value={String(value ?? '')}
            placeholder={field.placeholder}
            onChange={(e) => onChange(field.key, e.target.value)}
            aria-describedby={describedBy}
          />
          {help}
        </div>
      )
  }
}

interface RangeProps {
  field: Field
  value: unknown
  onChange: (key: string, value: unknown) => void
  id: string
  help: ReactNode
  describedBy?: string
}

/** A slider over the field's usual range, and a box for exact values up to
 *  its full range. */
function RangeField({ field, value, onChange, id, help, describedBy }: RangeProps) {
  const [lo, hi] = field.slider ?? [field.min ?? 0, field.max ?? 1]
  const step = field.step ?? (field.type === 'int' ? 1 : 0.01)
  const shown = formatNumber(value, field)
  const n = Number(value)
  return (
    <div className="field field--range">
      <div className="field-row">
        <label className="field-label" htmlFor={id}>
          {field.label}
        </label>
        <span className="field-number">
          <input
            className="input input--number"
            type="number"
            aria-label={`${field.label} value`}
            min={field.min}
            max={field.max}
            step={step}
            value={shown}
            onChange={(e) =>
              onChange(field.key, e.target.value === '' ? '' : Number(e.target.value))
            }
          />
          {field.unit && <span className="field-unit">{field.unit}</span>}
        </span>
      </div>
      <input
        id={id}
        className="range"
        type="range"
        min={lo}
        max={hi}
        step={step}
        value={Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : lo}
        onChange={(e) => onChange(field.key, Number(e.target.value))}
        aria-describedby={describedBy}
      />
      {help}
    </div>
  )
}
