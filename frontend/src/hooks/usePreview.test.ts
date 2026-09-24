import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { usePreview } from './usePreview'
import { defaultsFrom } from '../settings'
import { model, schema } from '../test/fixtures'

const fetchPreview = vi.hoisted(() => vi.fn())
vi.mock('../api', () => ({ fetchPreview }))

beforeEach(() => {
  vi.useFakeTimers()
  fetchPreview.mockReset()
})
afterEach(() => vi.useRealTimers())

const preview = { dims: [4, 4, 4], palette: [], blocks: new Int32Array(0), materials: [] }

describe('usePreview', () => {
  it('builds once settings stop changing, and keeps the last one up meanwhile', async () => {
    fetchPreview.mockResolvedValue(preview)
    const file = model()
    const { result, rerender } = renderHook(({ s }) => usePreview(file, s, schema), {
      initialProps: { s: defaultsFrom(schema) },
    })
    rerender({ s: { ...defaultsFrom(schema), max_size: 60 } })
    rerender({ s: { ...defaultsFrom(schema), max_size: 64 } })
    await act(() => vi.advanceTimersByTimeAsync(500))

    expect(fetchPreview).toHaveBeenCalledTimes(1)
    expect(fetchPreview.mock.calls[0][1]).toMatchObject({ max_size: 64 })
    expect(result.current.preview).toBe(preview)
    expect(result.current.loading).toBe(false)

    fetchPreview.mockReturnValue(new Promise(() => {}))
    rerender({ s: { ...defaultsFrom(schema), max_size: 32 } })
    await act(() => vi.advanceTimersByTimeAsync(500))
    expect(result.current.loading).toBe(true)
    expect(result.current.preview).toBe(preview)
  })

  it('ignores settings a preview does not use', async () => {
    fetchPreview.mockResolvedValue(preview)
    const file = model()
    const { rerender } = renderHook(({ s }) => usePreview(file, s, schema), {
      initialProps: { s: defaultsFrom(schema) },
    })
    await act(() => vi.advanceTimersByTimeAsync(500))
    rerender({ s: { ...defaultsFrom(schema), threads: 9, output_dir: '/x' } })
    await act(() => vi.advanceTimersByTimeAsync(500))
    expect(fetchPreview).toHaveBeenCalledTimes(1)
  })

  it('reports errors for the current request', async () => {
    fetchPreview.mockRejectedValue(new Error('Preview failed: bad model'))
    const { result } = renderHook(() => usePreview(model(), defaultsFrom(schema), schema))
    await act(() => vi.advanceTimersByTimeAsync(500))
    expect(result.current.error).toBe('Preview failed: bad model')
  })
})
