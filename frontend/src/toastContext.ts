import { createContext, useContext } from 'react'

export type ToastKind = 'error' | 'success' | 'info'
export type Notify = (kind: ToastKind, text: string) => void

export const ToastContext = createContext<Notify>(() => {})

/** `notify(kind, text)` from anywhere under `<ToastProvider>`. */
export const useToasts = () => useContext(ToastContext)
