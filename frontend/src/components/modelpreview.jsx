import { useEffect, useRef, useState } from 'react'
import * as THREE from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js'
import { lightVector, lightAngles, LIGHT_DEFAULTS } from '../lighting.js'
import { VERTEX_SHADER, FRAGMENT_SHADER } from '../delightshader.js'

const defaults = { brightness: 0, contrast: 1, saturation: 1, ...LIGHT_DEFAULTS }

// The model is fitted to ~2.1 units across, so the handle orbits just outside it.
const GIZMO_RADIUS = 1.9

// Dragging updates the gizmo and the shader every frame; React only needs to
// hear about it often enough to keep the sliders and the backend in step.
const DRAG_COMMIT_MS = 90

const MODES = [
  { key: 'lit', label: 'Lit', hint: 'The model as its material describes it.' },
  { key: 'albedo', label: 'De-lit albedo', hint: 'What the sampler will read once the chosen light is removed.' },
  { key: 'mask', label: 'Rejection mask', hint: 'Red marks surface the highlight pass is discounting.' },
]

export default function ModelPreview({ file, settings, onChange }) {
  const mountRef = useRef(null)
  const canvasRef = useRef(null)
  const sceneRef = useRef(null)
  const settingsRef = useRef(settings)
  const onChangeRef = useRef(onChange)
  const [error, setError] = useState('')
  const [mode, setMode] = useState('lit')
  // Read inside the loader callback, which outlives the render that made it.
  const modeRef = useRef(mode)
  const [showLighting, setShowLighting] = useState(false)
  settingsRef.current = settings
  onChangeRef.current = onChange
  modeRef.current = mode

  useEffect(() => {
    if (!file || !mountRef.current) return undefined

    const mount = mountRef.current
    const scene = new THREE.Scene()
    scene.background = new THREE.Color(0xf4f3ef)
    const camera = new THREE.PerspectiveCamera(35, 1, 0.01, 1000)
    camera.position.set(0, 0.35, 3)

    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true })
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
    renderer.outputColorSpace = THREE.SRGBColorSpace
    renderer.toneMapping = THREE.ACESFilmicToneMapping
    mount.replaceChildren(renderer.domElement)
    canvasRef.current = renderer.domElement
    const controls = new OrbitControls(camera, renderer.domElement)
    controls.enableDamping = true
    controls.autoRotate = true
    controls.autoRotateSpeed = 0.8
    controls.minDistance = 0.5
    controls.maxDistance = 12
    controls.target.set(0, 0, 0)
    controls.update()

    // glTF defaults metallicFactor to 1, and a metal has no diffuse term: its
    // colour is entirely reflected environment. Punctual lights alone leave one
    // black except for pinpoint highlights, so give the scene something to
    // reflect or every metallic model renders as a silhouette.
    const pmrem = new THREE.PMREMGenerator(renderer)
    const envRT = pmrem.fromScene(new RoomEnvironment(), 0.04)
    scene.environment = envRT.texture

    scene.add(new THREE.HemisphereLight(0xffffff, 0x777777, 2.2))
    const key = new THREE.DirectionalLight(0xffffff, 2.5)
    key.position.set(3, 5, 4)
    scene.add(key)
    const fill = new THREE.DirectionalLight(0xb8d4ff, 1.2)
    fill.position.set(-4, 2, -3)
    scene.add(fill)

    // ── Light gizmo ────────────────────────────────────────────────────────
    // The handle sits on a sphere around the model and is dragged along it, so
    // the direction it reports is always a unit vector in model space.
    const gizmo = new THREE.Group()
    const handle = new THREE.Mesh(
      new THREE.SphereGeometry(0.085, 24, 18),
      new THREE.MeshBasicMaterial({ color: 0xffb020 }),
    )
    const stem = new THREE.Line(
      new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(), new THREE.Vector3()]),
      new THREE.LineBasicMaterial({ color: 0xffb020, transparent: true, opacity: 0.45 }),
    )
    gizmo.add(handle, stem)
    scene.add(gizmo)

    const group = new THREE.Group()
    scene.add(group)
    let frame = 0
    const objectUrl = URL.createObjectURL(file)
    let disposed = false

    const state = { renderer, scene, camera, controls, handle, stem, meshes: [] }
    sceneRef.current = state

    const resize = () => {
      const width = mount.clientWidth || 640
      const height = mount.clientHeight || 360
      camera.aspect = width / height
      camera.updateProjectionMatrix()
      renderer.setSize(width, height, false)
    }
    const observer = new ResizeObserver(resize)
    observer.observe(mount)
    resize()

    applyFilters(canvasRef.current, settingsRef.current)
    applyLighting(state, settingsRef.current)

    new GLTFLoader().load(objectUrl, (gltf) => {
      if (disposed) return
      group.add(gltf.scene)
      const box = new THREE.Box3().setFromObject(gltf.scene)
      const size = box.getSize(new THREE.Vector3())
      const center = box.getCenter(new THREE.Vector3())
      gltf.scene.position.sub(center)
      const maxSize = Math.max(size.x, size.y, size.z) || 1
      gltf.scene.scale.setScalar(2.1 / maxSize)
      camera.position.set(0, 0.25, 3.1)
      controls.target.set(0, 0, 0)
      controls.update()

      flattenTransparency(gltf.scene)
      state.meshes = buildDelitMaterials(gltf.scene)
      applyLighting(state, settingsRef.current)
      applyMode(state, modeRef.current)
      setError('')
    }, undefined, (loadError) => {
      if (!disposed) setError(loadError?.message || 'Could not preview this model')
    })

    // ── Dragging the key light ─────────────────────────────────────────────
    const raycaster = new THREE.Raycaster()
    const pointer = new THREE.Vector2()
    const orbit = new THREE.Sphere(new THREE.Vector3(0, 0, 0), GIZMO_RADIUS)
    const scratch = new THREE.Vector3()
    let dragging = false
    let lastCommit = 0

    const setPointer = (event) => {
      const rect = renderer.domElement.getBoundingClientRect()
      pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1
      pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1
      raycaster.setFromCamera(pointer, camera)
    }

    const commit = (direction) => {
      const { azimuth, elevation } = lightAngles(direction.toArray())
      onChangeRef.current({
        ...settingsRef.current,
        light_azimuth: Math.round(azimuth * 10) / 10,
        light_elevation: Math.round(elevation * 10) / 10,
      })
    }

    const onPointerDown = (event) => {
      setPointer(event)
      if (!raycaster.intersectObject(handle, false).length) return
      dragging = true
      controls.enabled = false
      renderer.domElement.setPointerCapture(event.pointerId)
      event.preventDefault()
    }

    const onPointerMove = (event) => {
      if (!dragging) return
      setPointer(event)
      // Past the model's silhouette the ray misses the orbit sphere entirely.
      // The closest point on the ray is then outside it, which still reads as
      // the direction the pointer is aiming at.
      const hit = raycaster.ray.intersectSphere(orbit, scratch)
        || raycaster.ray.closestPointToPoint(orbit.center, scratch)
      const direction = hit.clone().sub(orbit.center)
      if (direction.lengthSq() < 1e-8) return
      direction.normalize()

      // Move the gizmo and the shader now; tell React on a slower cadence so a
      // drag does not queue a re-render (and a re-conversion) per frame.
      placeGizmo(state, direction)
      setShaderLightDir(state, direction)
      const now = performance.now()
      if (now - lastCommit > DRAG_COMMIT_MS) {
        lastCommit = now
        commit(direction)
      }
    }

    const onPointerUp = (event) => {
      if (!dragging) return
      dragging = false
      controls.enabled = true
      try { renderer.domElement.releasePointerCapture(event.pointerId) } catch { /* already gone */ }
      commit(handle.position.clone().normalize())
    }

    const dom = renderer.domElement
    dom.addEventListener('pointerdown', onPointerDown)
    dom.addEventListener('pointermove', onPointerMove)
    dom.addEventListener('pointerup', onPointerUp)
    dom.addEventListener('pointercancel', onPointerUp)

    const animate = () => {
      frame = requestAnimationFrame(animate)
      controls.update()
      scene.updateMatrixWorld()
      for (const entry of state.meshes) {
        entry.delit.uniforms.uNormalMatrix.value.getNormalMatrix(entry.mesh.matrixWorld)
      }
      renderer.render(scene, camera)
    }
    animate()

    return () => {
      disposed = true
      cancelAnimationFrame(frame)
      observer.disconnect()
      dom.removeEventListener('pointerdown', onPointerDown)
      dom.removeEventListener('pointermove', onPointerMove)
      dom.removeEventListener('pointerup', onPointerUp)
      dom.removeEventListener('pointercancel', onPointerUp)
      URL.revokeObjectURL(objectUrl)
      for (const entry of state.meshes) entry.delit.dispose()
      handle.geometry.dispose()
      handle.material.dispose()
      stem.geometry.dispose()
      stem.material.dispose()
      controls.dispose()
      envRT.dispose()
      pmrem.dispose()
      renderer.dispose()
      sceneRef.current = null
      mount.replaceChildren()
    }
  }, [file])

  useEffect(() => {
    if (sceneRef.current) applyMode(sceneRef.current, mode)
  }, [mode])

  useEffect(() => {
    applyFilters(canvasRef.current, settings)
  }, [settings.brightness, settings.contrast, settings.saturation])

  useEffect(() => {
    if (sceneRef.current) applyLighting(sceneRef.current, settings)
  }, [settings.light_azimuth, settings.light_elevation, settings.light_ambient,
      settings.light_gloss, settings.specular, settings.delight,
      settings.highlight_rejection])

  const update = (key, value) => onChange({ ...settings, [key]: Number(value) })
  const active = MODES.find((m) => m.key === mode) || MODES[0]

  return (
    <div className="panel preview-panel">
      <div className="preview-heading">
        <div>
          <h3>Live 3D Preview</h3>
          <p>Drag to orbit · wheel to zoom · drag the amber handle to aim the key light.</p>
        </div>
        <button className="preview-reset" onClick={() => onChange({ ...settings, ...defaults })}>Reset</button>
      </div>

      <div className="preview-modes" role="group" aria-label="Preview mode">
        {MODES.map((m) => (
          <button
            key={m.key}
            type="button"
            className={`preview-mode${m.key === mode ? ' preview-mode--on' : ''}`}
            onClick={() => setMode(m.key)}
          >
            {m.label}
          </button>
        ))}
      </div>

      <div ref={mountRef} className="model-preview" aria-label="Spinning 3D model preview">
        {error && <span className="preview-error">{error}</span>}
      </div>
      <p className="preview-mode-hint">{active.hint}</p>

      <div className="preview-controls">
        <Range label="Brightness" value={settings.brightness} min={-0.5} max={0.5} step={0.01} onChange={(v) => update('brightness', v)} />
        <Range label="Contrast" value={settings.contrast} min={0.5} max={2} step={0.01} onChange={(v) => update('contrast', v)} />
        <Range label="Saturation" value={settings.saturation} min={0} max={2} step={0.01} onChange={(v) => update('saturation', v)} />
      </div>

      <button
        type="button"
        className="preview-section-toggle"
        onClick={() => setShowLighting((v) => !v)}
        aria-expanded={showLighting}
      >
        {showLighting ? '▾' : '▸'} Lighting &amp; de-light
        {Number(settings.delight) > 0 && <em className="preview-badge">de-light on</em>}
      </button>

      {showLighting && (
        <div className="preview-lighting">
          <p className="preview-note">
            Models whose base-color texture is really a render — most photogrammetry and
            generated meshes — carry their highlights in the pixels. Aim the key light at
            one of them and raise <strong>De-light</strong> until the shine flattens out;
            the <strong>De-lit albedo</strong> view shows exactly what conversion will read.
            Where a highlight blew all the way to white the color is gone for good, and the
            voxel is rebuilt from the rest of its material instead.
          </p>
          <div className="preview-controls">
            <Range label="Light azimuth" value={settings.light_azimuth} min={-180} max={180} step={1} unit="°" onChange={(v) => update('light_azimuth', v)} />
            <Range label="Light elevation" value={settings.light_elevation} min={-90} max={90} step={1} unit="°" onChange={(v) => update('light_elevation', v)} />
            <Range label="De-light" value={settings.delight} min={0} max={1} step={0.01} onChange={(v) => update('delight', v)} />
            <Range label="Assumed gloss" value={settings.light_gloss} min={0} max={1} step={0.01} onChange={(v) => update('light_gloss', v)} />
            <Range label="Ambient" value={settings.light_ambient} min={0} max={1} step={0.01} onChange={(v) => update('light_ambient', v)} />
            <Range label="Specular gain" value={settings.specular} min={0} max={2} step={0.01} onChange={(v) => update('specular', v)} />
            <Range label="Highlight rejection" value={settings.highlight_rejection} min={0} max={1} step={0.01} onChange={(v) => update('highlight_rejection', v)} />
            <Range label="Blown-voxel recovery" value={settings.highlight_recovery} min={0} max={1} step={0.01} onChange={(v) => update('highlight_recovery', v)} />
          </div>
        </div>
      )}
    </div>
  )
}

