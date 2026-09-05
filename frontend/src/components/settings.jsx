import { useEffect, useState } from 'react'
import { checkOutputDir, fetchOutputDirSuggestions, revealFolder } from '../api.js'

export default function Settings({ settings, onChange, savedPath = '' }) {
  const update = (key, value) => onChange({ ...settings, [key]: value })

  // Browsers cannot hand a real folder path to a page, so the folder is typed
  // (or picked from the server's suggestions) and verified by the backend.
  const [suggestions, setSuggestions] = useState([])
  const [dirState, setDirState] = useState(null) // { ok, path } | { ok: false, error }
  const [checking, setChecking] = useState(false)

  useEffect(() => { fetchOutputDirSuggestions().then(setSuggestions).catch(() => {}) }, [])

  // Verify the folder shortly after typing stops, and again once a schematic
  // has been written — a folder reported as "will be created" exists by then.
  useEffect(() => {
    const path = settings.output_dir.trim()
    if (!settings.auto_save || !path) { setDirState(null); return }
    let cancelled = false
    setChecking(true)
    const t = setTimeout(async () => {
      try {
        const res = await checkOutputDir(path)
        if (!cancelled) setDirState(res)
      } catch (e) {
        if (!cancelled) setDirState({ ok: false, error: e.message || 'Check failed' })
      } finally {
        if (!cancelled) setChecking(false)
      }
    }, 600)
    return () => { cancelled = true; clearTimeout(t); setChecking(false) }
  }, [settings.output_dir, settings.auto_save, savedPath])

  return (
    <div className="panel settings-panel">
      <h3>Conversion Settings</h3>
      <div className="settings-grid">
        <label>
          <span>Max Size (blocks)</span>
          <input
            type="number"
            value={settings.max_size}
            onChange={(e) => update('max_size', e.target.value)}
            min="8" max="512"
          />
        </label>

        <label>
          <span>Voxel Size (optional)</span>
          <input
            type="text"
            value={settings.voxel_size}
            onChange={(e) => update('voxel_size', e.target.value)}
            placeholder="Auto"
          />
        </label>

        <label>
          <span>RAM Limit (GB)</span>
          <input
            type="number"
            value={settings.ram_limit}
            onChange={(e) => update('ram_limit', e.target.value)}
            min="0.5" max="32" step="0.5"
          />
        </label>

        <label>
          <span>Threads (batch)</span>
          <input
            type="number"
            value={settings.threads}
            onChange={(e) => update('threads', e.target.value)}
            min="1" max="32" step="1"
          />
        </label>

        <label>
          <span>Schematic Name</span>
          <input
            type="text"
            value={settings.schematic_name}
            onChange={(e) => update('schematic_name', e.target.value)}
            placeholder="Use file name"
          />
        </label>
      </div>

      <div className="output-dir">
        <label className="toggle">
          <input
            type="checkbox"
            checked={settings.auto_save}
            onChange={(e) => update('auto_save', e.target.checked)}
          />
          <span>Save .litematic straight into a folder</span>
        </label>

        <div className="output-dir-row">
          <input
            className="output-dir-input"
            type="text"
            value={settings.output_dir}
            onChange={(e) => update('output_dir', e.target.value)}
            disabled={!settings.auto_save}
            placeholder={suggestions[0]?.path || 'Full path to your schematics folder'}
            spellCheck="false"
          />
          <button
            type="button"
            className="btn-reset output-dir-open"
            disabled={!settings.auto_save || !dirState?.ok}
            onClick={() => revealFolder(settings.output_dir.trim()).catch(() => {})}
            title="Open this folder"
          >
            📂 Open
          </button>
        </div>

        {settings.auto_save && suggestions.length > 0 && (
          <div className="output-dir-suggestions">
            {suggestions.map((sug) => (
              <button
                key={sug.path}
                type="button"
                className={`output-dir-chip${sug.exists ? ' output-dir-chip--exists' : ''}`}
                title={sug.exists ? sug.path : `${sug.path} (will be created)`}
                onClick={() => update('output_dir', sug.path)}
              >
                {sug.path}
              </button>
            ))}
          </div>
        )}

        {settings.auto_save && (
          <p className={`output-dir-status${dirState && !dirState.ok ? ' output-dir-status--bad' : ''}`}>
            {checking
              ? 'Checking folder…'
              : dirState?.ok
                ? dirState.exists
                  ? `✓ Saving to ${dirState.path}`
                  : `✓ ${dirState.path} will be created on the first conversion`
                : dirState?.error
                  ? `⚠️ ${dirState.error}`
                  : 'Enter an absolute path — %APPDATA% and ~ are expanded. Created if missing.'}
          </p>
        )}
      </div>

      <div className="settings-toggles">
        <label className="toggle">
          <input
            type="checkbox"
            checked={settings.dither}
            onChange={(e) => update('dither', e.target.checked)}
          />
          <span>Bayer Dithering</span>
        </label>

        <label className="toggle">
          <input
            type="checkbox"
            checked={settings.color_sampling}
            onChange={(e) => update('color_sampling', e.target.checked)}
          />
          <span>Color Sampling</span>
        </label>

        <label className="toggle">
          <span>Default Block:</span>
          <select
            value={settings.no_color_block}
            onChange={(e) => update('no_color_block', e.target.value)}
          >
            <option value="white">White Concrete</option>
            <option value="netherrack">Netherrack</option>
          </select>
        </label>
      </div>
    </div>
  )
}
