//! The pipeline's result: which block goes where.

use serde::Serialize;

/// A voxelized, block-matched model.
///
/// Positions are block coordinates with the model's minimum corner at the
/// origin; `names` is the per-grid palette (sorted, no air) and `blocks`
/// indexes it once per position. Every schematic writer, the thumbnail and
/// the API's preview are built from this.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockGrid {
    /// Tight extent: one past the largest coordinate on each axis.
    pub size: [u32; 3],
    pub coords: Vec<[i32; 3]>,
    /// Index into `names`, one per entry of `coords`.
    pub blocks: Vec<u16>,
    /// Distinct block IDs, sorted.
    pub names: Vec<String>,
    /// Model-space position of block (0, 0, 0)'s minimum corner.
    pub origin: [f32; 3],
    /// Model units per block.
    pub pitch: f32,
}

/// One row of a material list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Material {
    pub name: String,
    pub count: u64,
}

impl BlockGrid {
    /// Build a grid from coordinates and the block ID chosen for each.
    ///
    /// Coordinates must be non-negative. Duplicate coordinates are not merged;
    /// the pipeline never produces them.
    pub fn from_names<'a>(
        coords: Vec<[i32; 3]>,
        names_per_block: impl IntoIterator<Item = &'a str>,
        origin: [f32; 3],
        pitch: f32,
    ) -> Self {
        let per_block: Vec<&str> = names_per_block.into_iter().collect();
        assert_eq!(per_block.len(), coords.len(), "one name per coordinate");
        let mut unique = per_block.clone();
        unique.sort_unstable();
        unique.dedup();
        assert!(
            unique.len() <= u16::MAX as usize,
            "at most 65535 block types"
        );
        let blocks = per_block
            .iter()
            .map(|n| unique.binary_search(n).expect("name is in the palette") as u16)
            .collect();
        Self {
            size: extent(&coords),
            names: unique.iter().map(|s| s.to_string()).collect(),
            coords,
            blocks,
            origin,
            pitch,
        }
    }

    /// Build a grid from coordinates and a palette index per block, where
    /// `name_of` resolves an index to its block ID. Several indices may name
    /// the same block; they collapse to one grid entry.
    pub fn from_indices<'a>(
        coords: Vec<[i32; 3]>,
        indices: &[u32],
        name_of: impl Fn(u32) -> &'a str,
        origin: [f32; 3],
        pitch: f32,
    ) -> Self {
        assert_eq!(indices.len(), coords.len(), "one index per coordinate");
        let mut distinct = indices.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        let mut unique: Vec<&str> = distinct.iter().map(|&i| name_of(i)).collect();
        unique.sort_unstable();
        unique.dedup();
        assert!(
            unique.len() <= u16::MAX as usize,
            "at most 65535 block types"
        );
        let to_grid: std::collections::HashMap<u32, u16> = distinct
            .iter()
            .map(|&i| {
                (
                    i,
                    unique.binary_search(&name_of(i)).expect("known name") as u16,
                )
            })
            .collect();
        Self {
            size: extent(&coords),
            names: unique.iter().map(|s| s.to_string()).collect(),
            blocks: indices.iter().map(|i| to_grid[i]).collect(),
            coords,
            origin,
            pitch,
        }
    }

    /// Number of placed blocks.
    pub fn len(&self) -> usize {
        self.coords.len()
    }

    pub fn is_empty(&self) -> bool {
        self.coords.is_empty()
    }

    /// Volume of the enclosing box.
    pub fn volume(&self) -> u64 {
        self.size.iter().map(|&d| d as u64).product()
    }

    /// Placed count of each entry of `names`.
    pub fn counts(&self) -> Vec<u64> {
        let mut counts = vec![0u64; self.names.len()];
        for &b in &self.blocks {
            counts[b as usize] += 1;
        }
        counts
    }

    /// Material list: every block used and how many, most used first.
    pub fn materials(&self) -> Vec<Material> {
        let mut list: Vec<Material> = self
            .names
            .iter()
            .zip(self.counts())
            .map(|(name, count)| Material {
                name: name.clone(),
                count,
            })
            .collect();
        list.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        list
    }

    /// The block at each position, as an ID.
    pub fn name_at(&self, i: usize) -> &str {
        &self.names[self.blocks[i] as usize]
    }
}

/// One past the largest coordinate on each axis; zero for an empty grid.
fn extent(coords: &[[i32; 3]]) -> [u32; 3] {
    let mut size = [0u32; 3];
    for c in coords {
        for axis in 0..3 {
            let past = (c[axis].max(0) as u32) + 1;
            size[axis] = size[axis].max(past);
        }
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BlockGrid {
        BlockGrid::from_names(
            vec![[0, 0, 0], [2, 0, 0], [0, 3, 1], [1, 1, 1]],
            [
                "minecraft:stone",
                "minecraft:dirt",
                "minecraft:stone",
                "minecraft:stone",
            ],
            [0.0; 3],
            1.0,
        )
    }

    #[test]
    fn palette_is_sorted_and_indexed() {
        let g = sample();
        assert_eq!(g.names, ["minecraft:dirt", "minecraft:stone"]);
        assert_eq!(g.blocks, [1, 0, 1, 1]);
        assert_eq!(g.name_at(1), "minecraft:dirt");
    }

    #[test]
    fn indices_that_share_a_name_collapse() {
        let names = ["minecraft:stone", "minecraft:dirt", "minecraft:stone"];
        let g = BlockGrid::from_indices(
            vec![[0, 0, 0], [1, 0, 0], [2, 0, 0]],
            &[2, 1, 0],
            |i| names[i as usize],
            [0.0; 3],
            1.0,
        );
        assert_eq!(g.names, ["minecraft:dirt", "minecraft:stone"]);
        assert_eq!(g.blocks, [1, 0, 1]);
        assert_eq!(
            g,
            BlockGrid::from_names(
                g.coords.clone(),
                ["minecraft:stone", "minecraft:dirt", "minecraft:stone"],
                [0.0; 3],
                1.0
            )
        );
    }

    #[test]
    fn size_is_tight() {
        let g = sample();
        assert_eq!(g.size, [3, 4, 2]);
        assert_eq!(g.volume(), 24);
    }

    #[test]
    fn materials_are_most_used_first() {
        let m = sample().materials();
        assert_eq!(m[0].name, "minecraft:stone");
        assert_eq!(m[0].count, 3);
        assert_eq!(m[1].count, 1);
    }
}
