// Default conversion settings — the one place the web UI defines them.
//
// These mirror the backend's own defaults (`ConversionOptions` and
// `LightingOptions` in backend/src/types.rs), so a field the UI leaves at its
// default converts exactly as if it had not been sent at all.

import { LIGHT_DEFAULTS, lightingParams } from './lighting.js'

export const SETTINGS_DEFAULTS = {
  max_size: '128',
  voxel_size: '',
  ram_limit: '4.0',
  threads: '4',
  dither: true,
  color_sampling: true,
  brightness: 0,
  contrast: 1,
  saturation: 1,
  no_color_block: 'white',
  schematic_name: '',
  ...LIGHT_DEFAULTS,
}

// The multipart fields the conversion routes read, built from a settings
// object. Anything the object leaves out falls back to SETTINGS_DEFAULTS.
export function conversionFields(settings = {}) {
  const s = { ...SETTINGS_DEFAULTS, ...settings }
  return {
    max_size: s.max_size,
    voxel_size: s.voxel_size,
    ram_limit: s.ram_limit,
    dither: s.dither ? 'true' : 'false',
    color_sampling: s.color_sampling ? 'true' : 'false',
    brightness: s.brightness,
    contrast: s.contrast,
    saturation: s.saturation,
    no_color_block: s.no_color_block,
    schematic_name: s.schematic_name,
    ...lightingParams(s),
  }
}
