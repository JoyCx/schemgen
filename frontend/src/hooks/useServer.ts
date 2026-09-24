// The server's schema and health, and whether it is reachable at all.

import { useEffect, useState } from 'react'
import { fetchHealth, fetchPalette, fetchSchema, fetchSystemInfo, fetchTexturesInfo } from '../api'
import type { Health, PaletteColors, Schema, TexturesInfo } from '../types'

const HEALTH_EVERY_MS = 15_000
const SCHEMA_RETRY_MS = 3_000

export interface ServerState {
  schema: Schema | null
  health: Health | null
  /** `null` until the first health check answers. */
  online: boolean | null
  /** What this OS calls its file manager ("Explorer", "Finder", …). */
  fileManager: string
  textures: TexturesInfo | null
}

export function useServer(): ServerState {
  const [schema, setSchema] = useState<Schema | null>(null)
  const [health, setHealth] = useState<Health | null>(null)
  const [online, setOnline] = useState<boolean | null>(null)
  const [fileManager, setFileManager] = useState('file manager')
  const [textures, setTextures] = useState<TexturesInfo | null>(null)

  // The schema is what every form field comes from: keep asking until the
  // server answers.
  useEffect(() => {
    let stopped = false
    let timer: ReturnType<typeof setTimeout>
    const load = async () => {
      try {
        const s = await fetchSchema()
        if (!stopped) setSchema(s)
      } catch {
        if (!stopped) timer = setTimeout(load, SCHEMA_RETRY_MS)
      }
    }
    load()
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  }, [])

  useEffect(() => {
    let stopped = false
    let timer: ReturnType<typeof setTimeout>
    const check = async () => {
      const controller = new AbortController()
      const timeout = setTimeout(() => controller.abort(), 5000)
      try {
        const h = await fetchHealth(controller.signal)
        if (!stopped) {
          setHealth(h)
          setOnline(true)
        }
      } catch {
        if (!stopped) setOnline(false)
      } finally {
        clearTimeout(timeout)
      }
      if (!stopped) timer = setTimeout(check, HEALTH_EVERY_MS)
    }
    check()
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  }, [])

  useEffect(() => {
    let stopped = false
    fetchSystemInfo()
      .then((info) => !stopped && info?.file_manager && setFileManager(info.file_manager))
      .catch(() => {})
    fetchTexturesInfo()
      .then((info) => !stopped && setTextures(info))
      .catch(() => {})
    return () => {
      stopped = true
    }
  }, [])

  return { schema, health, online, fileManager, textures }
}

/** Block colors for a target's palette. */
export function usePalette(target: string | undefined): PaletteColors | null {
  const [palette, setPalette] = useState<{ target: string; colors: PaletteColors } | null>(null)
  useEffect(() => {
    if (!target) return
    let stopped = false
    fetchPalette(target)
      .then((colors) => !stopped && setPalette({ target, colors }))
      .catch(() => {})
    return () => {
      stopped = true
    }
  }, [target])
  // A palette for another target is still better than none while the right
  // one loads: most blocks exist in both.
  return palette?.colors ?? null
}
