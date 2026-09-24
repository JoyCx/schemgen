// One three.js renderer for the whole viewport. The model and its blocks are
// two scenes seen through the same camera, so orbiting one orbits both, and
// Split draws the model left of the divider and the blocks right of it —
// the same view, compared.

import * as THREE from 'three'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js'
import { BlockLayer } from './blocks'
import { GIZMO_RADIUS, ModelLayer, type ModelLook } from './model'
import type { PaletteColors, Preview } from '../types'
import type { Settings } from '../settings'

export type ViewMode = 'model' | 'blocks' | 'split'

/** The model is framed to this many units across. */
const FRAME = 2.1
/** Radius the camera keeps in view: the model, and the light handle around it. */
const VIEW_RADIUS = 1.5
const FOV = 35
/** Looking down a little, from the front. */
const VIEW_DIRECTION = new THREE.Vector3(0, 0.12, 1).normalize()

const BACKGROUNDS = {
  light: { model: 0xf1f0ec, blocks: 0xcfe3ef },
  dark: { model: 0x1b1d22, blocks: 0x16222c },
}

export interface EngineEvents {
  /** The key light was dragged to a new model-space direction. */
  onLight?: (direction: [number, number, number], final: boolean) => void
  onModelError?: (message: string) => void
}

export class ViewportEngine {
  readonly canvas: HTMLCanvasElement
  private renderer: THREE.WebGLRenderer
  private camera = new THREE.PerspectiveCamera(FOV, 1, 0.01, 1000)
  private controls: OrbitControls
  private modelScene = new THREE.Scene()
  private blocksScene = new THREE.Scene()
  /** Model space → view space, shared by both scenes. */
  private modelRoot = new THREE.Group()
  private blocksRoot = new THREE.Group()
  private model = new ModelLayer()
  private blocks = new BlockLayer()
  private mode: ViewMode = 'model'
  private split = 0.5
  private dirty = true
  private rafId = 0
  private framed = false
  private observer: ResizeObserver
  private events: EngineEvents
  private dragging = false
  private environment: THREE.Texture
  /** Reframe on resize: true until the user moves the camera. */
  private autoFrame = true

  constructor(host: HTMLElement, events: EngineEvents = {}) {
    this.events = events
    this.renderer = new THREE.WebGLRenderer({ antialias: true })
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
    this.renderer.outputColorSpace = THREE.SRGBColorSpace
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping
    this.canvas = this.renderer.domElement
    this.canvas.className = 'viewport-canvas'
    host.prepend(this.canvas)

    this.controls = new OrbitControls(this.camera, this.canvas)
    this.controls.enableDamping = true
    this.controls.minDistance = 0.5
    this.controls.maxDistance = 12
    this.controls.addEventListener('change', () => (this.dirty = true))
    // Once the user moves the camera, resizing stops reframing it.
    this.controls.addEventListener('start', () => (this.autoFrame = false))

    // Model: a soft studio. Blocks: sky and sun, like the game.
    this.modelScene.add(new THREE.HemisphereLight(0xffffff, 0x777777, 2.2))
    const key = new THREE.DirectionalLight(0xffffff, 2.5)
    key.position.set(3, 5, 4)
    const fill = new THREE.DirectionalLight(0xb8d4ff, 1.2)
    fill.position.set(-4, 2, -3)
    this.modelScene.add(key, fill, this.modelRoot, this.model.gizmo)
    this.modelRoot.add(this.model.group)
    // Reflections for metals, which otherwise render black: there is no
    // diffuse color to light, only the room they reflect.
    const pmrem = new THREE.PMREMGenerator(this.renderer)
    this.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture
    this.modelScene.environment = this.environment
    this.modelScene.environmentIntensity = 0.8
    pmrem.dispose()

    this.blocksScene.add(new THREE.HemisphereLight(0xffffff, 0x52657a, 2.2))
    const sun = new THREE.DirectionalLight(0xffffff, 2.8)
    sun.position.set(4, 8, 5)
    this.blocksScene.add(sun, this.blocksRoot)
    this.blocksRoot.add(this.blocks.group)
    this.blocks.onChange = () => (this.dirty = true)

    this.setTheme('light')
    this.observer = new ResizeObserver(() => this.resize())
    this.observer.observe(host)
    this.resize()
    this.resetCamera()

    this.canvas.addEventListener('pointerdown', this.onPointerDown)
    this.canvas.addEventListener('pointermove', this.onPointerMove)
    this.canvas.addEventListener('pointerup', this.onPointerUp)
    this.canvas.addEventListener('pointercancel', this.onPointerUp)
    this.loop()
  }

