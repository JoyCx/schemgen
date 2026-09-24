// Form state for the conversion settings, built from the server's schema.

import { useCallback, useEffect, useMemo, useState } from 'react'
import { defaultsFrom, loadRemembered, saveRemembered, type Settings } from '../settings'
import type { Schema } from '../types'

export interface SettingsState {
  /** `null` until the schema has loaded. */
  settings: Settings | null
  set: (key: string, value: unknown) => void
  setMany: (values: Settings) => void
  /** Put these keys (default: all) back to the server's defaults. */
  reset: (keys?: string[]) => void
}

export function useSettings(schema: Schema | null): SettingsState {
  // What the user changed, on top of the defaults and remembered choices —
  // so the form needs no effect to initialize once the schema arrives.
  const [changes, setChanges] = useState<Settings>({})

  const base = useMemo(
    () => (schema ? { ...defaultsFrom(schema), ...loadRemembered(schema) } : null),
    [schema],
  )
  const settings = useMemo(() => (base ? { ...base, ...changes } : null), [base, changes])

  const set = useCallback((key: string, value: unknown) => {
    setChanges((c) => ({ ...c, [key]: value }))
  }, [])

  const setMany = useCallback((values: Settings) => {
    setChanges((c) => ({ ...c, ...values }))
  }, [])

  const reset = useCallback(
    (keys?: string[]) => {
      if (!schema) return
      const defaults = defaultsFrom(schema)
      setChanges((c) => {
        const next = { ...c }
        for (const key of keys ?? Object.keys(defaults)) next[key] = defaults[key]
        return next
      })
    },
    [schema],
  )

  const outputDir = settings?.output_dir
  const autoSave = settings?.auto_save
  const target = settings?.target
  const format = settings?.format
  useEffect(() => {
    if (settings) saveRemembered(settings)
    // Only the remembered keys matter; other edits need no write.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [outputDir, autoSave, target, format])

  return { settings, set, setMany, reset }
}
