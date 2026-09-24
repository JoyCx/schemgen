// Where finished schematics are copied: a path typed or picked from the
// launcher folders the server found, checked by the server as it is typed.

import { useEffect, useId, useState, type ReactNode } from 'react'
import { Check, FolderOpen, TriangleAlert } from 'lucide-react'
import { checkOutputDir, fetchOutputDirSuggestions, revealFolder } from '../../api'
import { AUTO_SAVE, type Settings } from '../../settings'
import type { DirCheck, Field, Suggestion } from '../../types'

interface Props {
  field: Field
  settings: Settings
  onChange: (key: string, value: unknown) => void
  onChangeMany: (values: Settings) => void
  fileManager: string
  help: ReactNode
}

/** "CurseForge · All the Mods · 1.20.1" for a folder inside a launcher instance. */
function instanceLabel(s: Suggestion): string {
  const i = s.instance
  if (!i) return s.path
  return [i.launcher, i.name, i.mc_version].filter(Boolean).join(' · ')
}

export function FolderField({ field, settings, onChange, onChangeMany, fileManager, help }: Props) {
  const id = useId()
  const enabled = !!settings[AUTO_SAVE]
  const path = String(settings[field.key] ?? '')
  const [suggestions, setSuggestions] = useState<Suggestion[]>([])
  const [check, setCheck] = useState<{ path: string; result: DirCheck } | null>(null)

  useEffect(() => {
    let stopped = false
    fetchOutputDirSuggestions()
      .then((s) => !stopped && setSuggestions(s))
      .catch(() => {})
    return () => {
      stopped = true
    }
  }, [])

  // Checked shortly after typing stops.
  useEffect(() => {
    const trimmed = path.trim()
    if (!enabled || !trimmed) return
    let stopped = false
    const timer = setTimeout(async () => {
      try {
        const result = await checkOutputDir(trimmed)
        if (!stopped) setCheck({ path: trimmed, result })
      } catch (e) {
        if (!stopped) {
          setCheck({
            path: trimmed,
            result: { ok: false, error: e instanceof Error ? e.message : 'Check failed' },
          })
        }
      }
    }, 500)
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  }, [enabled, path])

  const current = check && check.path === path.trim() ? check.result : null

  return (
    <div className="field field--folder">
      <label className="switch">
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => onChange(AUTO_SAVE, e.target.checked)}
        />
        <span className="switch-track" aria-hidden />
        <span className="field-label">{field.label}</span>
      </label>
      {help}
      {enabled && (
        <>
          <div className="folder-row">
            <input
              id={id}
              className="input"
              type="text"
              value={path}
              placeholder={suggestions[0]?.path || 'Full path to a schematics folder'}
              spellCheck={false}
              onChange={(e) => onChange(field.key, e.target.value)}
              aria-label="Folder path"
            />
            <button
              type="button"
              className="icon-button"
              disabled={!current?.ok || !current.exists}
              title={`Open in ${fileManager}`}
              onClick={() => revealFolder(path.trim()).catch(() => {})}
            >
              <FolderOpen size={16} />
            </button>
          </div>
          <p className={`folder-status${current && !current.ok ? ' text-danger' : ''}`}>
            {!path.trim() ? (
              'An absolute path; ~ and %APPDATA% expand. Created if missing.'
            ) : !current ? (
              'Checking…'
            ) : current.ok ? (
              <>
                <Check size={14} aria-hidden />{' '}
                {current.exists ? `Saving to ${current.path}` : `${current.path} will be created`}
              </>
            ) : (
              <>
                <TriangleAlert size={14} aria-hidden /> {current.error}
              </>
            )}
          </p>
          {suggestions.length > 0 && (
            <div className="chips" aria-label="Folders found on this computer">
              {suggestions.slice(0, 8).map((s) => (
                <button
                  key={s.path}
                  type="button"
                  className={`chip${s.exists ? '' : ' chip--missing'}${s.path === path ? ' is-on' : ''}`}
                  title={
                    (s.exists ? s.path : `${s.path} (will be created)`) +
                    (s.instance?.target
                      ? `\nAlso sets the target to Minecraft ${s.instance.target}`
                      : '')
                  }
                  onClick={() =>
                    onChangeMany({
                      [field.key]: s.path,
                      // A folder inside an instance knows its game version.
                      ...(s.instance?.target ? { target: s.instance.target } : {}),
                    })
                  }
                >
                  {instanceLabel(s)}
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  )
}
