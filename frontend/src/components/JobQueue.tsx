// The job list: one row per model, single file or a hundred. Convert runs
// every model not yet converted with the current settings.

import { useRef } from 'react'
import {
  Ban,
  CircleCheck,
  CircleX,
  Download,
  FileBox,
  FolderInput,
  FolderOpen,
  Loader2,
  Play,
  Plus,
  Trash2,
} from 'lucide-react'
import { downloadUrl } from '../api'
import { isActive, type Conversion, type QueueItem } from '../hooks/useConversion'

const STATUS: Record<QueueItem['status'], string> = {
  ready: 'Ready',
  uploading: 'Uploading',
  queued: 'Queued',
  running: 'Converting',
  done: 'Done',
  error: 'Failed',
  cancelled: 'Cancelled',
}

interface Props {
  conversion: Conversion
  canConvert: boolean
  /** The folder schematics are copied into, if one is set. */
  outputDir: string
  fileManager: string
}

const mb = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(bytes < 1024 * 1024 ? 2 : 1)} MB`

export function JobQueue({ conversion, canConvert, outputDir, fileManager }: Props) {
  const input = useRef<HTMLInputElement>(null)
  const { items, selected, pending } = conversion
  const running = items.some((it) => isActive(it.status))
  const done = items.filter((it) => it.status === 'done').length

  return (
    <section className="queue" aria-label="Models">
      <div className="queue-bar">
        <button type="button" className="button" onClick={() => input.current?.click()}>
          <Plus size={16} aria-hidden /> Add models
        </button>
        <input
          ref={input}
          type="file"
          accept=".glb,.gltf"
          multiple
          hidden
          onChange={(e) => {
            conversion.addFiles([...(e.target.files ?? [])])
            e.target.value = ''
          }}
        />
        <span className="queue-summary">
          {items.length === 0
            ? 'Drop .glb / .gltf files or a folder anywhere on the page.'
            : `${items.length} model${items.length === 1 ? '' : 's'} · ${done} done`}
        </span>
        <div className="queue-actions">
          {outputDir && done > 0 && (
            <button type="button" className="button button--quiet" onClick={conversion.saveAll}>
              <FolderInput size={16} aria-hidden /> Save all to folder
            </button>
          )}
          {items.length > 0 && (
            <button type="button" className="button button--quiet" onClick={conversion.clear}>
              <Trash2 size={16} aria-hidden /> Clear
            </button>
          )}
          <button
            type="button"
            className="button button--primary"
            disabled={!canConvert || pending.length === 0}
            onClick={() => conversion.convert()}
            title="Ctrl+Enter"
          >
            {running ? (
              <Loader2 size={16} className="spin" aria-hidden />
            ) : (
              <Play size={16} aria-hidden />
            )}
            {pending.length > 1 ? `Convert ${pending.length}` : 'Convert'}
            <kbd>Ctrl+Enter</kbd>
          </button>
        </div>
      </div>

      {items.length > 0 && (
        <ul className="queue-list">
          {items.map((item) => (
            <QueueRow
              key={item.key}
              item={item}
              selected={item.key === selected?.key}
              stale={conversion.isStale(item)}
              outputDir={outputDir}
              fileManager={fileManager}
              conversion={conversion}
            />
          ))}
        </ul>
      )}
    </section>
  )
}

interface RowProps {
  item: QueueItem
  selected: boolean
  stale: boolean
  outputDir: string
  fileManager: string
  conversion: Conversion
}

function QueueRow({ item, selected, stale, outputDir, fileManager, conversion }: RowProps) {
  const active = isActive(item.status)
  const view = item.view
  const Icon =
    item.status === 'done'
      ? CircleCheck
      : item.status === 'error'
        ? CircleX
        : item.status === 'cancelled'
          ? Ban
          : active
            ? Loader2
            : FileBox
  const detail =
    item.status === 'done'
      ? view?.saved_path
        ? `Saved to ${view.saved_path}`
        : view?.result
          ? `${view.result.blocks.toLocaleString()} blocks · ${view.result.dims.join(' × ')} · Minecraft ${view.result.target}`
          : item.message
      : item.message

  return (
    <li
      className={`queue-item queue-item--${item.status}${selected ? ' is-selected' : ''}`}
      onClick={() => conversion.select(item.key)}
      aria-current={selected || undefined}
    >
      <Icon size={18} className={`queue-icon${active ? ' spin' : ''}`} aria-hidden />
      <div className="queue-main">
        <div className="queue-title">
          <button type="button" className="queue-name" onClick={() => conversion.select(item.key)}>
            {item.file.name}
          </button>
          <span className="queue-size">{mb(item.file.size)}</span>
          <span className={`pill pill--${item.status}`}>
            {STATUS[item.status]}
            {item.status === 'running' ? ` ${Math.round(item.progress)}%` : ''}
          </span>
          {stale && <span className="pill pill--stale">Settings changed</span>}
        </div>
        {active && (
          <div className="progress" aria-hidden>
            <div className="progress-fill" style={{ width: `${item.progress}%` }} />
          </div>
        )}
        <p
          className={`queue-detail${item.status === 'error' ? ' text-danger' : ''}`}
          title={detail}
        >
          {detail}
        </p>
      </div>
      <div className="queue-row-actions" onClick={(e) => e.stopPropagation()}>
        {item.status === 'done' && item.jobId && (
          <>
            <button
              type="button"
              className="icon-button"
              title={`Show in ${fileManager}`}
              aria-label={`Show ${item.file.name} in ${fileManager}`}
              onClick={() => conversion.reveal(item.key)}
            >
              <FolderOpen size={16} />
            </button>
            {outputDir && !view?.saved_path && (
              <button
                type="button"
                className="icon-button"
                title={`Save to ${outputDir}`}
                aria-label={`Save ${item.file.name} to the folder`}
                onClick={() => conversion.save(item.key)}
              >
                <FolderInput size={16} />
              </button>
            )}
            <a
              className="icon-button"
              href={downloadUrl(item.jobId)}
              download={view?.download_name}
              title="Download"
              aria-label={`Download ${view?.download_name ?? item.file.name}`}
            >
              <Download size={16} />
            </a>
          </>
        )}
        {active ? (
          <button
            type="button"
            className="icon-button"
            title="Cancel"
            aria-label={`Cancel ${item.file.name}`}
            onClick={() => conversion.cancel(item.key)}
          >
            <Ban size={16} />
          </button>
        ) : (
          <button
            type="button"
            className="icon-button"
            title="Remove from the list"
            aria-label={`Remove ${item.file.name}`}
            onClick={() => conversion.remove(item.key)}
          >
            <Trash2 size={16} />
          </button>
        )}
      </div>
    </li>
  )
}
