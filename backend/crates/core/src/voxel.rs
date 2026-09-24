//! Surface voxelization: the shell of voxels a model's triangles pass through.
//!
//! A port of the Python helper's `voxelize_surface`, down to the order of its
//! random draws, so the two produce the same voxels:
//!
//! 1. points scattered over the surface by area, about twelve per voxel face
//!    of surface (seeded, so a model always gives the same shell);
//! 2. points along every edge, one per voxel length — thin walls and sharp
//!    features that scattering can miss;
//! 3. every vertex;
//!
//! all snapped to the grid and deduplicated. No dense grid is built, so memory
//! follows the surface, not the volume.

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::mesh::Model;
use crate::pipeline::{Progress, Stage};
use crate::rng::{pairwise_sum, NumpyRng};
use crate::settings::limits::MAX_SIZE;

/// Seed of every random draw the voxelizer and the sampler make.
pub const SEED: u64 = 0x5CE2;

/// Face samples per voxel face worth of surface area.
const SAMPLES_PER_VOXEL_FACE: f64 = 12.0;
const MIN_FACE_SAMPLES: usize = 100_000;
const MAX_FACE_SAMPLES: usize = 8_000_000;

/// Samples handled per parallel task.
const BLOCK: usize = 1 << 16;

/// The voxel shell of a model.
#[derive(Debug, Clone)]
pub struct Shell {
    /// Occupied cells, ascending by x, then y, then z.
    pub coords: Vec<[i32; 3]>,
    /// Model units per voxel.
    pub pitch: f64,
    /// Cells per axis.
    pub dims: [i32; 3],
    /// Model-space minimum corner of cell (0, 0, 0).
    pub origin: [f64; 3],
}

/// All primitives as one triangle soup, in scene order.
struct Soup {
    positions: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

impl Soup {
    fn new(model: &Model) -> Self {
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        for p in &model.primitives {
            let offset = positions.len() as u32;
            positions.extend_from_slice(&p.positions);
            triangles.extend(p.triangles.iter().map(|t| t.map(|i| i + offset)));
        }
        Soup {
            positions,
            triangles,
        }
    }

    fn corners(&self, t: usize) -> [[f64; 3]; 3] {
        self.triangles[t].map(|i| self.positions[i as usize])
    }

    /// Bounds of the vertices that triangles use, as trimesh's bounding box
    /// reports them: rebuilt from its center and extents, which rounds.
    fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let mut used = vec![false; self.positions.len()];
        for t in &self.triangles {
            for &i in t {
                used[i as usize] = true;
            }
        }
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for (p, _) in self.positions.iter().zip(&used).filter(|(_, &u)| u) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let center: [f64; 3] = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0);
        let extent: [f64; 3] = std::array::from_fn(|k| hi[k] - lo[k]);
        // The box mesh is placed with a translation, which trimesh skips when
        // it is within 1e-8 of none at all (by spread, then by size).
        let spread = center.iter().fold(0.0f64, |m, &c| m.max(c))
            - center.iter().fold(0.0f64, |m, &c| m.min(c));
        let largest = center.iter().fold(0.0f64, |m, &c| m.max(c.abs()));
        let moved = spread >= 1e-8 && largest >= 1e-8;
        let corner = |sign: f64| -> [f64; 3] {
            std::array::from_fn(|k| {
                let half = sign * 0.5 * extent[k];
                if moved {
                    half + center[k]
                } else {
                    half
                }
            })
        };
        (corner(-1.0), corner(1.0))
    }

    /// Each triangle's area, as trimesh computes it.
    fn areas(&self) -> Vec<f64> {
        (0..self.triangles.len())
            .into_par_iter()
            .map(|t| {
                let [v0, v1, v2] = self.corners(t);
                let a: [f64; 3] = std::array::from_fn(|k| v1[k] - v0[k]);
                let b: [f64; 3] = std::array::from_fn(|k| v2[k] - v1[k]);
                let c = [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ];
                (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() / 2.0
            })
            .collect()
    }
}

/// Snaps model-space points to cells and packs each as `x·Dy·Dz + y·Dz + z`,
/// which sorts like `(x, y, z)`.
#[derive(Clone, Copy)]
struct Grid {
    origin: [f64; 3],
    pitch: f64,
    dims: [i32; 3],
}

impl Grid {
    fn key(&self, p: [f64; 3]) -> i64 {
        let cell = |k: usize| -> i64 {
            let c = ((p[k] - self.origin[k]) / self.pitch).floor() as i32;
            i64::from(c.clamp(0, self.dims[k] - 1))
        };
        let (dy, dz) = (i64::from(self.dims[1]), i64::from(self.dims[2]));
        cell(0) * (dy * dz) + cell(1) * dz + cell(2)
    }

