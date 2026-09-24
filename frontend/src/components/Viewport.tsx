// The viewport: Model, Blocks and Split views of the selected model, one
// renderer, one camera.

import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { Boxes, Focus, Loader2, RotateCw, X } from 'lucide-react'
import { ViewportEngine, type ViewMode } from '../viewport/engine'
import { VIEW_MODES } from '../viewport/modes'
import type { ModelLook } from '../viewport/model'
import type { PaletteColors, Preview } from '../types'
import type { Settings } from '../settings'

const LOOKS: { key: ModelLook; label: string; hint: string }[] = [
  { key: 'lit', label: 'Lit', hint: 'The model as its materials describe it.' },
  {
    key: 'albedo',
    label: 'De-lit',
    hint: 'What the color sampler will read once the key light is removed.',
  },
  { key: 'mask', label: 'Rejection', hint: 'Red marks surface the highlight pass is discounting.' },
]

interface Props {
  file: File | null
  preview: Preview | null
  previewLoading: boolean
  previewError: string
  onCancelPreview: () => void
  palette: PaletteColors | null
  settings: Settings | null
  mode: ViewMode
  onMode: (mode: ViewMode) => void
  theme: 'light' | 'dark'
  onLight: (direction: [number, number, number]) => void
  onModelError: (message: string) => void
}

export function Viewport(props: Props) {
  const { file, preview, palette, settings, mode, theme } = props
  const hostRef = useRef<HTMLDivElement>(null)
  const engineRef = useRef<ViewportEngine | null>(null)
  const [look, setLook] = useState<ModelLook>('lit')
  const [split, setSplit] = useState(0.5)
  const [autoRotate, setAutoRotate] = useState(false)
  const events = useRef({ onLight: props.onLight, onModelError: props.onModelError })
  useEffect(() => {
    events.current = { onLight: props.onLight, onModelError: props.onModelError }
  })

  useEffect(() => {
    const host = hostRef.current
    if (!host) return
    let engine: ViewportEngine
    try {
      engine = new ViewportEngine(host, {
        onLight: (direction) => events.current.onLight(direction),
        onModelError: (message) => events.current.onModelError(message),
      })
    } catch {
      events.current.onModelError('This browser cannot draw 3D (WebGL is unavailable)')
      return
    }
    engineRef.current = engine
    return () => {
      engine.dispose()
      engineRef.current = null
    }
  }, [])

  useEffect(() => {
    engineRef.current?.setModel(file)
  }, [file])
  useEffect(() => {
    engineRef.current?.setBlocks(preview, palette)
  }, [preview, palette])
  useEffect(() => {
    engineRef.current?.setMode(mode)
  }, [mode])
  useEffect(() => {
    engineRef.current?.setSplit(split)
  }, [split])
  useEffect(() => {
    engineRef.current?.setLook(look)
  }, [look])
  useEffect(() => {
    engineRef.current?.setLighting(settings)
  }, [settings])
  useEffect(() => {
    engineRef.current?.setAutoRotate(autoRotate)
  }, [autoRotate])
  useEffect(() => {
    engineRef.current?.setTheme(theme)
  }, [theme])

  const dragDivider = (event: ReactPointerEvent<HTMLDivElement>) => {
    const host = hostRef.current
    if (!host) return
    const target = event.currentTarget
    target.setPointerCapture(event.pointerId)
    const move = (e: PointerEvent) => {
      const rect = host.getBoundingClientRect()
      setSplit((e.clientX - rect.left) / rect.width)
    }
    const up = () => {
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', up)
    }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', up)
  }

  const showsModel = mode !== 'blocks'
  const activeLook = LOOKS.find((l) => l.key === look) ?? LOOKS[0]

  return (
    <div className="viewport">
      <div className="viewport-toolbar">
        <div className="segmented" role="tablist" aria-label="View">
          {VIEW_MODES.map(({ key, label, icon: Icon, shortcut }) => (
            <button
              key={key}
              type="button"
              role="tab"
              aria-selected={mode === key}
              className={mode === key ? 'is-on' : ''}
              onClick={() => props.onMode(key)}
              title={`${label} (${shortcut})`}
            >
              <Icon size={16} aria-hidden /> {label}
            </button>
          ))}
        </div>
        {showsModel && (
          <div className="segmented segmented--small" role="group" aria-label="Model shading">
            {LOOKS.map((l) => (
              <button
                key={l.key}
                type="button"
                aria-pressed={look === l.key}
                className={look === l.key ? 'is-on' : ''}
                onClick={() => setLook(l.key)}
                title={l.hint}
              >
                {l.label}
              </button>
            ))}
          </div>
        )}
        <div className="viewport-tools">
          <button
            type="button"
            className={`icon-button${autoRotate ? ' is-on' : ''}`}
            aria-pressed={autoRotate}
            onClick={() => setAutoRotate((on) => !on)}
            title="Turn slowly"
          >
            <RotateCw size={16} />
          </button>
          <button
            type="button"
            className="icon-button"
            onClick={() => engineRef.current?.resetCamera()}
            title="Reset the view"
          >
            <Focus size={16} />
          </button>
        </div>
      </div>

      <div ref={hostRef} className={`viewport-stage viewport-stage--${mode}`}>
        {mode === 'split' && (
          <div
            className="split-divider"
            style={{ left: `${split * 100}%` }}
            onPointerDown={dragDivider}
            role="separator"
            aria-orientation="vertical"
            aria-valuenow={Math.round(split * 100)}
            aria-label="Model and blocks divider"
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === 'ArrowLeft') setSplit((s) => Math.max(0, s - 0.05))
              if (e.key === 'ArrowRight') setSplit((s) => Math.min(1, s + 0.05))
            }}
          >
            <span className="split-handle" />
          </div>
        )}
        {mode === 'split' && (
          <>
            <span className="stage-label stage-label--left">Model</span>
            <span className="stage-label stage-label--right">Blocks</span>
          </>
        )}
        {!file && (
          <div className="stage-empty">
            <Boxes size={40} aria-hidden />
            <p>Drop a .glb or .gltf model anywhere to start.</p>
          </div>
        )}
        {file && mode !== 'model' && (props.previewLoading || props.previewError || !preview) && (
          <div className="stage-status">
            {props.previewLoading ? (
              <>
                <Loader2 size={16} className="spin" aria-hidden /> Building the block preview…
                <button type="button" className="link-button" onClick={props.onCancelPreview}>
                  <X size={14} aria-hidden /> Cancel
                </button>
              </>
            ) : props.previewError ? (
              <span className="text-danger">{props.previewError}</span>
            ) : (
              'The block preview appears here.'
            )}
          </div>
        )}
        {file && mode === 'model' && (
          <p className="stage-hint">
            {activeLook.hint} Drag the amber handle to aim the key light.
          </p>
        )}
        {preview?.capped && mode !== 'model' && (
          <p className="stage-note">
            Preview at {Math.max(...preview.dims)} blocks; the conversion uses the full size.
          </p>
        )}
      </div>
    </div>
  )
}
