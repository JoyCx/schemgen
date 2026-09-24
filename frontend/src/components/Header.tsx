// The header: what this is, whether the server is there, and the theme.

import { Boxes, Monitor, Moon, Palette, Sun } from 'lucide-react'
import type { ThemeChoice } from '../hooks/useTheme'
import type { ServerState } from '../hooks/useServer'

interface Props {
  server: ServerState
  target: string | undefined
  theme: ThemeChoice
  onCycleTheme: () => void
  onShowPalette: () => void
}

const THEME_ICON = { system: Monitor, light: Sun, dark: Moon }
const THEME_LABEL = {
  system: 'Theme: follow the system',
  light: 'Theme: light',
  dark: 'Theme: dark',
}

export function Header({ server, target, theme, onCycleTheme, onShowPalette }: Props) {
  const { health, online, schema } = server
  const blocks = schema?.targets.find((t) => t.id === target)?.blocks ?? health?.palette_blocks
  const ThemeIcon = THEME_ICON[theme]
  const status = online === null ? 'checking' : online ? 'ok' : 'down'
  return (
    <header className="app-header">
      <div className="brand">
        <Boxes size={22} aria-hidden />
        <span>
          SchemGen<em>2</em>
        </span>
      </div>
      <div
        className={`health health--${status}`}
        role="status"
        title={
          health
            ? `schemgen2 ${health.version} · voxelizer: ${health.voxelizer ?? 'rust'}`
            : undefined
        }
      >
        <span className="health-dot" aria-hidden />
        {status === 'checking' && 'Connecting…'}
        {status === 'down' && 'Server unreachable'}
        {status === 'ok' && (
          <>
            Server ok
            {target && <span className="health-sep">Minecraft {target}</span>}
            {blocks != null && <span className="health-sep">{blocks} blocks</span>}
          </>
        )}
      </div>
      <div className="header-actions">
        <button
          type="button"
          className="icon-button"
          onClick={onShowPalette}
          title="Block palette"
          aria-label="Block palette"
        >
          <Palette size={18} />
        </button>
        <button
          type="button"
          className="icon-button"
          onClick={onCycleTheme}
          title={THEME_LABEL[theme]}
          aria-label={THEME_LABEL[theme]}
        >
          <ThemeIcon size={18} />
        </button>
      </div>
    </header>
  )
}
