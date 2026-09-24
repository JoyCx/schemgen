// Layout only: the hooks own the state, the components draw it.
//
//   header    — health, palette, theme
//   workspace — viewport + materials | settings   (tabs below 900 px)
//   queue     — the models and their jobs

import { useCallback, useState } from 'react'
import { Eye, SlidersHorizontal } from 'lucide-react'
import { Header } from './components/Header'
import { Viewport } from './components/Viewport'
import { VIEW_MODES } from './viewport/modes'
import { MaterialList } from './components/MaterialList'
import { SettingsForm } from './components/SettingsForm'
import { JobQueue } from './components/JobQueue'
import { PaletteDialog } from './components/PaletteDialog'
import { DropOverlay } from './components/DropOverlay'
import { useServer, usePalette } from './hooks/useServer'
import { useSettings } from './hooks/useSettings'
import { useConversion } from './hooks/useConversion'
import { usePreview } from './hooks/usePreview'
import { useTheme } from './hooks/useTheme'
import { useShortcuts } from './hooks/useShortcuts'
import { useMediaQuery } from './hooks/useMediaQuery'
import { useToasts } from './toastContext'
import { AUTO_SAVE } from './settings'
import type { ViewMode } from './viewport/engine'

export default function App() {
  const server = useServer()
  const { schema } = server
  const { settings, set, setMany, reset } = useSettings(schema)
  const target = settings?.target as string | undefined
  const palette = usePalette(target)
  const notify = useToasts()
  const conversion = useConversion({ schema, settings, notify })
  const selected = conversion.selected
  const preview = usePreview(selected?.file ?? null, settings, schema)
  const theme = useTheme()
  const narrow = useMediaQuery('(max-width: 899px)')
  const [pane, setPane] = useState<'view' | 'settings'>('view')
  const [mode, setMode] = useState<ViewMode>('model')
  const [paletteOpen, setPaletteOpen] = useState(false)

  useShortcuts({
    'mod+enter': () => conversion.convert(),
    ...Object.fromEntries(VIEW_MODES.map((m) => [m.shortcut, () => setMode(m.key)])),
  })

  const onLight = useCallback(
    (direction: [number, number, number]) => set('light_dir', direction),
    [set],
  )
  const onModelError = useCallback((message: string) => notify('error', message), [notify])

  // The finished list when the selected model has one for these settings;
  // otherwise the preview's.
  const result =
    selected?.status === 'done' && !conversion.isStale(selected) ? selected.view?.result : null
  const materials = result?.materials ?? preview.preview?.materials ?? null
  const outputDir = settings?.[AUTO_SAVE] ? String(settings.output_dir ?? '').trim() : ''

  const viewPane = (
    <section className="pane pane--view" aria-label="Preview">
      <Viewport
        file={selected?.file ?? null}
        preview={preview.preview}
        previewLoading={preview.loading}
        previewError={preview.error}
        onCancelPreview={preview.cancel}
        palette={palette}
        settings={settings}
        mode={mode}
        onMode={setMode}
        theme={theme.resolved}
        onLight={onLight}
        onModelError={onModelError}
      />
      {materials && materials.length > 0 && (
        <MaterialList
          materials={materials}
          source={result ? 'final' : 'preview'}
          title={`Materials for ${selected?.view?.download_name ?? selected?.file.name ?? 'the model'}${target ? ` (Minecraft ${target})` : ''}`}
          palette={palette}
        />
      )}
    </section>
  )

  const settingsPane = (
    <section className="pane pane--settings" aria-label="Settings">
      {schema && settings ? (
        <SettingsForm
          schema={schema}
          settings={settings}
          onChange={set}
          onChangeMany={setMany}
          onReset={reset}
          fileManager={server.fileManager}
        />
      ) : (
        <p className="pane-empty">
          {server.online === false ? 'Waiting for the server…' : 'Loading settings…'}
        </p>
      )}
    </section>
  )

  return (
    <div className="app">
      <Header
        server={server}
        target={target}
        theme={theme.choice}
        onCycleTheme={theme.cycle}
        onShowPalette={() => setPaletteOpen(true)}
      />
      {narrow && (
        <nav className="pane-tabs segmented" aria-label="Panes">
          <button
            type="button"
            className={pane === 'view' ? 'is-on' : ''}
            onClick={() => setPane('view')}
          >
            <Eye size={16} aria-hidden /> Preview
          </button>
          <button
            type="button"
            className={pane === 'settings' ? 'is-on' : ''}
            onClick={() => setPane('settings')}
          >
            <SlidersHorizontal size={16} aria-hidden /> Settings
          </button>
        </nav>
      )}
      <main className="workspace">
        {(!narrow || pane === 'view') && viewPane}
        {(!narrow || pane === 'settings') && settingsPane}
      </main>
      <JobQueue
        conversion={conversion}
        canConvert={!!schema && !!settings && server.online !== false}
        outputDir={outputDir}
        fileManager={server.fileManager}
      />
      <PaletteDialog
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        palette={palette}
        target={target}
      />
      <DropOverlay onFiles={conversion.addFiles} />
    </div>
  )
}
