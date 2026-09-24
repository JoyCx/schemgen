// Follow a set of jobs over server-sent events (polling where those fail).

import { useEffect, useRef } from 'react'
import { watchJob } from '../api'
import type { JobView } from '../types'

/** Call `onUpdate` with every state change of the jobs in `jobIds`. A job
 *  that leaves the list stops being followed; one that finishes stops by
 *  itself. */
export function useJobEvents(jobIds: string[], onUpdate: (view: JobView) => void): void {
  const handler = useRef(onUpdate)
  useEffect(() => {
    handler.current = onUpdate
  })

  const watches = useRef(new Map<string, () => void>())
  const key = [...new Set(jobIds)].sort().join(',')

  useEffect(() => {
    const wanted = new Set(key ? key.split(',') : [])
    const current = watches.current
    for (const [id, stop] of current) {
      if (!wanted.has(id)) {
        stop()
        current.delete(id)
      }
    }
    for (const id of wanted) {
      if (!current.has(id))
        current.set(
          id,
          watchJob(id, (view) => handler.current(view)),
        )
    }
  }, [key])

  useEffect(() => {
    const current = watches.current
    return () => {
      for (const stop of current.values()) stop()
      current.clear()
    }
  }, [])
}
