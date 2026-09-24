import { describe, expect, it } from 'vitest'
import { decodeBlocks, withToken } from './api'

describe('api', () => {
  it('decodes packed preview blocks', () => {
    const blocks = new Int32Array([1, 2, 3, 0, -1, 5, 6, 7])
    const base64 = btoa(String.fromCharCode(...new Uint8Array(blocks.buffer)))
    expect([...decodeBlocks(base64)]).toEqual([...blocks])
  })

  it('leaves URLs alone without a token', () => {
    expect(withToken('/api/jobs/1/download')).toBe('/api/jobs/1/download')
  })
})