  // ---- Inputs --------------------------------------------------------------

  async setModel(file: File | null): Promise<void> {
    this.framed = false
    this.resetCamera()
    try {
      await this.model.load(file)
    } catch (e) {
      this.events.onModelError?.(e instanceof Error ? e.message : 'Could not display this model')
    }
    this.fit()
    this.dirty = true
  }

  setBlocks(preview: Preview | null, palette: PaletteColors | null): void {
    this.blocks.set(preview, palette)
    if (!this.model.loaded) this.fit()
    this.dirty = true
  }

  setMode(mode: ViewMode): void {
    this.mode = mode
    this.dirty = true
  }

  /** Where the divider is in Split, 0 (left edge) to 1. */
  setSplit(split: number): void {
    this.split = Math.min(1, Math.max(0, split))
    this.dirty = true
  }

  setLook(look: ModelLook): void {
    this.model.setLook(look)
    this.dirty = true
  }

  setLighting(settings: Settings | null): void {
    if (this.dragging) return
    this.model.setLighting(settings)
    this.dirty = true
  }

  setAutoRotate(on: boolean): void {
    this.controls.autoRotate = on
    this.controls.autoRotateSpeed = 0.8
    this.dirty = true
  }

  setTheme(theme: 'light' | 'dark'): void {
    this.modelScene.background = new THREE.Color(BACKGROUNDS[theme].model)
    this.blocksScene.background = new THREE.Color(BACKGROUNDS[theme].blocks)
    this.dirty = true
  }

  /** Back to the starting view: far enough that the model and the light
   *  handle fit, whichever way round the viewport is. */
  resetCamera(): void {
    const half = THREE.MathUtils.degToRad(FOV / 2)
    const vertical = Math.tan(half)
    const horizontal = vertical * Math.max(this.camera.aspect, 0.1)
    const distance = (VIEW_RADIUS / Math.min(vertical, horizontal)) * 1.05
    this.camera.position.copy(VIEW_DIRECTION).multiplyScalar(distance)
    this.controls.target.set(0, 0, 0)
    this.controls.maxDistance = distance * 4
    this.controls.update()
    this.autoFrame = true
    this.dirty = true
  }

  // ---- Framing -------------------------------------------------------------

  /** Center and scale model space to ~FRAME units — from the model when it
   *  loaded, else from the block grid. Both scenes share the transform, so
   *  the blocks sit exactly over the model. */
  private fit(): void {
    const box = !this.model.bounds.isEmpty() ? this.model.bounds : this.blocks.bounds
    if (box.isEmpty()) return
    if (this.framed && box === this.blocks.bounds) return
    const size = box.getSize(new THREE.Vector3())
    const center = box.getCenter(new THREE.Vector3())
    const scale = FRAME / (Math.max(size.x, size.y, size.z) || 1)
    for (const root of [this.modelRoot, this.blocksRoot]) {
      root.scale.setScalar(scale)
      root.position.copy(center).multiplyScalar(-scale)
    }
    this.framed = box === this.model.bounds
  }

  // ---- Rendering -----------------------------------------------------------

  private resize(): void {
    const width = this.canvas.parentElement?.clientWidth || 640
    const height = this.canvas.parentElement?.clientHeight || 400
    this.camera.aspect = width / height
    this.camera.updateProjectionMatrix()
    this.renderer.setSize(width, height, false)
    if (this.autoFrame) this.resetCamera()
    this.dirty = true
  }

