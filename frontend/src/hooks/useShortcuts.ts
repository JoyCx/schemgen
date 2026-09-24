// Keyboard shortcuts for the whole page.

import { useEffect, useRef } from 'react'

export type Shortcuts = Record<string, (event: KeyboardEvent) => void>

function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable
}

/** Keys are `mod+enter` (Ctrl, or ⌘ on a Mac) or a plain key such as `1`.
 *  Plain keys are ignored while typing in a field; `mod+` ones are not. */
export function useShortcuts(shortcuts: Shortcuts): void {
  const latest = useRef(shortcuts)
  useEffect(() => {
    latest.current = shortcuts
  })

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey
      const key = event.key.toLowerCase()
      const name = mod ? `mod+${key}` : key
      if (!mod && (typing(event.target) || event.altKey)) return
      const handler = latest.current[name]
      if (handler) {
        event.preventDefault()
        handler(event)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
}
