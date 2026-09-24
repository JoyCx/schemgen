// The live block preview: the pipeline at preview resolution, rerun a moment
// after the settings that change the schematic stop changing.

import { useCallback, useEffect, useMemo, useState } from 'react'
import { fetchPreview } from '../api'
import { toApiSettings, type Settings } from '../settings'
import type { Preview, Schema } from '../types'

const DEBOUNCE_MS = 400

export interface PreviewState {
  preview: Preview | null
  loading: boolean
  error: string
  /** Stop building the current preview. */
  cancel: () => void
}

interface Result {
  request: string
  preview: Preview | null
  error: string
}

export function usePreview(
  file: File | null,
  settings: Settings | null,
  schema: Schema | null,
): PreviewState {
  // Everything the preview depends on, as one comparable string.
  const payload = useMemo(
    () => (settings && schema ? JSON.stringify(toApiSettings(settings, schema)) : null),
    [settings, schema],
  )
  const request = file && payload ? `${file.name}|${file.size}|${file.lastModified}|${payload}` : ''

  const [result, setResult] = useState<Result | null>(null)
  const [started, setStarted] = useState('')
  const [cancelled, setCancelled] = useState('')
  const [controller, setController] = useState<AbortController | null>(null)

  useEffect(() => {
    if (!file || !payload || !request) return
    const abort = new AbortController()
    const timer = setTimeout(async () => {
      setStarted(request)
      setController(abort)
      try {
        const preview = await fetchPreview(file, JSON.parse(payload), abort.signal)
        setResult({ request, preview, error: '' })
      } catch (e) {
        if (!abort.signal.aborted) {
          setResult({
            request,
            preview: null,
            error: e instanceof Error ? e.message : 'Preview failed',
          })
        }
      }
    }, DEBOUNCE_MS)
    return () => {
      clearTimeout(timer)
      abort.abort()
    }
  }, [file, payload, request])

  const cancel = useCallback(() => {
    controller?.abort()
    setCancelled(started)
  }, [controller, started])

  // The newest result for this file stays up while a newer one builds, so the
  // view does not blank out on every slider move.
  const fileKey = file ? `${file.name}|${file.size}|${file.lastModified}|` : null
  const current = result && fileKey && result.request.startsWith(fileKey) ? result : null
  const loading =
    !!request && started === request && result?.request !== request && cancelled !== request

  return {
    preview: current?.preview ?? null,
    loading,
    error: current?.request === request ? current.error : '',
    cancel,
  }
}
