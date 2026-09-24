// Dropping files anywhere on the page adds them, folders included.

import { useEffect, useState } from 'react'
import { Upload } from 'lucide-react'
import { droppedFiles } from '../dropFiles'

export function DropOverlay({ onFiles }: { onFiles: (files: File[]) => void }) {
  const [over, setOver] = useState(false)

  useEffect(() => {
    let depth = 0
    const hasFiles = (e: DragEvent) =>
      !!e.dataTransfer && [...e.dataTransfer.types].includes('Files')
    const enter = (e: DragEvent) => {
      if (!hasFiles(e)) return
      depth++
      setOver(true)
    }
    const leave = (e: DragEvent) => {
      if (!hasFiles(e)) return
      depth = Math.max(0, depth - 1)
      if (!depth) setOver(false)
    }
    const overHandler = (e: DragEvent) => {
      if (hasFiles(e)) e.preventDefault()
    }
    const drop = async (e: DragEvent) => {
      if (!hasFiles(e) || !e.dataTransfer) return
      e.preventDefault()
      depth = 0
      setOver(false)
      onFiles(await droppedFiles(e.dataTransfer))
    }
    window.addEventListener('dragenter', enter)
    window.addEventListener('dragleave', leave)
    window.addEventListener('dragover', overHandler)
    window.addEventListener('drop', drop)
    return () => {
      window.removeEventListener('dragenter', enter)
      window.removeEventListener('dragleave', leave)
      window.removeEventListener('dragover', overHandler)
      window.removeEventListener('drop', drop)
    }
  }, [onFiles])

  if (!over) return null
  return (
    <div className="drop-overlay" aria-hidden>
      <Upload size={40} />
      <p>Drop models to add them</p>
    </div>
  )
}
