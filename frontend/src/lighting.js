// Light-direction math for the de-light pass.
//
// The key light is a direction in *model* space, not relative to the camera:
// the sampler has to remove the same light the preview draws, and a
// camera-relative light would swing around every time the user orbits — the
// same model would then convert differently depending on where the camera
// happened to be. glTF and three.js share a Y-up right-handed frame, so the
// vector crosses to the backend with no conversion at either end. The
// settings form shows it as azimuth and elevation.

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
