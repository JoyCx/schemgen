// The key light: stored as a model-space direction, edited as azimuth and
// elevation (and by dragging the handle in the Model view).

import type { ReactNode } from 'react'
import { lightAngles, lightVector } from '../../lighting.js'
import type { Field } from '../../types'

interface Props {
  field: Field
  value: unknown
  onChange: (key: string, value: unknown) => void
  help: ReactNode
}

export function DirectionField({ field, value, onChange, help }: Props) {
  const vector = (
    Array.isArray(value) && value.length === 3 ? value.map(Number) : field.default
  ) as [number, number, number]
  const { azimuth, elevation } = lightAngles(vector)
  const set = (az: number, el: number) => onChange(field.key, lightVector(az, el))

  return (
    <div className="field field--direction">
      <span className="field-label">{field.label}</span>
      <label className="direction-row">
        <span>Azimuth</span>
        <input
          className="range"
          type="range"
          min={-180}
          max={180}
          step={1}
          value={Math.round(azimuth)}
          onChange={(e) => set(Number(e.target.value), elevation)}
        />
        <output>{Math.round(azimuth)}°</output>
      </label>
      <label className="direction-row">
        <span>Elevation</span>
        <input
          className="range"
          type="range"
          min={-90}
          max={90}
          step={1}
          value={Math.round(elevation)}
          onChange={(e) => set(azimuth, Number(e.target.value))}
        />
        <output>{Math.round(elevation)}°</output>
      </label>
      {help}
      <p className="field-help">Or drag the amber handle in the Model view.</p>
    </div>
  )
}
