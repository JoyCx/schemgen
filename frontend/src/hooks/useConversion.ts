// The job list: models to convert, their jobs on the server, and what can be
// done with the results. One model is a batch of one — there is no separate
// single-file mode.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { cancelJob, revealJob, saveToFolder, startJobs } from '../api'
import { AUTO_SAVE, conversionKey, toApiSettings, type Settings } from '../settings'
import type { JobStatus, JobView, Schema } from '../types'
import { useJobEvents } from './useJobEvents'

export type ItemStatus = 'ready' | 'uploading' | JobStatus

export interface QueueItem {
  key: string
  file: File
  status: ItemStatus
  jobId: string | null
  /** 0–100. */
  progress: number
  message: string
  error: string | null
  view: JobView | null
  /** The conversion key of the settings it was last converted with. */
  convertedWith: string | null
}

export const isActive = (s: ItemStatus) => s === 'uploading' || s === 'queued' || s === 'running'

export function isModel(file: File): boolean {
  const name = file.name.toLowerCase()
  return name.endsWith('.glb') || name.endsWith('.gltf')
}

const fileIdentity = (f: File) => `${f.name}|${f.size}|${f.lastModified}`

let nextKey = 1

interface Options {
  schema: Schema | null
  settings: Settings | null
  notify: (kind: 'error' | 'success' | 'info', text: string) => void
}

export interface Conversion {
  items: QueueItem[]
  selected: QueueItem | null
  select: (key: string) => void
  addFiles: (files: File[]) => void
  remove: (key: string) => void
  clear: () => void
  /** Convert the given items, or every one not already converted with the
   *  current settings. */
  convert: (keys?: string[]) => Promise<void>
  cancel: (key: string) => void
  save: (key: string) => Promise<void>
  saveAll: () => Promise<void>
  reveal: (key: string) => Promise<void>
  /** Items Convert would convert now. */
  pending: QueueItem[]
  /** Whether an item's result no longer matches the settings. */
  isStale: (item: QueueItem) => boolean
}

