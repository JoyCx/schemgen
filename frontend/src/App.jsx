import { useState, useCallback, useEffect, useRef } from 'react'
import DropZone from './components/DropZone.jsx'
import Settings from './components/Settings.jsx'
import PaletteGrid from './components/PaletteGrid.jsx'
import ModelPreview from './components/ModelPreview.jsx'
import MinecraftPreview from './components/MinecraftPreview.jsx'
import BatchPanel from './components/BatchPanel.jsx'
import {
  startJobs,
  watchJob,
  fetchPalette,
  fetchSchema,
  fetchHealth,
  fetchPreview,
  downloadUrl,
  saveToFolder,
  revealJob,
  revealFolder,
  fetchSystemInfo,
} from './api.js'
import { LIGHT_DEFAULTS } from './lighting.js'
import { SETTINGS_DEFAULTS } from './settingsDefaults.js'
import './App.css'

const OUTPUT_PREFS_KEY = 'schemgen2.output'

// Remember the output folder, target version and file format between
// sessions — they are machine-level choices, not something to redo per visit.
function loadOutputPrefs() {
  const empty = { output_dir: '', auto_save: false }
  try {
    const raw = localStorage.getItem(OUTPUT_PREFS_KEY)
    if (!raw) return empty
    const p = JSON.parse(raw)
    return {
      output_dir: p.output_dir || '',
      auto_save: !!p.auto_save,
      ...(p.target ? { target: p.target } : {}),
      ...(p.format ? { format: p.format } : {}),
    }
  } catch {
    return empty
  }
} // The lighting knobs, in the order the conversion key and the preview both
// read them. Kept in one place so adding a knob cannot update one and not the
// other — which would leave the preview showing a stale conversion.
const LIGHT_KEYS = Object.keys(LIGHT_DEFAULTS)