  private loop = () => {
    this.rafId = requestAnimationFrame(this.loop)
    this.controls.update()
    if (!this.dirty && !this.controls.autoRotate) return
    this.dirty = false
    this.render()
  }

  private render(): void {
    const r = this.renderer
    const size = r.getSize(new THREE.Vector2())
    this.modelScene.updateMatrixWorld()
    this.model.update()
    this.model.gizmo.visible = this.mode !== 'blocks'
    r.setScissorTest(false)
    r.setViewport(0, 0, size.x, size.y)
    if (this.mode === 'model') {
      r.render(this.modelScene, this.camera)
    } else if (this.mode === 'blocks') {
      r.render(this.blocksScene, this.camera)
    } else {
      // Both halves use the full viewport, so their projections line up.
      const divider = Math.round(this.split * size.x)
      r.setScissorTest(true)
      r.setScissor(0, 0, divider, size.y)
      r.render(this.modelScene, this.camera)
      r.setScissor(divider, 0, size.x - divider, size.y)
      r.render(this.blocksScene, this.camera)
      r.setScissorTest(false)
    }
  }

  // ---- Dragging the key light ----------------------------------------------

  private raycaster = new THREE.Raycaster()
  private pointer = new THREE.Vector2()
  private orbit = new THREE.Sphere(new THREE.Vector3(), GIZMO_RADIUS)
  private lastSent = 0

  /** Whether a canvas x (0–1) shows the model, where the handle lives. */
  private showsModel(x: number): boolean {
    return this.mode === 'model' || (this.mode === 'split' && x < this.split)
  }

  private aimAt(event: PointerEvent): number {
    const rect = this.canvas.getBoundingClientRect()
    const x = (event.clientX - rect.left) / rect.width
    this.pointer.set(x * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1)
    this.raycaster.setFromCamera(this.pointer, this.camera)
    return x
  }

  private onPointerDown = (event: PointerEvent) => {
    const x = this.aimAt(event)
    if (!this.showsModel(x) || !this.raycaster.intersectObject(this.model.handle, false).length)
      return
    this.dragging = true
    this.controls.enabled = false
    this.canvas.setPointerCapture(event.pointerId)
    event.preventDefault()
  }

  private onPointerMove = (event: PointerEvent) => {
    if (!this.dragging) return
    this.aimAt(event)
    // Past the model's silhouette the ray misses the handle's sphere; the
    // ray's closest point to its center still says where it is aiming.
    const scratch = new THREE.Vector3()
    const hit =
      this.raycaster.ray.intersectSphere(this.orbit, scratch) ||
      this.raycaster.ray.closestPointToPoint(this.orbit.center, scratch)
    const direction = hit.clone().sub(this.orbit.center)
    if (direction.lengthSq() < 1e-8) return
    direction.normalize()
    this.model.aim(direction)
    this.dirty = true
    // The handle follows every frame; the settings (and so a new preview)
    // only a few times a second.
    const now = performance.now()
    if (now - this.lastSent > 90) {
      this.lastSent = now
      this.events.onLight?.(direction.toArray() as [number, number, number], false)
    }
  }

  private onPointerUp = (event: PointerEvent) => {
    if (!this.dragging) return
    this.dragging = false
    this.controls.enabled = true
    try {
      this.canvas.releasePointerCapture(event.pointerId)
    } catch {
      /* already released */
    }
    this.events.onLight?.(this.model.direction().toArray() as [number, number, number], true)
  }

  dispose(): void {
    cancelAnimationFrame(this.rafId)
    this.observer.disconnect()
    this.canvas.removeEventListener('pointerdown', this.onPointerDown)
    this.canvas.removeEventListener('pointermove', this.onPointerMove)
    this.canvas.removeEventListener('pointerup', this.onPointerUp)
    this.canvas.removeEventListener('pointercancel', this.onPointerUp)
    this.model.dispose()
    this.blocks.dispose()
    this.environment.dispose()
    this.controls.dispose()
    this.renderer.dispose()
    this.canvas.remove()
  }
}
