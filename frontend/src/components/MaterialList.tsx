// What the schematic is made of: every block and how many, sortable, and
// copyable as text for whoever gathers the materials.

import { useState } from 'react'
import { ArrowDownAZ, ArrowDownWideNarrow, Check, Copy } from 'lucide-react'
import { blockLabel, inStacks, materialsText, sortMaterials, type MaterialSort } from '../materials'
import type { Material, PaletteColors } from '../types'

interface Props {
  materials: Material[]
  /** `final` for a finished conversion; `preview` for the low-resolution one. */
  source: 'final' | 'preview'
  title: string
  palette: PaletteColors | null
}

export function MaterialList({ materials, source, title, palette }: Props) {
  const [sort, setSort] = useState<MaterialSort>('count')
  const [copied, setCopied] = useState(false)
  const sorted = sortMaterials(materials, sort)
  const total = materials.reduce((n, m) => n + m.count, 0)

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(materialsText(sorted, title))
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      /* clipboard refused — nothing useful to say */
    }
  }

  return (
    <section className="materials" aria-label="Material list">
      <header className="materials-header">
        <h2>
          Materials{' '}
          <span className={`badge${source === 'preview' ? ' badge--muted' : ''}`}>
            {source === 'preview' ? 'preview' : 'final'}
          </span>
        </h2>
        <span className="materials-summary">
          {total.toLocaleString()} blocks · {materials.length} kinds
        </span>
        <div className="materials-tools">
          <button
            type="button"
            className="icon-button"
            onClick={() => setSort(sort === 'count' ? 'name' : 'count')}
            title={sort === 'count' ? 'Sort by name' : 'Sort by count'}
            aria-label={sort === 'count' ? 'Sort by name' : 'Sort by count'}
          >
            {sort === 'count' ? <ArrowDownAZ size={16} /> : <ArrowDownWideNarrow size={16} />}
          </button>
          <button
            type="button"
            className="icon-button"
            onClick={copy}
            title="Copy as text"
            aria-label="Copy as text"
          >
            {copied ? <Check size={16} /> : <Copy size={16} />}
          </button>
        </div>
      </header>
      <p className="materials-note">
        {source === 'preview' ? 'Counts from the preview; convert for the final list. ' : ''}
        sb: shulker box (1,728) · st: stack (64)
      </p>
      <ol className="materials-list">
        {sorted.map((m) => {
          const rgb = palette?.[m.name.replace(/^minecraft:/, '')]
          return (
            <li key={m.name}>
              <span
                className="swatch"
                style={rgb ? { background: `rgb(${rgb[0]}, ${rgb[1]}, ${rgb[2]})` } : undefined}
                aria-hidden
              />
              <span className="materials-name" title={m.name}>
                {blockLabel(m.name)}
              </span>
              <span className="materials-count">{m.count.toLocaleString()}</span>
              <span className="materials-stacks">{inStacks(m.count)}</span>
            </li>
          )
        })}
      </ol>
    </section>
  )
}
