import { describe, expect, it } from 'vitest'
import {
  AUTO_SAVE,
  conversionKey,
  decimals,
  defaultsFrom,
  formatNumber,
  isShown,
  loadRemembered,
  saveRemembered,
  toApiSettings,
} from './settings'
import { schema } from './test/fixtures'

const field = (key: string) => schema.fields.find((f) => f.key === key)!

describe('settings', () => {
  it('starts every field at the schema default', () => {
    const s = defaultsFrom(schema)
    expect(s.max_size).toBe(128)
    expect(s.target).toBe(schema.default_target)
    expect(s.format).toBe('litematic')
    expect(s[AUTO_SAVE]).toBe(false)
    expect(s.output_dir).toBe('')
  })

  it('shows a field only while its condition holds', () => {
    const s = defaultsFrom(schema)
    expect(isShown(field('dither'), s)).toBe(true)
    expect(isShown(field('dither'), { ...s, color_sampling: false })).toBe(false)
    expect(isShown(field('default_block'), { ...s, color_sampling: false })).toBe(true)
  })

  it('builds the API object: numbers, nulls and the folder only when saving', () => {
    const s = {
      ...defaultsFrom(schema),
      max_size: '64.4',
      voxel_size: '',
      output_dir: '~/schematics',
    }
    const preview = toApiSettings(s, schema)
    expect(preview.max_size).toBe(64)
    expect(preview.voxel_size).toBeNull()
    expect('output_dir' in preview).toBe(false)
    expect('threads' in preview).toBe(false)

    const job = toApiSettings(s, schema, { job: true })
    expect(job.output_dir).toBeNull()
    expect(job.threads).toBe(4)
    expect(toApiSettings({ ...s, [AUTO_SAVE]: true }, schema, { job: true }).output_dir).toBe(
      '~/schematics',
    )
    expect(toApiSettings({ ...s, voxel_size: '0' }, schema).voxel_size).toBeNull()
    expect(toApiSettings({ ...s, voxel_size: '0.25' }, schema).voxel_size).toBe(0.25)
  })

  it('keeps a direction a direction', () => {
    const s = defaultsFrom(schema)
    expect(toApiSettings({ ...s, light_dir: [0, 1, 0] }, schema).light_dir).toEqual([0, 1, 0])
    expect(toApiSettings({ ...s, light_dir: 'nonsense' }, schema).light_dir).toEqual(
      field('light_dir').default,
    )
  })

  it('changes the conversion key only for settings that change the schematic', () => {
    const s = defaultsFrom(schema)
    const key = conversionKey(s, schema)
    expect(conversionKey({ ...s, threads: 9 }, schema)).toBe(key)
    expect(conversionKey({ ...s, output_dir: '/x', [AUTO_SAVE]: true }, schema)).toBe(key)
    expect(conversionKey({ ...s, max_size: 64 }, schema)).not.toBe(key)
    expect(conversionKey({ ...s, target: '1.20.4' }, schema)).not.toBe(key)
  })

  it('remembers folder, version and format, and forgets choices the server dropped', () => {
    saveRemembered({
      ...defaultsFrom(schema),
      output_dir: '/mc',
      [AUTO_SAVE]: true,
      target: '1.20.4',
      max_size: 7,
    })
    expect(loadRemembered(schema)).toEqual({
      output_dir: '/mc',
      [AUTO_SAVE]: true,
      target: '1.20.4',
      format: 'litematic',
    })
    localStorage.setItem('schemgen2.output', JSON.stringify({ target: '0.9', format: 'litematic' }))
    expect(loadRemembered(schema)).toEqual({ format: 'litematic' })
  })

  it('formats numbers to their step', () => {
    expect(decimals(0.01)).toBe(2)
    expect(decimals(1)).toBe(0)
    expect(formatNumber(0.3199999928474426, { step: 0.01, type: 'float' })).toBe('0.32')
    expect(formatNumber(127.6, { type: 'int' })).toBe('128')
    expect(formatNumber('', { type: 'float' })).toBe('0')
  })
})
