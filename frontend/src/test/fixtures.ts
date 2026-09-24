import schemaJson from './schema.json'
import type { JobView, Schema } from '../types'

/** GET /api/schema as a real server answered it. */
export const schema = schemaJson as unknown as Schema

export function model(name = 'castle.glb', size = 1024): File {
  return new File([new Uint8Array(size)], name, { type: 'model/gltf-binary', lastModified: 1 })
}

export function jobView(id: string, change: Partial<JobView> = {}): JobView {
  return {
    id,
    status: 'running',
    progress: 50,
    stage: 'match',
    message: 'Matching blocks…',
    input_name: 'castle.glb',
    name: 'castle',
    download_name: 'castle.litematic',
    format: 'litematic',
    target: '1.21.8',
    created_ms: 0,
    finished_ms: null,
    error: null,
    result: null,
    saved_path: null,
    save_error: null,
    links: { download: '', thumbnail: '', events: '' },
    ...change,
  }
}