    fn coords(&self, key: i64) -> [i32; 3] {
        let (dy, dz) = (i64::from(self.dims[1]), i64::from(self.dims[2]));
        [
            (key / (dy * dz)) as i32,
            ((key / dz) % dy) as i32,
            (key % dz) as i32,
        ]
    }
}

fn sorted_unique(mut keys: Vec<i64>) -> Vec<i64> {
    keys.par_sort_unstable();
    keys.dedup();
    keys
}

/// Voxelize `model`'s surface with cells of `voxel_size` model units, or —
/// when it is `None` — cells that make the longest side `max_size` long.
pub fn voxelize(
    model: &Model,
    voxel_size: Option<f64>,
    max_size: u32,
    progress: &mut dyn Progress,
) -> Result<Shell> {
    let soup = Soup::new(model);
    if soup.triangles.is_empty() {
        return Err(Error::NoGeometry);
    }
    let (lo, hi) = soup.bounds();
    let extents: [f64; 3] = std::array::from_fn(|k| hi[k] - lo[k]);
    let longest = extents.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let voxel_size = voxel_size.unwrap_or(longest / f64::from(max_size));
    if !(voxel_size.is_finite() && voxel_size > 0.0) {
        return Err(Error::Mesh("the model has no size to voxelize".into()));
    }

    // A point on the far face belongs to the last cell rather than opening a
    // new one; the tolerance absorbs the rounding in extent / (extent / n).
    let dims: [f64; 3] = std::array::from_fn(|k| (extents[k] / voxel_size - 1e-9).ceil().max(1.0));
    if dims.iter().any(|&d| d > f64::from(MAX_SIZE.max)) {
        return Err(Error::invalid(
            "voxel_size",
            format!(
                "{voxel_size} makes the model {:.0} × {:.0} × {:.0} blocks; the most is {} along any side",
                dims[0],
                dims[1],
                dims[2],
                MAX_SIZE.max
            ),
        ));
    }
    let grid = Grid {
        origin: lo,
        pitch: voxel_size,
        dims: dims.map(|d| d as i32),
    };

    let areas = soup.areas();
    let surface = pairwise_sum(&areas).max(1e-12);
    let wanted = (surface / (voxel_size * voxel_size) * SAMPLES_PER_VOXEL_FACE) as usize;
    let count = wanted.clamp(MIN_FACE_SAMPLES, MAX_FACE_SAMPLES);
    progress.update(
        Stage::Voxelize,
        0.04,
        &format!(
            "Voxelizing {} triangles: {}×{}×{} grid, {count} surface samples",
            soup.triangles.len(),
            grid.dims[0],
            grid.dims[1],
            grid.dims[2]
        ),
    );

    let mut keys = face_keys(&soup, &areas, count, &grid, progress)?;
    keys.extend(edge_keys(&soup, &grid));
    // Every vertex a triangle uses (unused ones never made it into the soup's
    // triangles, and bounds ignore them too).
    keys.extend(
        soup.triangles
            .iter()
            .flatten()
            .map(|&i| grid.key(soup.positions[i as usize])),
    );
    let keys = sorted_unique(keys);

    Ok(Shell {
        coords: keys.iter().map(|&k| grid.coords(k)).collect(),
        pitch: voxel_size,
        dims: grid.dims,
        origin: lo,
    })
}

/// `trimesh.sample.sample_surface(mesh, count, seed=SEED)` snapped to cells.
///
/// NumPy draws `count` face picks, then `count` pairs of edge lengths, from
/// one stream; each block of samples here jumps straight to its own part of
/// both, so blocks run in parallel and still see the same numbers.
fn face_keys(
    soup: &Soup,
    areas: &[f64],
    count: usize,
    grid: &Grid,
    progress: &mut dyn Progress,
) -> Result<Vec<i64>> {
    let mut cumulative = Vec::with_capacity(areas.len());
    let mut total = 0.0;
    for &a in areas {
        total += a;
        cumulative.push(total);
    }
    let last = cumulative.len() - 1;
    let rng = NumpyRng::new(SEED);

    let blocks: Vec<usize> = (0..count).step_by(BLOCK).collect();
    let mut keys = Vec::new();
    // Chunks of blocks, so a cancel is noticed between them.
    for chunk in blocks.chunks(rayon::current_num_threads().max(1) * 4) {
        if progress.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let found: Vec<Vec<i64>> = chunk
            .par_iter()
            .map(|&start| {
                let end = (start + BLOCK).min(count);
                let mut picks = rng.skipped(start as u128);
                let mut lengths = rng.skipped((count + 2 * start) as u128);
                let mut out = Vec::with_capacity(end - start);
                for _ in start..end {
                    let pick = picks.random() * total;
                    let face = cumulative.partition_point(|&c| c < pick).min(last);
                    let (mut r0, mut r1) = (lengths.random(), lengths.random());
                    if r0 + r1 > 1.0 {
                        r0 = (r0 - 1.0).abs();
                        r1 = (r1 - 1.0).abs();
                    }
                    let [v0, v1, v2] = soup.corners(face);
                    let point: [f64; 3] = std::array::from_fn(|k| {
                        ((v1[k] - v0[k]) * r0 + (v2[k] - v0[k]) * r1) + v0[k]
                    });
                    out.push(grid.key(point));
                }
                out.sort_unstable();
                out.dedup();
                out
            })
            .collect();
        keys.extend(found.into_iter().flatten());
    }
    Ok(keys)
}

