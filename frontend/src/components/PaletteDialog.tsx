// Every block the target's palette may use, with its color.

import { useEffect, useRef, useState } from 'react'
import { Search, X } from 'lucide-react'
import { blockLabel } from '../materials'
import type { PaletteColors } from '../types'

interface Props {
  open: boolean
  onClose: () => void
  palette: PaletteColors | null
  target: string | undefined
}

export function PaletteDialog({ open, onClose, palette, target }: Props) {
  const ref = useRef<HTMLDialogElement>(null)
  const [query, setQuery] = useState('')

  useEffect(() => {
    const dialog = ref.current
    if (!dialog) return
    if (open && !dialog.open) dialog.showModal?.()
    if (!open && dialog.open) dialog.close?.()
  }, [open])

  const entries = Object.entries(palette ?? {})
    .filter(([name]) => blockLabel(name).toLowerCase().includes(query.trim().toLowerCase()))
    .sort(([a], [b]) => a.localeCompare(b))

  return (
    <dialog
      ref={ref}
      className="dialog"
      onClose={onClose}
      onCancel={onClose}
      aria-label="Block palette"
    >
      <header className="dialog-header">
        <h2>
          Block palette{target ? ` · Minecraft ${target}` : ''} ({Object.keys(palette ?? {}).length}
          )
        </h2>
        <button type="button" className="icon-button" onClick={onClose} aria-label="Close">
          <X size={18} />
        </button>
      </header>
      <p className="dialog-note">
        Full, solid, grief-safe blocks only: nothing that falls, burns, decays or needs support.
      </p>
      <label className="search">
        <Search size={16} aria-hidden />
        <input
          className="input"
          type="search"
          placeholder="Filter blocks"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </label>
      <ul className="palette-grid">
        {entries.map(([name, rgb]) => (
          <li key={name} title={`minecraft:${name}`}>
            <span
              className="swatch swatch--large"
              style={{ background: `rgb(${rgb[0]}, ${rgb[1]}, ${rgb[2]})` }}
            />
            <span>{blockLabel(name)}</span>
          </li>
        ))}
      </ul>
    </dialog>
  )
}
