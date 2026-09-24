import { describe, expect, it } from 'vitest'
import { blockLabel, inStacks, materialsText, sortMaterials } from './materials'

describe('materials', () => {
  it('names blocks for people', () => {
    expect(blockLabel('minecraft:waxed_cut_copper')).toBe('Waxed cut copper')
  })

  it('counts in shulker boxes, stacks and blocks', () => {
    expect(inStacks(0)).toBe('0')
    expect(inStacks(63)).toBe('63')
    expect(inStacks(64)).toBe('1 st')
    expect(inStacks(1728 + 64 * 3 + 12)).toBe('1 sb + 3 st + 12')
  })

  it('sorts by count, then by name', () => {
    const list = [
      { name: 'minecraft:stone', count: 5 },
      { name: 'minecraft:andesite', count: 9 },
      { name: 'minecraft:dirt', count: 5 },
    ]
    expect(sortMaterials(list, 'count').map((m) => m.name)).toEqual([
      'minecraft:andesite',
      'minecraft:dirt',
      'minecraft:stone',
    ])
    expect(sortMaterials(list, 'name')[0].name).toBe('minecraft:andesite')
  })

  it('writes an aligned plain-text list', () => {
    const text = materialsText(
      [
        { name: 'minecraft:stone', count: 1240 },
        { name: 'minecraft:oak_planks', count: 7 },
      ],
      'Materials for castle',
    )
    expect(text.split('\n')).toEqual([
      'Materials for castle',
      '1,240  Stone       19 st + 24',
      '    7  Oak planks  7',
      '1,247 blocks, 2 kinds (sb = shulker box, st = stack of 64)',
    ])
  })
})