// Settings that actually change the schematic. Editing the output folder must
// not trigger a re-conversion — the finished file is copied instead.
function conversionKey(s) {
  return JSON.stringify([
    s.max_size,
    s.voxel_size,
    s.ram_limit,
    s.dither,
    s.color_sampling,
    s.brightness,
    s.contrast,
    s.saturation,
    s.no_color_block,
    s.schematic_name,
    s.target,
    s.format,
    ...LIGHT_KEYS.map((k) => s[k]),
  ])
}
export default function App() {
  const [files, setFiles] = useState([])
  const [status, setStatus] = useState('idle') // idle | uploading | running | done | error
  const [progress, setProgress] = useState(0)
  const [message, setMessage] = useState('')
  const [result, setResult] = useState(null) // { job_id, download_name }
  const [error, setError] = useState('')
  const [palette, setPalette] = useState(null)
  const [litematicPreview, setLitematicPreview] = useState(null)
  const [previewLoading, setPreviewLoading] = useState(false)
  const [previewError, setPreviewError] = useState('')
  const [batchJobs, setBatchJobs] = useState([])
  const cancelPreviewRef = useRef(null)
  const [fileManager, setFileManager] = useState('Explorer')
  const [revealError, setRevealError] = useState('')

  const [settings, setSettings] = useState(() => ({
    ...SETTINGS_DEFAULTS,
    ...loadOutputPrefs(),
  }))
  const [version, setVersion] = useState('')
  const [schema, setSchema] = useState(null) // Persist the folder choice.
  useEffect(() => {
    try {
      localStorage.setItem(
        OUTPUT_PREFS_KEY,
        JSON.stringify({
          output_dir: settings.output_dir,
          auto_save: settings.auto_save,
          target: settings.target,
          format: settings.format,
        }),
      )
    } catch {
      /* private mode — not worth surfacing */
    }
  }, [settings.output_dir, settings.auto_save, settings.target, settings.format]) // Single-file mode only when exactly one file is present.
  const file = files.length === 1 ? files[0] : null
  const batchMode = files.length > 1

  // Always-current refs so conversion closures never go stale.
  const fileRef = useRef(file)
  const settingsRef = useRef(settings)
  const statusRef = useRef(status)
  const pendingRef = useRef(false)
  const runConvertRef = useRef(null)
  // Stops following the single-file job, and each batch job by id.
  const stopWatchRef = useRef(null)
  const batchWatchesRef = useRef(new Map())
  // `${job_id}|${folder as typed}` for the copy we know already happened, so the
  // re-save effect below does not repeat what the conversion just did.
  const savedForRef = useRef('')
  fileRef.current = file
  settingsRef.current = settings
  statusRef.current = status

  // The palette shown and matched against depends on the target version.
  useEffect(() => {
    fetchPalette(settings.target)
      .then(setPalette)
      .catch(() => {})
  }, [settings.target])
  useEffect(() => {
    fetchSchema()
      .then(setSchema)
      .catch(() => {})
  }, []) // The version shown is the server's — Cargo.toml is the one place it is set.
  useEffect(() => {
    fetchHealth()
      .then((h) => setVersion(h.version || ''))
      .catch(() => {})
  }, [])
  useEffect(() => {
    fetchSystemInfo()
      .then((i) => i?.file_manager && setFileManager(i.file_manager))
      .catch(() => {})
  }, [])

  // Show the finished schematic in the OS file manager instead of downloading a
  // second copy of a file that is already on this machine.
  const reveal = useCallback(
    async (jobId) => {
      setRevealError('')
      try {
        await revealJob(jobId)
      } catch (e) {
        setRevealError(e.message || `Could not open ${fileManager}`)
      }
    },
    [fileManager],
  )

  const openFolder = useCallback(async () => {
    const dir = settingsRef.current.output_dir.trim()
    if (!dir) return
    setRevealError('')
    try {
      await revealFolder(dir)
    } catch (e) {
      setRevealError(e.message || `Could not open ${fileManager}`)
    }
  }, [fileManager])

  // ---- Single-file conversion ----------------------------------------------
  const runConvert = useCallback(async () => {
    const f = fileRef.current
    const s = settingsRef.current
    if (!f) return

    // Already converting — remember we need another pass with latest settings.
    if (statusRef.current === 'uploading' || statusRef.current === 'running') {
      pendingRef.current = true
      return
    }

    pendingRef.current = false
    setStatus('uploading')
    setError('')
    setProgress(0)
    setMessage('Uploading…')

    const requestedDir = s.auto_save ? s.output_dir.trim() : ''

    try {
      const { jobs } = await startJobs([f], s)
      const job_id = jobs[0].job_id

      setStatus('running')
      stopWatchRef.current?.()
      stopWatchRef.current = watchJob(job_id, (data) => {
        setProgress(data.progress)
        setMessage(data.message)
        if (data.status === 'done') {
          setStatus('done')
          if (data.saved_path) savedForRef.current = `${job_id}|${requestedDir}`
          setResult({
            job_id,
            download_name: data.download_name,
            saved_path: data.saved_path || '',
            save_error: data.save_error || '',
          })
          if (pendingRef.current) {
            pendingRef.current = false
            setTimeout(() => runConvertRef.current?.(), 50)
          }
        } else if (data.status === 'error' || data.status === 'cancelled') {
          setStatus('error')
          setError(data.error || data.message || 'Conversion failed')
        }
      })
    } catch (e) {
      setStatus('error')
      setError(e.message || 'Upload failed')
    }
  }, []) // stable — reads live values via refs

  runConvertRef.current = runConvert

  // Auto-convert: immediately when a single file lands.
  useEffect(() => {
    if (!file) {
      setResult(null)
      setStatus('idle')
      setError('')
      return
    }
    runConvert()
  }, [file]) // eslint-disable-line react-hooks/exhaustive-deps

  // Auto-reconvert: 1.5 s after settings settle (skip if already running).
  useEffect(() => {
    if (!file) return
    const timer = setTimeout(runConvert, 1500)
    return () => clearTimeout(timer)
  }, [conversionKey(settings)]) // eslint-disable-line react-hooks/exhaustive-deps

  // ---- Re-save on folder change (single) ------------------------------------
  // The schematic is already built; pointing at a different folder only needs a
  // copy, so this never re-runs the conversion.
  useEffect(() => {
    const dir = settings.output_dir.trim()
    if (!settings.auto_save || !dir || !result?.job_id) return

    // The typed folder never string-matches the expanded path the server
    // reports, so compare against what we know was already written instead.
    const key = `${result.job_id}|${dir}`
    if (savedForRef.current === key) return

    let cancelled = false
    const t = setTimeout(async () => {
      try {
        const { saved_path } = await saveToFolder(result.job_id, dir)
        savedForRef.current = key
        if (!cancelled)
          setResult((r) =>
            r && r.job_id === result.job_id ? { ...r, saved_path, save_error: '' } : r,
          )
      } catch (e) {
        if (!cancelled)
          setResult((r) =>
            r && r.job_id === result.job_id ? { ...r, save_error: e.message || 'Save failed' } : r,
          )
      }
    }, 800)
    return () => {
      cancelled = true
      clearTimeout(t)
    }
  }, [settings.output_dir, settings.auto_save, result?.job_id, result?.saved_path])

  // ---- Live Minecraft preview (single only) ---------------------------------
  useEffect(() => {
    if (!file) {
      setLitematicPreview(null)
      cancelPreviewRef.current = null
      return
    }
    const controller = new AbortController()
    let timedOut = false
    let userCancelled = false

    cancelPreviewRef.current = () => {
      userCancelled = true
      controller.abort()
      setPreviewLoading(false)
      setPreviewError('')
    }

    const timer = setTimeout(async () => {
      setPreviewLoading(true)
      setPreviewError('')
      const watchdog = setTimeout(
        () => {
          timedOut = true
          controller.abort()
        },
        10 * 60 * 1000,
      )
      try {
        const data = await fetchPreview(file, settings, controller.signal)
        clearTimeout(watchdog)
        if (!controller.signal.aborted) setLitematicPreview(data)
      } catch (e) {
        clearTimeout(watchdog)
        if (timedOut) {
          setPreviewError(
            'Preview timed out — model is very large. Reduce Max Size or wait for the download to finish.',
          )
          setPreviewLoading(false)
        } else if (!userCancelled && !controller.signal.aborted) {
          setPreviewError(e.message || 'Preview failed')
          setPreviewLoading(false)
        }
      } finally {
        if (!timedOut && !userCancelled && !controller.signal.aborted) setPreviewLoading(false)
      }
    }, 450)

    return () => {
      clearTimeout(timer)
      controller.abort()
      cancelPreviewRef.current = null
    }
  }, [
    file,
    settings.max_size,
    settings.voxel_size,
    settings.ram_limit,
    settings.dither,
    settings.color_sampling,
    settings.brightness,
    settings.contrast,
    settings.saturation,
    settings.no_color_block,
    settings.target,
    ...LIGHT_KEYS.map((k) => settings[k]),
  ]) // ---- Batch conversion -----------------------------------------------------
  const stopBatchWatches = useCallback(() => {
    for (const stop of batchWatchesRef.current.values()) stop()
    batchWatchesRef.current.clear()
  }, [])

  const startBatch = useCallback(
    async (fileList) => {
      const s = settingsRef.current
      stopBatchWatches()
      try {
        const { jobs } = await startJobs(fileList, s)
        setBatchJobs(
          jobs.map((j) => ({
            key: j.job_id,
            job_id: j.job_id,
            filename: j.filename,
            status: 'queued',
            progress: 0,
            message: 'Queued…',
            download_name: '',
            saved_path: '',
            save_error: '',
          })),
        )
        setError('')
        for (const j of jobs) {
          const stop = watchJob(j.job_id, (u) => {
            setBatchJobs((prev) =>
              prev.map((b) =>
                b.job_id === u.id
                  ? {
                      ...b,
                      status: u.status,
                      progress: u.progress,
                      message: u.error || u.message,
                      download_name: u.download_name,
                      saved_path: u.saved_path || '',
                      save_error: u.save_error || '',
                    }
                  : b,
              ),
            )
          })
          batchWatchesRef.current.set(j.job_id, stop)
        }
      } catch (e) {
        setError(e.message || 'Batch upload failed')
        setBatchJobs([])
      }
    },
    [stopBatchWatches],
  )

  // Stop following every job when the page goes away.
  useEffect(
    () => () => {
      stopWatchRef.current?.()
      stopBatchWatches()
    },
    [stopBatchWatches],
  )

  // Start/clear batch when the number of files crosses the multi-file boundary.
  useEffect(() => {
    if (files.length > 1) {
      setResult(null)
      setStatus('idle')
      startBatch(files)
    } else {
      stopBatchWatches()
      setBatchJobs([])
    }
  }, [files]) // eslint-disable-line react-hooks/exhaustive-deps

  // Copy every finished batch job into the chosen folder (no re-conversion).
  const saveBatchToFolder = useCallback(async () => {
    const dir = settingsRef.current.output_dir.trim()
    if (!dir) return
    const done = batchJobs.filter((j) => j.status === 'done')
    const results = await Promise.all(
      done.map(async (j) => {
        try {
          const { saved_path } = await saveToFolder(j.job_id, dir)
          return { key: j.key, saved_path, save_error: '' }
        } catch (e) {
          return { key: j.key, saved_path: '', save_error: e.message || 'Save failed' }
        }
      }),
    )
    setBatchJobs((prev) =>
      prev.map((j) => {
        const r = results.find((x) => x.key === j.key)
        return r ? { ...j, ...r } : j
      }),
    )
  }, [batchJobs])

  const handleFiles = useCallback((list) => {
    setFiles(list)
  }, [])
  const rerunBatch = useCallback(() => {
    if (files.length > 1) startBatch(files)
  }, [files, startBatch])
  const clearAll = useCallback(() => {
    stopBatchWatches()
    setFiles([])
    setBatchJobs([])
    setResult(null)
    setStatus('idle')
    setError('')
  }, [stopBatchWatches])

  const converting = status === 'uploading' || status === 'running'
  const batchRunning =
    batchJobs.length > 0 && batchJobs.some((j) => j.status === 'queued' || j.status === 'running')

  return (
    <div className="app">
      <header className="app-header">
        <h1>
          SchemGen<em>2</em>
        </h1>
        <p className="subtitle">GLB → Litematica · CIEDE2000 · Rust</p>
      </header>

      <main className="app-main">
        <DropZone files={files} onFiles={handleFiles} />

        {file && <ModelPreview file={file} settings={settings} onChange={setSettings} />}
        {file && (
          <MinecraftPreview
            preview={litematicPreview}
            palette={palette}
            loading={previewLoading}
            error={previewError}
            onCancel={cancelPreviewRef.current}
          />
        )}
        {files.length > 0 && (
          <Settings
            settings={settings}
            schema={schema}
            onChange={setSettings}
            savedPath={result?.saved_path || ''}
          />
        )}

        {batchMode && (
          <BatchPanel
            jobs={batchJobs}
            running={batchRunning}
            threads={settings.threads}
            error={error}
            outputDir={settings.auto_save ? settings.output_dir.trim() : ''}
            fileManager={fileManager}
            onRerun={rerunBatch}
            onClear={clearAll}
            onSaveAll={saveBatchToFolder}
            onReveal={reveal}
            onOpenFolder={openFolder}
          />
        )}

        {/* Persistent download bar (single-file mode only) */}
        {file && !batchMode && (
          <div className="download-bar">
            {converting && (
              <div className="download-progress">
                <span className="download-progress-label">
                  {status === 'uploading' ? 'Uploading…' : `Converting — ${Math.round(progress)}%`}
                </span>
                <div className="download-progress-track">
                  <div className="download-progress-fill" style={{ width: `${progress}%` }} />
                </div>
                <span className="download-progress-msg">{message}</span>
              </div>
            )}

            {status === 'error' && <p className="download-error">⚠️ {error}</p>}

            {status === 'done' && result?.saved_path && (
              <p className="download-saved">
                📁 Saved to <code>{result.saved_path}</code>
              </p>
            )}
            {status === 'done' && result?.save_error && (
              <p className="download-error">
                ⚠️ Could not save to your folder: {result.save_error}
              </p>
            )}
            {revealError && <p className="download-error">⚠️ {revealError}</p>}

            {/* The file is already on this machine, so revealing it is the
                primary action; downloading a second copy is the fallback. */}
            <button
              type="button"
              className={`btn-download${!result || converting ? ' btn-download--disabled' : ''}`}
              disabled={!result || converting}
              onClick={() => result && reveal(result.job_id)}
            >
              {converting
                ? 'Converting…'
                : result
                  ? `📂 Show ${result.download_name} in ${fileManager}`
                  : `📂 Show in ${fileManager}`}
            </button>

            <a
              href={result ? downloadUrl(result.job_id) : undefined}
              className={`btn-download-secondary${!result || converting ? ' btn-download-secondary--disabled' : ''}`}
              download={result?.download_name}
              aria-disabled={!result || converting}
              onClick={(e) => {
                if (!result || converting) e.preventDefault()
              }}
            >
              ⬇️ Download a copy instead
            </a>
          </div>
        )}

        {palette && <PaletteGrid palette={palette} />}
      </main>

      <footer className="app-footer">
        <span>SchemGen2{version ? ` v${version}` : ''}</span>
        <span>Rust + Actix-web · Vite React · CIEDE2000 · Batch</span>
      </footer>
    </div>
  )
}