export function useConversion({ schema, settings, notify }: Options): Conversion {
  const [items, setItems] = useState<QueueItem[]>([])
  const [selectedKey, setSelectedKey] = useState<string | null>(null)

  const currentKey = useMemo(
    () => (schema && settings ? conversionKey(settings, schema) : null),
    [schema, settings],
  )

  // The list as of the last render, for deciding what to announce; updates
  // themselves go through the state setter.
  const latest = useRef(items)
  useEffect(() => {
    latest.current = items
  })

  const onJobUpdate = useCallback(
    (view: JobView) => {
      const before = latest.current.find((it) => it.jobId === view.id)
      if (before && view.status !== before.status) {
        if (view.status === 'error') {
          notify('error', `${before.file.name}: ${view.error || 'conversion failed'}`)
        } else if (view.status === 'done' && view.save_error) {
          notify(
            'error',
            `${before.file.name} converted, but could not be saved: ${view.save_error}`,
          )
        }
      }
      setItems((all) =>
        all.map((it) =>
          it.jobId === view.id
            ? {
                ...it,
                status: view.status,
                progress: view.progress,
                message: view.error || view.message,
                error: view.error,
                view,
              }
            : it,
        ),
      )
    },
    [notify],
  )

  const following = items.filter(
    (it) => it.jobId && (it.status === 'queued' || it.status === 'running'),
  )
  useJobEvents(
    following.map((it) => it.jobId as string),
    onJobUpdate,
  )

  const addFiles = useCallback(
    (files: File[]) => {
      const models = files.filter(isModel)
      if (models.length < files.length) {
        notify('info', `Skipped ${files.length - models.length} file(s) that are not .glb or .gltf`)
      }
      const seen = new Set(items.map((it) => fileIdentity(it.file)))
      const fresh: QueueItem[] = models
        .filter((f) => !seen.has(fileIdentity(f)))
        .map((file) => ({
          key: `item-${nextKey++}`,
          file,
          status: 'ready',
          jobId: null,
          progress: 0,
          message: 'Ready',
          error: null,
          view: null,
          convertedWith: null,
        }))
      if (!fresh.length) return
      setItems((all) => [...all, ...fresh])
      setSelectedKey(fresh[0].key)
    },
    [items, notify],
  )

  const cancel = useCallback(
    (key: string) => {
      const item = items.find((it) => it.key === key)
      if (item?.jobId && isActive(item.status)) cancelJob(item.jobId).catch(() => {})
    },
    [items],
  )

  const remove = useCallback(
    (key: string) => {
      cancel(key)
      setItems((all) => all.filter((it) => it.key !== key))
      setSelectedKey((sel) => (sel === key ? null : sel))
    },
    [cancel],
  )

  const clear = useCallback(() => {
    for (const it of items) if (it.jobId && isActive(it.status)) cancelJob(it.jobId).catch(() => {})
    setItems([])
    setSelectedKey(null)
  }, [items])

  const isStale = useCallback(
    (item: QueueItem) => item.status === 'done' && item.convertedWith !== currentKey,
    [currentKey],
  )

  const pending = useMemo(
    () =>
      items.filter(
        (it) =>
          it.status === 'ready' ||
          it.status === 'error' ||
          it.status === 'cancelled' ||
          (it.status === 'done' && it.convertedWith !== currentKey),
      ),
    [items, currentKey],
  )

  const convert = useCallback(
    async (keys?: string[]) => {
      if (!schema || !settings) return
      const batch = keys
        ? items.filter((it) => keys.includes(it.key) && !isActive(it.status))
        : pending
      if (!batch.length) return
      const key = conversionKey(settings, schema)
      const batchKeys = new Set(batch.map((it) => it.key))
      setItems((all) =>
        all.map((it) =>
          batchKeys.has(it.key)
            ? {
                ...it,
                status: 'uploading',
                progress: 0,
                message: 'Uploading…',
                error: null,
                view: null,
                jobId: null,
                convertedWith: key,
              }
            : it,
        ),
      )
      try {
        const { jobs } = await startJobs(
          batch.map((it) => it.file),
          toApiSettings(settings, schema, { job: true }),
        )
        setItems((all) =>
          all.map((it) => {
            const index = batch.findIndex((b) => b.key === it.key)
            const job = index >= 0 ? jobs[index] : undefined
            return job ? { ...it, jobId: job.job_id, status: 'queued', message: 'Queued…' } : it
          }),
        )
      } catch (e) {
        const message = e instanceof Error ? e.message : 'Upload failed'
        notify('error', message)
        setItems((all) =>
          all.map((it) =>
            batchKeys.has(it.key) ? { ...it, status: 'error', error: message, message } : it,
          ),
        )
      }
    },
    [schema, settings, items, pending, notify],
  )

  const outputDir = settings && settings[AUTO_SAVE] ? String(settings.output_dir ?? '').trim() : ''

  const save = useCallback(
    async (key: string) => {
      const item = items.find((it) => it.key === key)
      if (!item?.jobId || item.status !== 'done' || !outputDir) return
      try {
        const { saved_path } = await saveToFolder(item.jobId, outputDir)
        setItems((all) =>
          all.map((it) =>
            it.key === key && it.view
              ? { ...it, view: { ...it.view, saved_path, save_error: null } }
              : it,
          ),
        )
        notify('success', `Saved ${saved_path}`)
      } catch (e) {
        notify('error', `Could not save ${item.file.name}: ${e instanceof Error ? e.message : e}`)
      }
    },
    [items, outputDir, notify],
  )

  const saveAll = useCallback(async () => {
    for (const it of items) if (it.status === 'done') await save(it.key)
  }, [items, save])

  const reveal = useCallback(
    async (key: string) => {
      const item = items.find((it) => it.key === key)
      if (!item?.jobId) return
      try {
        await revealJob(item.jobId)
      } catch (e) {
        notify('error', e instanceof Error ? e.message : 'Could not open the file manager')
      }
    },
    [items, notify],
  )

  const selected = items.find((it) => it.key === selectedKey) ?? items[0] ?? null

  return {
    items,
    selected,
    select: setSelectedKey,
    addFiles,
    remove,
    clear,
    convert,
    cancel,
    save,
    saveAll,
    reveal,
    pending,
    isStale,
  }
}
