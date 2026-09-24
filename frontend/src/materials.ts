// Material lists: what a schematic is made of, as builders read it.

import type { Material } from './types'

export const STACK = 64
export const SHULKER = 27 * STACK

/** "Waxed cut copper" for `minecraft:waxed_cut_copper`. */
export function blockLabel(id: string): string {
  const name = id.replace(/^minecraft:/, '').replace(/_/g, ' ')
  return name.charAt(0).toUpperCase() + name.slice(1)
}

/** "2 sb + 3 st + 12" — shulker boxes, stacks and single blocks. */
export function inStacks(count: number): string {
  const boxes = Math.floor(count / SHULKER)
  const stacks = Math.floor((count % SHULKER) / STACK)
  const rest = count % STACK
  const parts: string[] = []
  if (boxes) parts.push(`${boxes} sb`)
  if (stacks) parts.push(`${stacks} st`)
  if (rest || !parts.length) parts.push(String(rest))
  return parts.join(' + ')
}

export type MaterialSort = 'count' | 'name'

export function sortMaterials(materials: Material[], by: MaterialSort): Material[] {
  const out = [...materials]
  if (by === 'name') out.sort((a, b) => blockLabel(a.name).localeCompare(blockLabel(b.name)))
  else out.sort((a, b) => b.count - a.count || a.name.localeCompare(b.name))
  return out
}

/** The list as plain text, aligned for reading and pasting. */
export function materialsText(materials: Material[], title: string): string {
  const total = materials.reduce((n, m) => n + m.count, 0)
  const width = Math.max(...materials.map((m) => m.count.toLocaleString('en-US').length), 1)
  const nameWidth = Math.max(...materials.map((m) => blockLabel(m.name).length), 1)
  const lines = materials.map(
    (m) =>
      `${m.count.toLocaleString('en-US').padStart(width)}  ${blockLabel(m.name).padEnd(nameWidth)}  ${inStacks(m.count)}`,
  )
  return [
    title,
    ...lines,
    `${total.toLocaleString('en-US')} blocks, ${materials.length} kinds (sb = shulker box, st = stack of 64)`,
  ].join('\n')
}
