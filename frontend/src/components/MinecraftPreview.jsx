import { useEffect, useRef, useState } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'

const textureCache = new Map()
const textureLoader = new THREE.TextureLoader()

// Block IDs whose texture file has a different name. Waxed copper and hyphae
// reuse other blocks' textures and several blocks only ship face textures.
const TEXTURE_ALIASES = {
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

function textureFileFor(id) {
  if (TEXTURE_ALIASES[id]) return TEXTURE_ALIASES[id]
  let file = id.replace(/^waxed_/, '') // waxed copper reuses unwaxed art
  file = file.replace(/(^|_)hyphae$/, '$1stem') // hyphae reuse stem textures
  return TEXTURE_ALIASES[file] || file
}

function fallbackColor(name, palette) {
  const rgb = palette?.[name.replace(/^minecraft:/, '')]
  if (rgb) return new THREE.Color(rgb[0] / 255, rgb[1] / 255, rgb[2] / 255)
  return new THREE.Color(0.7, 0.7, 0.7)
}

/// Material that shows the block texture when it loads, and falls back to the
/// flat palette color when it doesn't — a missing texture must never render
/// as a black box. The texture is NOT tinted by the palette color: tinting
/// multiplies the two and darkens every block.
function blockMaterial(name, palette) {
  const id = name.replace(/^minecraft:/, '')
  const material = new THREE.MeshStandardMaterial({
    color: fallbackColor(name, palette),
    roughness: 0.9,
  })
  const file = textureFileFor(id)
  const cached = textureCache.get(file)
  if (cached !== undefined) {
    if (cached !== null) {
      material.map = cached
      material.color.set(0xffffff)
    }
    return material
  }
  textureLoader.load(
    `https://assets.mcasset.cloud/1.21.5/assets/minecraft/textures/block/${file}.png`,
    (texture) => {
      texture.colorSpace = THREE.SRGBColorSpace
      texture.magFilter = THREE.NearestFilter
      texture.minFilter = THREE.NearestFilter
      textureCache.set(file, texture)
      material.map = texture
      material.color.set(0xffffff)
      material.needsUpdate = true
    },
    undefined,
    () => {
      // 404 or network failure — remember it and keep the palette color.
      textureCache.set(file, null)
    },
  )
  return material
}

export default function MinecraftPreview({ preview, palette, loading, error, onCancel }) {
  const mountRef = useRef(null)
  const [renderError, setRenderError] = useState('')
  const [stats, setStats] = useState('')
  const [elapsed, setElapsed] = useState(0)

  // Tick elapsed seconds while loading
  useEffect(() => {
    if (!loading) {
      setElapsed(0)
      return
    }
    setElapsed(0)
    const t = setInterval(() => setElapsed((s) => s + 1), 1000)
    return () => clearInterval(t)
  }, [loading])

  useEffect(() => {
    if (!preview || !mountRef.current) return undefined
    let frame = 0
    let renderer
    let observer
    let controls
    const mount = mountRef.current

    try {
      const scene = new THREE.Scene()
      scene.background = new THREE.Color(0x9ec6df)
      const camera = new THREE.PerspectiveCamera(38, 1, 0.01, 2000)
      renderer = new THREE.WebGLRenderer({ antialias: true })
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
      renderer.outputColorSpace = THREE.SRGBColorSpace
      controls = new OrbitControls(camera, renderer.domElement)
      controls.enableDamping = true
      controls.autoRotate = true
      controls.autoRotateSpeed = 0.55
      controls.minDistance = 0.5
      controls.maxDistance = 12
      controls.target.set(0, 0, 0)
      controls.update()
      mount.replaceChildren(renderer.domElement)
      scene.add(new THREE.HemisphereLight(0xffffff, 0x52657a, 2.2))
      const sun = new THREE.DirectionalLight(0xffffff, 2.8)
      sun.position.set(4, 8, 5)
      scene.add(sun)

      // Packed as x, y, z, palette index per block (see api.js decodeBlocks).
      const blocks = preview.blocks || new Int32Array(0)
      const names = preview.palette || []
      const dims = preview.dims || [1, 1, 1]
      const count = blocks.length / 4
      const group = new THREE.Group()
      scene.add(group)
      const perBlock = new Array(names.length).fill(0)
      for (let i = 3; i < blocks.length; i += 4) perBlock[blocks[i]]++
      const geometry = new THREE.BoxGeometry(0.98, 0.98, 0.98)
      const center = new THREE.Vector3(dims[0] / 2, dims[1] / 2, dims[2] / 2)
      const meshes = names.map((name, index) => {
        if (!perBlock[index]) return null
        const mesh = new THREE.InstancedMesh(
          geometry,
          blockMaterial(name, palette),
          perBlock[index],
        )
        mesh.frustumCulled = false
        group.add(mesh)
        return mesh
      })
      const filled = new Array(names.length).fill(0)
      const matrix = new THREE.Matrix4()
      for (let i = 0; i < blocks.length; i += 4) {
        const index = blocks[i + 3]
        matrix.makeTranslation(
          blocks[i] + 0.5 - center.x,
          blocks[i + 1] + 0.5 - center.y,
          blocks[i + 2] + 0.5 - center.z,
        )
        meshes[index].setMatrixAt(filled[index]++, matrix)
      }
      for (const mesh of meshes) if (mesh) mesh.instanceMatrix.needsUpdate = true
      group.scale.setScalar(3.8 / Math.max(dims[0], dims[1], dims[2], 1))
      camera.position.set(4.8, 3.2, 4.8)
      camera.lookAt(0, 0, 0)
      const kinds = perBlock.filter(Boolean).length
      setStats(`${count.toLocaleString()} blocks · ${kinds} block types`)
      setRenderError('')

      const resize = () => {
        const width = mount.clientWidth || 640
        const height = mount.clientHeight || 390
        camera.aspect = width / height
        camera.updateProjectionMatrix()
        renderer.setSize(width, height, false)
      }
      observer = new ResizeObserver(resize)
      observer.observe(mount)
      resize()
      const animate = () => {
        frame = requestAnimationFrame(animate)
        controls.update()
        renderer.render(scene, camera)
      }
      animate()
    } catch (e) {
      setRenderError(e?.message || 'Could not render the Minecraft preview')
    }
    return () => {
      cancelAnimationFrame(frame)
      observer?.disconnect()
      controls?.dispose()
      renderer?.dispose()
      mount.replaceChildren()
    }
  }, [preview, palette])

  return (
    <div className="panel minecraft-preview-panel">
      <div className="preview-heading">
        <div>
          <h3>Minecraft Preview</h3>
          <p>
            Drag to orbit · wheel to zoom · right-drag to pan. Built by the same pipeline as the
            conversion.
          </p>
        </div>
        <span className="preview-status">
          {loading ? `Building… ${elapsed}s` : stats}
          {loading && onCancel && (
            <button className="preview-cancel" onClick={onCancel}>
              Cancel
            </button>
          )}
        </span>
      </div>
      <div ref={mountRef} className="minecraft-preview" aria-label="Minecraft schematic preview">
        {!preview && !loading && <span className="preview-muted">Preview will appear here.</span>}
        {(error || renderError) && <span className="preview-error">{error || renderError}</span>}
        {loading && !preview && <span className="preview-muted">Building preview schematic…</span>}
      </div>
    </div>
  )
}