// ── Scene helpers ────────────────────────────────────────────────────────────

function applyFilters(canvas, settings) {
  if (!canvas) return
  const brightness = Math.max(0, 1 + Number(settings.brightness || 0))
  const contrast = Math.max(0, Number(settings.contrast || 1))
  const saturation = Math.max(0, Number(settings.saturation || 1))
  canvas.style.filter = `brightness(${brightness}) contrast(${contrast}) saturate(${saturation})`
}

// The converter has no transparency model. sample_colors.py weights every
// surface it hits by that texel's alpha — alpha is coverage, not see-through —
// and the result is opaque blocks. Nothing is blended and nothing is sorted.
//
// So nothing here is blended either. Leaving a material in three.js's
// transparent queue makes it sort by object centroid, and on a model with many
// interleaved meshes that order is wrong often enough that panels, roll cages
// and seats blink out as the camera moves. Which materials it hits depends on
// how the model was exported, so any rule that decides *which* ones to blend is
// a rule that fails on the next model. Blend none of them: the preview is then
// stable at every angle by construction, and it shows what conversion will
// actually produce — a windscreen you cannot see through, because it is going
// to become solid blocks.
//
// The alpha test drops only texels with no coverage at all, matching the
// sampler, where alpha 0 contributes nothing and everything above it counts.
const EMPTY_ALPHA = 0.05

