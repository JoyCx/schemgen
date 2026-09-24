// The Blocks view: a preview's block grid as instanced cubes, textured.
//
// Textures come from the server — the user's own Minecraft install (see
// GET /api/textures) — then from a public copy, and a block with neither is
// drawn in its flat palette color. A missing texture never renders black.

import * as THREE from 'three'
import { blockTextureUrl } from '../api'
import type { PaletteColors, Preview } from '../types'

const CDN = 'https://assets.mcasset.cloud/1.21.5/assets/minecraft/textures/block'

// Block IDs whose texture file has a different name. Waxed copper and hyphae
// reuse other blocks' textures, and several blocks only ship face textures.
const TEXTURE_ALIASES: Record<string, string> = {
  snow_block: 'snow',
  magma_block: 'magma',
  quartz_block: 'quartz_block_side',
  smooth_quartz: 'quartz_block_bottom',
  smooth_sandstone: 'sandstone_top',
  smooth_red_sandstone: 'red_sandstone_top',
  basalt: 'basalt_side',
  polished_basalt: 'polished_basalt_side',
  bone_block: 'bone_block_side',
  ochre_froglight: 'ochre_froglight_side',
  verdant_froglight: 'verdant_froglight_side',
  pearlescent_froglight: 'pearlescent_froglight_side',
  muddy_mangrove_roots: 'muddy_mangrove_roots_side',
  reinforced_deepslate: 'reinforced_deepslate_side',
  grass_block: 'grass_block_side',
}

/** The texture file (stem) a block ID is drawn with. */
export function textureFileFor(blockId: string): string {
  const id = blockId.replace(/^minecraft:/, '')
  if (TEXTURE_ALIASES[id]) return TEXTURE_ALIASES[id]
  let file = id.replace(/^waxed_/, '') // waxed copper reuses unwaxed art
  file = file.replace(/(^|_)hyphae$/, '$1stem') // hyphae reuse stem textures
  return TEXTURE_ALIASES[file] || file
}

const loader = new THREE.TextureLoader()
const cache = new Map<string, Promise<THREE.Texture | null>>()

function load(url: string): Promise<THREE.Texture> {
  return new Promise((resolve, reject) => loader.load(url, resolve, undefined, reject))
}

/** A block texture, from the server or else the CDN; `null` if neither has it. */
export function blockTexture(file: string): Promise<THREE.Texture | null> {
  let pending = cache.get(file)
  if (!pending) {
    pending = load(blockTextureUrl(file))
      .catch(() => load(`${CDN}/${file}.png`))
      .then((texture) => {
        texture.colorSpace = THREE.SRGBColorSpace
        texture.magFilter = THREE.NearestFilter
        texture.minFilter = THREE.NearestFilter
        return texture
      })
      .catch(() => null)
    cache.set(file, pending)
  }
  return pending
}

export function paletteColor(blockId: string, palette: PaletteColors | null): THREE.Color {
  const rgb = palette?.[blockId.replace(/^minecraft:/, '')]
  if (rgb)
    return new THREE.Color().setRGB(rgb[0] / 255, rgb[1] / 255, rgb[2] / 255, THREE.SRGBColorSpace)
  return new THREE.Color(0.7, 0.7, 0.7)
}

/** The block grid of a preview, in model space: block (x, y, z) sits at
 *  `origin + (x, y, z) · pitch`, so it overlays the model exactly. */
export class BlockLayer {
  readonly group = new THREE.Group()
  private meshes: THREE.InstancedMesh[] = []
  private geometry: THREE.BoxGeometry | null = null
  private disposed = false
  /** Called when a texture arrives and the view needs redrawing. */
  onChange: () => void = () => {}

  /** Model-space bounds of the grid, for framing when there is no model. */
  bounds = new THREE.Box3()

  set(preview: Preview | null, palette: PaletteColors | null): void {
    this.clear()
    if (!preview || !preview.palette.length) return
    const { blocks, palette: names, origin, pitch, dims } = preview
    const perBlock = new Array(names.length).fill(0)
    for (let i = 3; i < blocks.length; i += 4) perBlock[blocks[i]]++

    this.geometry = new THREE.BoxGeometry(pitch * 0.98, pitch * 0.98, pitch * 0.98)
    const byIndex: (THREE.InstancedMesh | null)[] = names.map((name, index) => {
      if (!perBlock[index]) return null
      const material = new THREE.MeshStandardMaterial({
        color: paletteColor(name, palette),
        roughness: 0.9,
      })
      const mesh = new THREE.InstancedMesh(this.geometry!, material, perBlock[index])
      mesh.frustumCulled = false
      this.group.add(mesh)
      this.meshes.push(mesh)
      // The texture is not tinted by the palette color: tinting multiplies
      // the two and darkens every block.
      blockTexture(textureFileFor(name)).then((texture) => {
        if (!texture || this.disposed || !this.meshes.includes(mesh)) return
        material.map = texture
        material.color.set(0xffffff)
        material.needsUpdate = true
        this.onChange()
      })
      return mesh
    })

    const filled = new Array(names.length).fill(0)
    const matrix = new THREE.Matrix4()
    for (let i = 0; i < blocks.length; i += 4) {
      const index = blocks[i + 3]
      matrix.makeTranslation(
        origin[0] + (blocks[i] + 0.5) * pitch,
        origin[1] + (blocks[i + 1] + 0.5) * pitch,
        origin[2] + (blocks[i + 2] + 0.5) * pitch,
      )
      byIndex[index]?.setMatrixAt(filled[index]++, matrix)
    }
    for (const mesh of this.meshes) mesh.instanceMatrix.needsUpdate = true

    this.bounds.set(
      new THREE.Vector3(...origin),
      new THREE.Vector3(
        origin[0] + dims[0] * pitch,
        origin[1] + dims[1] * pitch,
        origin[2] + dims[2] * pitch,
      ),
    )
  }

  clear(): void {
    for (const mesh of this.meshes) {
      this.group.remove(mesh)
      ;(mesh.material as THREE.Material).dispose()
      mesh.dispose()
    }
    this.meshes = []
    this.geometry?.dispose()
    this.geometry = null
    this.bounds.makeEmpty()
  }

  dispose(): void {
    this.disposed = true
    this.clear()
  }
}
