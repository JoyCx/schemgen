import { describe, expect, it } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { useSettings } from './useSettings'
import { schema } from '../test/fixtures'

describe('useSettings', () => {
  it('waits for the schema, then starts from its defaults', () => {
    const { result, rerender } = renderHook(({ s }) => useSettings(s), {
      initialProps: { s: null as typeof schema | null },
    })
    expect(result.current.settings).toBeNull()
    rerender({ s: schema })
    expect(result.current.settings?.max_size).toBe(128)
  })

  it('changes, resets and remembers', () => {
    const { result } = renderHook(() => useSettings(schema))
    act(() => result.current.set('max_size', 64))
    act(() => result.current.setMany({ target: '1.20.4', output_dir: '/mc' }))
    expect(result.current.settings).toMatchObject({ max_size: 64, target: '1.20.4' })
    expect(JSON.parse(localStorage.getItem('schemgen2.output')!)).toMatchObject({
      target: '1.20.4',
      output_dir: '/mc',
    })

    act(() => result.current.reset(['max_size']))
    expect(result.current.settings).toMatchObject({ max_size: 128, target: '1.20.4' })
    act(() => result.current.reset())
    expect(result.current.settings?.target).toBe(schema.default_target)
  })

  it('starts from what was remembered', () => {
    localStorage.setItem('schemgen2.output', JSON.stringify({ target: '1.19.4', format: 'schem' }))
    const { result } = renderHook(() => useSettings(schema))
    expect(result.current.settings).toMatchObject({ target: '1.19.4', format: 'schem' })
  })
})
