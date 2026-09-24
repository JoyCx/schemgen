import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MaterialList } from './MaterialList'

const materials = [
  { name: 'minecraft:stone', count: 1240 },
  { name: 'minecraft:andesite', count: 70 },
]

describe('MaterialList', () => {
  it('lists blocks by count and sorts by name on request', async () => {
    render(
      <MaterialList
        materials={materials}
        source="final"
        title="Materials"
        palette={{ stone: [120, 120, 120] }}
      />,
    )
    const names = () =>
      screen.getAllByRole('listitem').map((li) => li.querySelector('.materials-name')?.textContent)
    expect(names()).toEqual(['Stone', 'Andesite'])
    expect(screen.getByText('1,310 blocks · 2 kinds')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Sort by name' }))
    expect(names()).toEqual(['Andesite', 'Stone'])
  })

  it('copies the list as text', async () => {
    const user = userEvent.setup()
    const writeText = vi.spyOn(navigator.clipboard, 'writeText')
    render(
      <MaterialList
        materials={materials}
        source="preview"
        title="Materials for castle"
        palette={null}
      />,
    )
    expect(screen.getByText(/Counts from the preview/)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Copy as text' }))
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining('1,240  Stone'))
  })
})
