import { useEffect, useRef, useState, useCallback } from 'react'
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
  let file = id.replace(/^waxed_/, '')            // waxed copper reuses unwaxed art
  file = file.replace(/(^|_)hyphae$/, '$1stem')   // hyphae reuse stem textures
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
    texture => {
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
    if (!loading) { setElapsed(0); return }
    setElapsed(0)
    const t = setInterval(() => setElapsed(s => s + 1), 1000)
    return () => clearInterval(t)
  }, [loading])

  useEffect(() => {
    if (!preview || !mountRef.current) return undefined
    let disposed = false
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

      const blocks = preview.blocks || []
      const grid = preview.grid || [1, 1, 1]
      const group = new THREE.Group()
      scene.add(group)
      const grouped = new Map()
      for (const block of blocks) {
        if (!grouped.has(block.name)) grouped.set(block.name, [])
        grouped.get(block.name).push(block)
      }
      const geometry = new THREE.BoxGeometry(0.98, 0.98, 0.98)
      const center = new THREE.Vector3(Number(grid[0]) / 2, Number(grid[1]) / 2, Number(grid[2]) / 2)
      for (const [name, entries] of grouped) {
        const material = blockMaterial(name, palette)
        const mesh = new THREE.InstancedMesh(geometry, material, entries.length)
        const matrix = new THREE.Matrix4()
        entries.forEach((block, index) => {
          matrix.makeTranslation(block.x + 0.5 - center.x, block.y + 0.5 - center.y, block.z + 0.5 - center.z)
          mesh.setMatrixAt(index, matrix)
        })
        mesh.instanceMatrix.needsUpdate = true
        mesh.frustumCulled = false
        group.add(mesh)
      }
      group.scale.setScalar(3.8 / Math.max(Number(grid[0]), Number(grid[1]), Number(grid[2]), 1))
      camera.position.set(4.8, 3.2, 4.8)
      camera.lookAt(0, 0, 0)
      setStats(`${blocks.length.toLocaleString()} blocks · ${grouped.size} block types`)
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
      disposed = true
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
          <p>Drag to orbit · wheel to zoom · right-drag to pan. Uses the same blocks as the verified preview .litematic.</p>
        </div>
        <span className="preview-status">
          {loading ? `Building… ${elapsed}s` : stats}
          {loading && onCancel && (
            <button className="preview-cancel" onClick={onCancel}>Cancel</button>
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