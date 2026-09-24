import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook, waitFor } from '@testing-library/react'
import { useConversion } from './useConversion'
import { defaultsFrom } from '../settings'
import { jobView, model, schema } from '../test/fixtures'
import type { JobView } from '../types'

const api = vi.hoisted(() => ({
  startJobs: vi.fn(),
  cancelJob: vi.fn(async () => null),
  saveToFolder: vi.fn(),
  revealJob: vi.fn(async () => null),
  watchers: new Map<string, (view: JobView) => void>(),
}))

vi.mock('../api', () => ({
  startJobs: api.startJobs,
  cancelJob: api.cancelJob,
  saveToFolder: api.saveToFolder,
  revealJob: api.revealJob,
  watchJob: (id: string, onUpdate: (view: JobView) => void) => {
    api.watchers.set(id, onUpdate)
    return () => api.watchers.delete(id)
  },
}))

beforeEach(() => {
  vi.clearAllMocks()
  api.watchers.clear()
})

function setup(settings = defaultsFrom(schema)) {
  const notify = vi.fn()
  const hook = renderHook(({ s }) => useConversion({ schema, settings: s, notify }), {
    initialProps: { s: settings },
  })
  return { ...hook, notify }
}

describe('useConversion', () => {
  it('queues models only, once each, and selects the first new one', () => {
    const { result, notify } = setup()
    const castle = model('castle.glb')
    act(() => result.current.addFiles([castle, model('notes.txt'), model('tower.GLTF')]))
    act(() => result.current.addFiles([castle]))
    expect(result.current.items.map((it) => it.file.name)).toEqual(['castle.glb', 'tower.GLTF'])
    expect(result.current.selected?.file.name).toBe('castle.glb')
    expect(notify).toHaveBeenCalledWith('info', expect.stringContaining('not .glb or .gltf'))
  })

  it('converts the batch in one request and follows every job to the end', async () => {
    api.startJobs.mockResolvedValue({
      jobs: [
        { job_id: 'a', filename: 'castle.glb', name: 'castle' },
        { job_id: 'b', filename: 'tower.glb', name: 'tower' },
      ],
    })
    const { result } = setup()
    act(() => result.current.addFiles([model('castle.glb'), model('tower.glb')]))
    await act(() => result.current.convert())

    expect(api.startJobs).toHaveBeenCalledTimes(1)
    const [files, sent] = api.startJobs.mock.calls[0]
    expect(files.map((f: File) => f.name)).toEqual(['castle.glb', 'tower.glb'])
    expect(sent).toMatchObject({ max_size: 128, threads: 4, output_dir: null })
    await waitFor(() => expect([...api.watchers.keys()].sort()).toEqual(['a', 'b']))

    act(() => api.watchers.get('a')!(jobView('a', { status: 'running', progress: 40 })))
    expect(result.current.items[0]).toMatchObject({ status: 'running', progress: 40 })
    act(() => api.watchers.get('a')!(jobView('a', { status: 'done', progress: 100 })))
    expect(result.current.items[0].status).toBe('done')
    expect(result.current.pending.map((it) => it.file.name)).toEqual([])
  })

  it('announces failures and marks results made with other settings', async () => {
    api.startJobs.mockResolvedValue({
      jobs: [{ job_id: 'a', filename: 'castle.glb', name: 'castle' }],
    })
    const { result, rerender, notify } = setup()
    act(() => result.current.addFiles([model('castle.glb')]))
    await act(() => result.current.convert())
    await waitFor(() => expect(api.watchers.has('a')).toBe(true))
    act(() => api.watchers.get('a')!(jobView('a', { status: 'done' })))
    expect(result.current.isStale(result.current.items[0])).toBe(false)

    rerender({ s: { ...defaultsFrom(schema), max_size: 64 } })
    expect(result.current.isStale(result.current.items[0])).toBe(true)
    expect(result.current.pending).toHaveLength(1)

    api.startJobs.mockRejectedValue(new Error('Upload failed: too large'))
    await act(() => result.current.convert())
    expect(result.current.items[0]).toMatchObject({
      status: 'error',
      error: 'Upload failed: too large',
    })
    expect(notify).toHaveBeenCalledWith('error', 'Upload failed: too large')
  })

  it('cancels running jobs when they are removed', async () => {
    api.startJobs.mockResolvedValue({
      jobs: [{ job_id: 'a', filename: 'castle.glb', name: 'castle' }],
    })
    const { result } = setup()
    act(() => result.current.addFiles([model('castle.glb')]))
    await act(() => result.current.convert())
    act(() => result.current.remove(result.current.items[0].key))
    expect(api.cancelJob).toHaveBeenCalledWith('a')
    expect(result.current.items).toEqual([])
  })
})
