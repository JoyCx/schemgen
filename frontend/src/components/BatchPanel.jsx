import { downloadUrl } from '../api.js'

const STATUS_LABEL = {
  queued: 'Queued',
  running: 'Converting',
  done: 'Done',
  error: 'Failed',
}

export default function BatchPanel({
  jobs, running, threads, error, outputDir, fileManager,
  onRerun, onClear, onSaveAll, onReveal, onOpenFolder,
}) {
  const total = jobs.length
  const done = jobs.filter((j) => j.status === 'done').length
  const failed = jobs.filter((j) => j.status === 'error').length
  const saved = jobs.filter((j) => j.saved_path).length

  return (
    <div className="panel batch-panel">
      <div className="batch-header">
        <h3>Batch Conversion</h3>
        <span className="batch-summary">
          {total} files · {done} done · {failed} failed · {threads} threads
          {outputDir ? ` · ${saved} saved to folder` : ''}
        </span>
      </div>

      {error && <p className="batch-error">{error}</p>}

      <div className="batch-actions">
        <button className="btn-reset" onClick={onRerun} disabled={running}>
          {running ? 'Converting…' : 'Re-run with current settings'}
        </button>
        {outputDir && (
          <>
            <button className="btn-reset" onClick={onSaveAll} disabled={running || done === 0}>
              Save {done} to folder
            </button>
            <button className="btn-reset" onClick={onOpenFolder}>
              📂 Open folder
            </button>
          </>
        )}
        <button className="btn-reset" onClick={onClear} disabled={running}>
          Clear
        </button>
      </div>

      {jobs.length === 0 ? (
        <p className="batch-empty">Uploading files…</p>
      ) : (
        <ul className="batch-list">
          {jobs.map((j) => (
            <li key={j.key} className={`batch-item batch-item--${j.status}`}>
              <div className="batch-item-top">
                <span className="batch-item-name" title={j.filename}>{j.filename}</span>
                <span className="batch-item-status">{STATUS_LABEL[j.status] || j.status}</span>
              </div>
              <div className="batch-item-track">
                <div
                  className={`batch-item-fill batch-item-fill--${j.status}`}
                  style={{ width: `${Math.round(j.progress)}%` }}
                />
              </div>
              <div className="batch-item-bottom">
                <span className="batch-item-msg" title={j.saved_path || j.message}>
                  {j.saved_path ? `📁 ${j.saved_path}` : j.save_error ? `⚠️ ${j.save_error}` : j.message}
                </span>
                {j.status === 'done' && (
                  <span className="batch-item-actions">
                    <button
                      type="button"
                      className="batch-item-download"
                      onClick={() => onReveal(j.job_id)}
                      title={`Show in ${fileManager}`}
                    >
                      📂 Show
                    </button>
                    <a
                      className="batch-item-download"
                      href={downloadUrl(j.job_id)}
                      download={j.download_name}
                    >
                      ⬇️
                    </a>
                  </span>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
