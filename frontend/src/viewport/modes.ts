import { Box, Boxes, Columns2 } from 'lucide-react'
import type { ViewMode } from './engine'

/** The viewport's tabs and their keyboard shortcuts. */
export const VIEW_MODES: { key: ViewMode; label: string; icon: typeof Box; shortcut: string }[] = [
  { key: 'model', label: 'Model', icon: Box, shortcut: '1' },
  { key: 'blocks', label: 'Blocks', icon: Boxes, shortcut: '2' },
  { key: 'split', label: 'Split', icon: Columns2, shortcut: '3' },
]