/// Points along every distinct edge: one per voxel length, at least both
/// ends — `np.linspace(0, 1, n)` between the edge's lower- and higher-numbered
/// vertex.
fn edge_keys(soup: &Soup, grid: &Grid) -> Vec<i64> {
    let mut edges: Vec<u64> = soup
        .triangles
        .par_iter()
        .flat_map_iter(|t| {
            [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
                .map(|(a, b)| u64::from(a.min(b)) << 32 | u64::from(a.max(b)))
        })
        .collect();
    edges.par_sort_unstable();
    edges.dedup();

    let found: Vec<Vec<i64>> = edges
        .par_chunks(BLOCK)
        .map(|chunk| {
            let mut out = Vec::new();
            for &edge in chunk {
                let v0 = soup.positions[(edge >> 32) as usize];
                let v1 = soup.positions[(edge & 0xffff_ffff) as usize];
                let d: [f64; 3] = std::array::from_fn(|k| v1[k] - v0[k]);
                let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let n = ((length / grid.pitch) as i64 + 1).max(2);
                let step = 1.0 / (n - 1) as f64;
                for i in 0..n {
                    let t = if i == n - 1 { 1.0 } else { i as f64 * step };
                    out.push(grid.key(std::array::from_fn(|k| v0[k] * (1.0 - t) + v1[k] * t)));
                }
            }
            out.sort_unstable();
            out.dedup();
            out
        })
        .collect();
    found.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{LoadOptions, Primitive};
    use crate::pipeline::NoProgress;

    fn cube(size: f64) -> Model {
        let s = size / 2.0;
        let positions: Vec<[f64; 3]> = (0..8)
            .map(|i| {
                [
                    if i & 1 == 0 { -s } else { s },
                    if i & 2 == 0 { -s } else { s },
                    if i & 4 == 0 { -s } else { s },
                ]
            })
            .collect();
        let triangles = vec![
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ];
        Model {
            primitives: vec![Primitive {
                positions,
                triangles,
                ..Primitive::default()
            }],
            ..Model::default()
        }
    }

    #[test]
    fn a_cube_becomes_a_hollow_shell() {
        let shell = voxelize(&cube(2.0), None, 8, &mut NoProgress).unwrap();
        assert_eq!(shell.dims, [8, 8, 8]);
        // 8³ minus the 6³ inside.
        assert_eq!(shell.coords.len(), 8 * 8 * 8 - 6 * 6 * 6);
        assert!(
            shell.coords.windows(2).all(|w| w[0] < w[1]),
            "sorted and unique"
        );
        assert!((shell.pitch - 0.25).abs() < 1e-12);
        assert_eq!(shell.origin, [-1.0; 3]);
    }

    #[test]
    fn longest_side_is_exactly_max_size() {
        for max in [1, 7, 64, 100] {
            let shell = voxelize(&cube(3.3), None, max, &mut NoProgress).unwrap();
            assert_eq!(shell.dims, [max as i32; 3], "max_size {max}");
        }
    }

    #[test]
    fn explicit_voxel_size_wins_and_is_bounded() {
        let shell = voxelize(&cube(2.0), Some(0.5), 128, &mut NoProgress).unwrap();
        assert_eq!(shell.dims, [4, 4, 4]);
        let err = voxelize(&cube(2.0), Some(1e-4), 128, &mut NoProgress).unwrap_err();
        assert!(err.to_string().contains("voxel_size"), "{err}");
    }

    #[test]
    fn same_model_same_voxels() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/textured.glb");
        let model = crate::mesh::load(&path, LoadOptions { textures: false }).unwrap();
        let a = voxelize(&model, None, 48, &mut NoProgress).unwrap();
        let b = voxelize(&model, None, 48, &mut NoProgress).unwrap();
        assert_eq!(a.coords, b.coords);
        assert!(!a.coords.is_empty());
    }
}
