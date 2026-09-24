// Toasts: errors and confirmations that do not belong to one spot on the page.

import { useCallback, useMemo, useState, type ReactNode } from 'react'
import { AlertTriangle, CheckCircle2, Info, X } from 'lucide-react'

import { ToastContext, type Notify, type ToastKind } from './toastContext'

interface Toast {
  id: number
  kind: ToastKind
  text: string
}

const LIFETIME: Record<ToastKind, number> = { error: 9000, success: 4000, info: 5000 }
const ICON = { error: AlertTriangle, success: CheckCircle2, info: Info }

let nextId = 1

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([])

  const dismiss = useCallback((id: number) => {
    setToasts((all) => all.filter((t) => t.id !== id))
  }, [])

  const notify = useCallback<Notify>(
    (kind, text) => {
      const id = nextId++
      // The same message twice in a row is one toast, not a stack of them.
      setToasts((all) => [...all.filter((t) => t.text !== text).slice(-3), { id, kind, text }])
      setTimeout(() => dismiss(id), LIFETIME[kind])
    },
    [dismiss],
  )

  const value = useMemo(() => notify, [notify])
  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => {
          const Icon = ICON[t.kind]
          return (
            <div
              key={t.id}
              className={`toast toast--${t.kind}`}
              role={t.kind === 'error' ? 'alert' : undefined}
            >
              <Icon size={18} aria-hidden />
              <span className="toast-text">{t.text}</span>
              <button
                type="button"
                className="icon-button"
                onClick={() => dismiss(t.id)}
                aria-label="Dismiss"
              >
                <X size={16} />
              </button>
            </div>
          )
        })}
      </div>
    </ToastContext.Provider>
  )
}
