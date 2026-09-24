import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SettingsForm } from './SettingsForm'
import { defaultsFrom, type Settings } from '../settings'
import { schema } from '../test/fixtures'

vi.mock('../api', () => ({
  fetchOutputDirSuggestions: vi.fn(async () => [
    {
      path: '/home/me/.minecraft/schematics',
      exists: true,
      instance: { name: 'ATM9', launcher: 'CurseForge', mc_version: '1.20.1', target: '1.20.1' },
    },
  ]),
  checkOutputDir: vi.fn(async (path: string) => ({ ok: true, path, exists: true })),
  revealFolder: vi.fn(async () => null),
}))

/** The form with real state, reporting every change. */
function Harness({ onChange = () => {} }: { onChange?: (key: string, value: unknown) => void }) {
  const [settings, setSettings] = useState<Settings>(defaultsFrom(schema))
  return (
    <SettingsForm
      schema={schema}
      settings={settings}
      onChange={(key, value) => {
        onChange(key, value)
        setSettings((s) => ({ ...s, [key]: value }))
      }}
      onChangeMany={(values) => setSettings((s) => ({ ...s, ...values }))}
      onReset={(keys) =>
        setSettings((s) => ({
          ...s,
          ...Object.fromEntries(keys.map((k) => [k, defaultsFrom(schema)[k]])),
        }))
      }
      fileManager="Explorer"
    />
  )
}

describe('SettingsForm', () => {
  it('draws the groups the schema lists, in order, open ones first', () => {
    render(<Harness />)
    const toggles = screen.getAllByRole('button', { expanded: true }).map((b) => b.textContent)
    expect(toggles).toEqual(['Size & shape', 'Color', 'Target version', 'Output'])
    expect(screen.getByRole('button', { name: 'Lighting' })).toHaveAttribute(
      'aria-expanded',
      'false',
    )
  })

  it('shows a field only while its condition holds', async () => {
    render(<Harness />)
    expect(screen.getByRole('checkbox', { name: 'Dithering' })).toBeInTheDocument()
    expect(screen.queryByRole('combobox', { name: 'Block' })).toBeNull()
    await userEvent.click(screen.getByRole('checkbox', { name: 'Color sampling' }))
    expect(screen.queryByRole('checkbox', { name: 'Dithering' })).toBeNull()
    expect(screen.getByRole('combobox', { name: 'Block' })).toBeInTheDocument()
  })

  it('reports numbers from sliders and boxes', async () => {
    const onChange = vi.fn()
    render(<Harness onChange={onChange} />)
    const box = screen.getByRole('spinbutton', { name: 'Max size value' })
    await userEvent.clear(box)
    await userEvent.type(box, '64')
    expect(onChange).toHaveBeenLastCalledWith('max_size', 64)
    expect(screen.getByRole('slider', { name: 'Max size' })).toHaveValue('64')
  })

  it('lists targets with their block counts', () => {
    render(<Harness />)
    const select = screen.getByRole('combobox', { name: 'Minecraft version' })
    const options = within(select)
      .getAllByRole('option')
      .map((o) => o.textContent)
    expect(options).toContain('1.21.8 — 181 blocks (default)')
    expect(options).toContain('1.16.5 — 137 blocks')
  })

  it('keeps advanced fields behind a switch', async () => {
    render(<Harness />)
    expect(screen.queryByRole('textbox', { name: 'Voxel size' })).toBeNull()
    await userEvent.click(screen.getByRole('checkbox', { name: 'Show advanced settings' }))
    expect(screen.getByRole('textbox', { name: 'Voxel size' })).toBeInTheDocument()
  })

  it('offers launcher folders, and picking one sets its version', async () => {
    render(<Harness />)
    await userEvent.click(screen.getByRole('checkbox', { name: 'Save into folder' }))
    await userEvent.click(await screen.findByRole('button', { name: 'CurseForge · ATM9 · 1.20.1' }))
    expect(screen.getByRole('textbox', { name: 'Folder path' })).toHaveValue(
      '/home/me/.minecraft/schematics',
    )
    expect(screen.getByRole('combobox', { name: 'Minecraft version' })).toHaveValue('1.20.1')
    expect(await screen.findByText(/Saving to \/home\/me/)).toBeInTheDocument()
  })

  it('resets a group to the defaults', async () => {
    render(<Harness />)
    await userEvent.click(screen.getByRole('checkbox', { name: 'Dithering' }))
    expect(screen.getByRole('checkbox', { name: 'Dithering' })).not.toBeChecked()
    await userEvent.click(screen.getByRole('button', { name: 'Reset color to the defaults' }))
    expect(screen.getByRole('checkbox', { name: 'Dithering' })).toBeChecked()
  })
})
