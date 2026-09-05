export default function ProgressPanel({ status, progress, message }) {
  const pct = Math.round(progress)

  return (
    <div className="panel progress-panel">
      <div className="progress-header">
        <span className="progress-status">
          {status === 'uploading' ? 'Uploading...' : `Converting — ${pct}%`}
        </span>
      </div>

      <div className="progress-bar-track">
        <div
          className="progress-bar-fill"
          style={{ width: `${pct}%` }}
        />
      </div>

      <p className="progress-message">{message || 'Starting...'}</p>

      {status === 'uploading' && (
        <div className="spinner" />
      )}
    </div>
  )
}