function flattenTransparency(root) {
  const seen = new Set()
  root.traverse((node) => {
    if (!node.isMesh || !node.material) return
    const mats = Array.isArray(node.material) ? node.material : [node.material]
    for (const m of mats) {
      if (!m || seen.has(m)) continue
      seen.add(m)
      m.transparent = false
      m.depthWrite = true
      m.alphaTest = m.map ? EMPTY_ALPHA : 0
      m.needsUpdate = true
    }
  })
}

// One de-lit material per mesh, mirroring what that mesh was already drawing.
function buildDelitMaterials(root) {
  const meshes = []
  root.traverse((node) => {
    if (!node.isMesh || Array.isArray(node.material) || !node.material) return
    const source = node.material
    const delit = new THREE.ShaderMaterial({
      vertexShader: VERTEX_SHADER,
      fragmentShader: FRAGMENT_SHADER,
      transparent: source.transparent,
      alphaTest: source.alphaTest,
      side: source.side,
      uniforms: {
        uMap: { value: source.map || null },
        uHasMap: { value: !!source.map },
        // Three stores colors in linear working space, which is where glTF's
        // baseColorFactor lives too, so this needs no conversion.
        uBaseColor: { value: new THREE.Vector3(...(source.color ? source.color.toArray() : [1, 1, 1])) },
        uLightDir: { value: new THREE.Vector3(...lightVector(LIGHT_DEFAULTS.light_azimuth, LIGHT_DEFAULTS.light_elevation)) },
        uNormalMatrix: { value: new THREE.Matrix3() },
        uAmbient: { value: LIGHT_DEFAULTS.light_ambient },
        uGloss: { value: LIGHT_DEFAULTS.light_gloss },
        uSpecular: { value: LIGHT_DEFAULTS.specular },
        uDelight: { value: LIGHT_DEFAULTS.delight },
        uRejection: { value: LIGHT_DEFAULTS.highlight_rejection },
        uAlphaTest: { value: source.alphaTest || 0 },
        uMode: { value: 0 },
      },
    })
    meshes.push({ mesh: node, original: source, delit })
  })
  return meshes
}

