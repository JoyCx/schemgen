import { downloadUrl } from '../api.js'

export default function ResultPanel({ result, onReset, onReconvert, needsUpdate, reconverting }) {
  return (
    <div className={`panel result-panel${reconverting ? ' result-reconverting' : ''}`}>
      <div className="result-icon">{reconverting ? '⏳' : '✓'}</div>
      <h3>{reconverting ? 'Converting…' : 'Conversion Complete'}</h3>
      <p className="result-file">{result.download_name}</p>
      {needsUpdate && !reconverting && (
        <p className="result-note">Settings changed — download reflects the previous conversion.</p>
      )}
      {reconverting && (
        <p className="result-note">New conversion in progress. Previous download still available below.</p>
      )}

      <div className="result-actions">
        <a
          href={downloadUrl(result.job_id)}
          className="btn-download"
          download={result.download_name}
        >
          Download .litematic
        </a>
        {onReconvert && !reconverting && (
          <button className="btn-reset" onClick={onReconvert}>
            Re-convert with current settings
          </button>
        )}
        {!reconverting && (
          <button className="btn-reset" onClick={onReset}>
            Convert Another
          </button>
        )}
      </div>
    </div>
  )
}