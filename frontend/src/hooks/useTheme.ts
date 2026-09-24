// Light, dark, or whatever the system prefers — remembered per browser.

import { useCallback, useEffect, useState, useSyncExternalStore } from 'react'

export type ThemeChoice = 'system' | 'light' | 'dark'
const KEY = 'schemgen2.theme'
const QUERY = '(prefers-color-scheme: dark)'

function readChoice(): ThemeChoice {
  try {
    const saved = localStorage.getItem(KEY)
    return saved === 'light' || saved === 'dark' ? saved : 'system'
  } catch {
    return 'system'
  }
}

function subscribe(onChange: () => void) {
  const media = window.matchMedia?.(QUERY)
  media?.addEventListener?.('change', onChange)
  return () => media?.removeEventListener?.('change', onChange)
}

const systemDark = () => !!window.matchMedia?.(QUERY).matches

export function useTheme() {
  const [choice, setChoice] = useState<ThemeChoice>(readChoice)
  const prefersDark = useSyncExternalStore(subscribe, systemDark, () => false)
  const resolved: 'light' | 'dark' = choice === 'system' ? (prefersDark ? 'dark' : 'light') : choice

  useEffect(() => {
    document.documentElement.dataset.theme = resolved
    try {
      localStorage.setItem(KEY, choice)
    } catch {
      /* not worth surfacing */
    }
  }, [choice, resolved])

  /** system → light → dark → system. */
  const cycle = useCallback(() => {
    setChoice((c) => (c === 'system' ? 'light' : c === 'light' ? 'dark' : 'system'))
  }, [])

  return { choice, resolved, setChoice, cycle }
}
