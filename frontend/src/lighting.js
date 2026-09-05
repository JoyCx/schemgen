// Shared light-direction math for the de-light pass.
//
// The key light is stored as azimuth/elevation in *model* space rather than as
// a camera-relative vector. The sampler has to remove the same light the
// preview draws, and a camera-relative light would swing around every time the
// user orbits — the same model would then convert differently depending on
// where the camera happened to be. glTF and three.js share a Y-up
// right-handed frame, so the vector built here crosses to the backend with no
// conversion at either end.

const DEG = Math.PI / 180

// Azimuth is measured around +Y, from +Z toward +X; elevation is the angle
// above the XZ plane.
export function lightVector(azimuthDeg, elevationDeg) {
  const az = Number(azimuthDeg) * DEG
  const el = Number(elevationDeg) * DEG
  const flat = Math.cos(el)
  return [flat * Math.sin(az), Math.sin(el), flat * Math.cos(az)]
}

export function lightAngles([x, y, z]) {
  const len = Math.hypot(x, y, z) || 1
  return {
    azimuth: Math.atan2(x / len, z / len) / DEG,
    elevation: Math.asin(Math.min(1, Math.max(-1, y / len))) / DEG,
  }
}

// Mirrors LightingOptions::default() in backend/src/types.rs, which in turn
// mirrors DEFAULT_LIGHTING in backend/scripts/sample_colors.py. The angles here
// are that default direction (0.35, 0.85, 0.40) written as two numbers a slider
// can hold.
export const LIGHT_DEFAULTS = {
  light_azimuth: 41,
  light_elevation: 58,
  light_ambient: 0.32,
  light_gloss: 0.5,
  specular: 1.1,
  highlight_rejection: 0.75,
  highlight_recovery: 1,
  delight: 0,
}

// The multipart fields the backend reads, built from a settings object.
export function lightingParams(s) {
  const [x, y, z] = lightVector(s.light_azimuth, s.light_elevation)
  return {
    light_dir: `${x.toFixed(5)},${y.toFixed(5)},${z.toFixed(5)}`,
    light_ambient: s.light_ambient,
    light_gloss: s.light_gloss,
    specular: s.specular,
    highlight_rejection: s.highlight_rejection,
    highlight_recovery: s.highlight_recovery,
    delight: s.delight,
  }
}
