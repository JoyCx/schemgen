// The Model view: the glTF as its materials describe it, or as the color
// sampler will read it once the assumed lighting is removed — plus the handle
// that aims the key light.

import * as THREE from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { FRAGMENT_SHADER, VERTEX_SHADER } from '../delightshader.js'
import type { Settings } from '../settings'

export type ModelLook = 'lit' | 'albedo' | 'mask'

/** The handle orbits just outside the model, which is framed ~2.1 units across. */
export const GIZMO_RADIUS = 1.35

interface Delit {
  mesh: THREE.Mesh
  original: THREE.Material
  delit: THREE.ShaderMaterial
}

const DEFAULT_DIR = new THREE.Vector3(0.35, 0.85, 0.4).normalize()

export class ModelLayer {
  /** Holds the glTF scene, in model space. */
  readonly group = new THREE.Group()
  /** The key-light handle, in view space (it is not scaled with the model). */
  readonly gizmo = new THREE.Group()
  readonly handle: THREE.Mesh
  private stem: THREE.Line
  private meshes: Delit[] = []
  private url: string | null = null
  private generation = 0
  look: ModelLook = 'lit'
  /** Model-space bounds of what loaded. */
  bounds = new THREE.Box3()

  constructor() {
    this.handle = new THREE.Mesh(
      new THREE.SphereGeometry(0.085, 24, 18),
      new THREE.MeshBasicMaterial({ color: 0xffb020 }),
    )
    this.stem = new THREE.Line(
      new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(), new THREE.Vector3()]),
      new THREE.LineBasicMaterial({ color: 0xffb020, transparent: true, opacity: 0.45 }),
    )
    this.gizmo.add(this.handle, this.stem)
    this.placeGizmo(DEFAULT_DIR)
  }

  /** Load `file`, replacing whatever was shown. Resolves once it is in. */
  load(file: File | null): Promise<void> {
    const generation = ++this.generation
    this.clear()
    if (!file) return Promise.resolve()
    this.url = URL.createObjectURL(file)
    const url = this.url
    return new Promise((resolve, reject) => {
      new GLTFLoader().load(
        url,
        (gltf) => {
          if (generation !== this.generation) return resolve()
          this.group.add(gltf.scene)
          gltf.scene.updateMatrixWorld(true)
          this.bounds.setFromObject(gltf.scene)
          this.meshes = buildDelitMaterials(gltf.scene)
          this.applyLook()
          resolve()
        },
        undefined,
        (error) => {
          if (generation !== this.generation) return resolve()
          reject(error instanceof Error ? error : new Error('Could not display this model'))
        },
      )
    })
  }

  setLook(look: ModelLook): void {
    this.look = look
    this.applyLook()
  }

  private applyLook(): void {
    for (const entry of this.meshes) {
      entry.delit.uniforms.uMode.value = this.look === 'mask' ? 1 : 0
      entry.mesh.material = this.look === 'lit' ? entry.original : entry.delit
    }
  }

  /** The key light and the de-light knobs, from the settings. */
  setLighting(settings: Settings | null): void {
    const raw = settings?.light_dir
    const dir =
      Array.isArray(raw) && raw.length === 3
        ? new THREE.Vector3(...raw.map(Number))
        : DEFAULT_DIR.clone()
    if (dir.lengthSq() < 1e-12) dir.copy(DEFAULT_DIR)
    dir.normalize()
    this.placeGizmo(dir)
    const number = (key: string, fallback: number) => {
      const n = Number(settings?.[key])
      return Number.isFinite(n) ? n : fallback
    }
    for (const { delit } of this.meshes) {
      const u = delit.uniforms
      u.uLightDir.value.copy(dir)
      u.uAmbient.value = number('light_ambient', 0.32)
      u.uGloss.value = number('light_gloss', 0.5)
      u.uSpecular.value = number('specular', 1.1)
      u.uDelight.value = number('delight', 0)
      u.uRejection.value = number('highlight_rejection', 0.75)
    }
  }

  /** Move the handle and the shader's light while dragging, before React hears. */
  aim(direction: THREE.Vector3): void {
    this.placeGizmo(direction)
    for (const { delit } of this.meshes) delit.uniforms.uLightDir.value.copy(direction)
  }

  direction(): THREE.Vector3 {
    return this.handle.position.clone().normalize()
  }

  private placeGizmo(direction: THREE.Vector3): void {
    const tip = this.handle.position.copy(direction).multiplyScalar(GIZMO_RADIUS)
    const points = this.stem.geometry.attributes.position as THREE.BufferAttribute
    points.setXYZ(0, 0, 0, 0)
    points.setXYZ(1, tip.x, tip.y, tip.z)
    points.needsUpdate = true
    this.stem.geometry.computeBoundingSphere()
  }

  /** Normal matrices for the de-lit shader, once world matrices are current. */
  update(): void {
    for (const { mesh, delit } of this.meshes) {
      delit.uniforms.uNormalMatrix.value.getNormalMatrix(mesh.matrixWorld)
    }
  }

  get loaded(): boolean {
    return this.group.children.length > 0
  }

  clear(): void {
    for (const { delit } of this.meshes) delit.dispose()
    this.meshes = []
    for (const child of [...this.group.children]) {
      this.group.remove(child)
      child.traverse((node) => {
        const mesh = node as THREE.Mesh
        mesh.geometry?.dispose?.()
        const materials = Array.isArray(mesh.material)
          ? mesh.material
          : mesh.material
            ? [mesh.material]
            : []
        for (const m of materials) m.dispose()
      })
    }
    if (this.url) URL.revokeObjectURL(this.url)
    this.url = null
    this.bounds.makeEmpty()
  }

  dispose(): void {
    this.generation++
    this.clear()
    this.handle.geometry.dispose()
    ;(this.handle.material as THREE.Material).dispose()
    this.stem.geometry.dispose()
    ;(this.stem.material as THREE.Material).dispose()
  }
}

/** One de-lit material per mesh, mirroring what that mesh was already drawing. */
function buildDelitMaterials(root: THREE.Object3D): Delit[] {
  const out: Delit[] = []
  root.traverse((node) => {
    const mesh = node as THREE.Mesh
    if (!mesh.isMesh || Array.isArray(mesh.material) || !mesh.material) return
    const source = mesh.material as THREE.MeshStandardMaterial
    const delit = new THREE.ShaderMaterial({
      vertexShader: VERTEX_SHADER,
      fragmentShader: FRAGMENT_SHADER,
      transparent: source.transparent,
      alphaTest: source.alphaTest,
      side: source.side,
      uniforms: {
        uMap: { value: source.map || null },
        uHasMap: { value: !!source.map },
        // Three keeps colors in linear working space, which is where glTF's
        // baseColorFactor lives too, so this needs no conversion.
        uBaseColor: {
          value: new THREE.Vector3(...(source.color ? source.color.toArray() : [1, 1, 1])),
        },
        uLightDir: { value: DEFAULT_DIR.clone() },
        uNormalMatrix: { value: new THREE.Matrix3() },
        uAmbient: { value: 0.32 },
        uGloss: { value: 0.5 },
        uSpecular: { value: 1.1 },
        uDelight: { value: 0 },
        uRejection: { value: 0.75 },
        uAlphaTest: { value: source.alphaTest || 0 },
        uMode: { value: 0 },
      },
    })
    out.push({ mesh, original: source, delit })
  })
  return out
}
