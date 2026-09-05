import { useState } from 'react'

export default function PaletteGrid({ palette }) {
  const [expanded, setExpanded] = useState(false)

  const entries = Object.entries(palette)
  const display = expanded ? entries : entries.slice(0, 24)

  return (
    <div className="panel palette-panel">
      <h3 onClick={() => setExpanded(!expanded)} className="palette-title">
        Block Palette ({entries.length} blocks)
        <span className="palette-toggle">{expanded ? '−' : '+'}</span>
      </h3>

      <div className="palette-grid">
        {display.map(([name, rgb]) => (
          <div
            key={name}
            className="palette-item"
            title={name}
          >
            <div
              className="palette-swatch"
              style={{ backgroundColor: `rgb(${rgb[0]},${rgb[1]},${rgb[2]})` }}
            />
            <span className="palette-name">{name}</span>
          </div>
        ))}
      </div>

      {entries.length > 24 && !expanded && (
        <button className="btn-show-more" onClick={() => setExpanded(true)}>
          Show all {entries.length} blocks
        </button>
      )}
    </div>
  )
}