function applyMode(state, mode) {
  for (const entry of state.meshes) {
    entry.delit.uniforms.uMode.value = mode === 'mask' ? 1 : 0
    entry.mesh.material = mode === 'lit' ? entry.original : entry.delit
  }
}

function placeGizmo(state, direction) {
  const tip = state.handle.position.copy(direction).multiplyScalar(GIZMO_RADIUS)
  const points = state.stem.geometry.attributes.position
  points.setXYZ(0, 0, 0, 0)
  points.setXYZ(1, tip.x, tip.y, tip.z)
  points.needsUpdate = true
  state.stem.geometry.computeBoundingSphere()
}

function setShaderLightDir(state, direction) {
  for (const entry of state.meshes) entry.delit.uniforms.uLightDir.value.copy(direction)
}

function applyLighting(state, settings) {
  const direction = new THREE.Vector3(
    ...lightVector(settings.light_azimuth ?? LIGHT_DEFAULTS.light_azimuth,
      settings.light_elevation ?? LIGHT_DEFAULTS.light_elevation),
  )
  placeGizmo(state, direction)
  setShaderLightDir(state, direction)
  for (const entry of state.meshes) {
    const u = entry.delit.uniforms
    u.uAmbient.value = Number(settings.light_ambient ?? LIGHT_DEFAULTS.light_ambient)
    u.uGloss.value = Number(settings.light_gloss ?? LIGHT_DEFAULTS.light_gloss)
    u.uSpecular.value = Number(settings.specular ?? LIGHT_DEFAULTS.specular)
    u.uDelight.value = Number(settings.delight ?? LIGHT_DEFAULTS.delight)
    u.uRejection.value = Number(settings.highlight_rejection ?? LIGHT_DEFAULTS.highlight_rejection)
  }
}

function Range({ label, value, min, max, step, unit = '', onChange }) {
  const shown = unit ? `${Math.round(Number(value))}${unit}` : Number(value).toFixed(2)
  return (
    <label className="preview-range">
      <span>{label}<output>{shown}</output></span>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(e.target.value)} />
    </label>
  )
}
