import { useCallback, useRef, useState } from 'react'

// Recursively collect files from a FileSystemEntry (file or directory).
function collectFromEntry(entry, out) {
  return new Promise((resolve) => {
    if (entry.isFile) {
      entry.file((f) => { out.push(f); resolve() }, () => resolve())
    } else if (entry.isDirectory) {
      const reader = entry.createReader()
      const step = () => {
        reader.readEntries((entries) => {
          if (!entries.length) return resolve()
          Promise.all(entries.map((e) => collectFromEntry(e, out))).then(step)
        }, () => resolve())
      }
      step()
    } else {
      resolve()
    }
  })
}

async function collectFiles(items) {
  const out = []
  for (const item of items) {
    if (item.webkitGetAsEntry) {
      const entry = item.webkitGetAsEntry()
      if (entry) await collectFromEntry(entry, out)
    } else {
      const f = item.getAsFile ? item.getAsFile() : null
      if (f) out.push(f)
    }
  }
  return out
}

const accept = (list) => list.filter(
  (f) => f.name.toLowerCase().endsWith('.glb') || f.name.toLowerCase().endsWith('.gltf'),
)

export default function DropZone({ files, onFiles }) {
  const [dragOver, setDragOver] = useState(false)
  const inputRef = useRef(null)

  const handleDrop = useCallback(async (e) => {
    e.preventDefault()
    setDragOver(false)
    const items = e.dataTransfer.items
    let list = []
    if (items && items.length) {
      list = await collectFiles([...items])
    } else {
      list = [...e.dataTransfer.files]
    }
    const accepted = accept(list)
    if (accepted.length) onFiles(accepted)
  }, [onFiles])

  const handleChange = useCallback((e) => {
    const accepted = accept([...e.target.files])
    if (accepted.length) onFiles(accepted)
    e.target.value = ''
  }, [onFiles])

  return (
    <div
      className={`dropzone ${dragOver ? 'drag-over' : ''} ${files.length ? 'has-file' : ''}`}
      onDragOver={(e) => { e.preventDefault(); setDragOver(true) }}
      onDragLeave={() => setDragOver(false)}
      onDrop={handleDrop}
      onClick={() => inputRef.current?.click()}
    >
      <input
        ref={inputRef}
        type="file"
        accept=".glb,.gltf"
        multiple
        onChange={handleChange}
        hidden
      />
      {files.length ? (
        <div className="dropzone-file">
          <span className="dropzone-icon">🗂️</span>
          <span className="dropzone-name">
            {files.length === 1 ? files[0].name : `${files.length} files selected`}
          </span>
          {files.length === 1 && (
            <span className="dropzone-size">{(files[0].size / 1024 / 1024).toFixed(1)} MB</span>
          )}
          {files.length > 1 && (
            <span className="dropzone-size">
              {files.slice(0, 4).map((f) => f.name).join(', ')}
              {files.length > 4 ? ` +${files.length - 4} more` : ''}
            </span>
          )}
          <span className="dropzone-hint">Click or drop to replace · supports folders</span>
        </div>
      ) : (
        <div className="dropzone-empty">
          <span className="dropzone-icon">🗂️</span>
          <span className="dropzone-text">Drop .glb / .gltf files or a folder here</span>
          <span className="dropzone-hint">Multiple files → batch conversion</span>
        </div>
      )}
    </div>
  )
}
