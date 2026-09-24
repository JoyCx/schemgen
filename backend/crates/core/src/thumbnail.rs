//! A small isometric picture of a [`BlockGrid`]: flat-shaded cubes, drawn
//! the way Minecraft maps and item icons draw blocks.
//!
//! The projection is 2:1 pixel isometric, viewed from +X, +Y, +Z: a block is
//! a hexagon `2u` pixels wide and `2u` tall, made of its top face and its +Z
//! (left) and +X (right) sides. With `u` even every corner lands on a whole
//! pixel, so each block is the same sprite stamped at an integer offset.
//! Blocks are stamped in increasing `x + y + z`, which for grid-aligned cubes
//! seen along (1, 1, 1) is exactly back to front. The picture is drawn larger
//! than asked for and scaled down, which is what smooths its edges.

use image::imageops::FilterType;
use image::{Rgba, RgbaImage};

use crate::grid::BlockGrid;

/// Longest side of the full-size drawing.
const MAX_RENDER: f64 = 4096.0;

/// Brightness of each visible face, as block shading does in game.
const SHADE_TOP: f32 = 1.0;
const SHADE_LEFT: f32 = 0.80;
const SHADE_RIGHT: f32 = 0.62;

/// Draw `grid` into a `size` × `size` image with a transparent background.
/// `color_of` gives each block ID's color; blocks it does not know are grey.
pub fn render(
    grid: &BlockGrid,
    color_of: impl Fn(&str) -> Option<[u8; 3]>,
    size: u32,
) -> RgbaImage {
    let size = size.max(8);
    let mut canvas = RgbaImage::new(size, size);
    if grid.is_empty() {
        return canvas;
    }

    let colors: Vec<[[u8; 4]; 3]> = grid
        .names
        .iter()
        .map(|n| {
            let c = color_of(n).unwrap_or([150, 150, 150]);
            [SHADE_TOP, SHADE_LEFT, SHADE_RIGHT].map(|s| {
                let ch = |v: u8| (v as f32 * s).round().clamp(0.0, 255.0) as u8;
                [ch(c[0]), ch(c[1]), ch(c[2]), 255]
            })
        })
        .collect();

    let [sx, sy, sz] = grid.size.map(|d| d as i64);
    // Extent in units of u.
    let span_w = (sx + sz) as f64;
    let span_h = (sx + sz) as f64 / 2.0 + sy as f64;
    let span = span_w.max(span_h);
    // Aim for about twice the output size, for smooth edges once scaled down.
    let wanted = (2.0 * size as f64 / span).min(MAX_RENDER / span);

    let drawing = if wanted >= 2.0 {
        let u = ((wanted / 2.0).floor() as i64 * 2).max(2);
        draw_sprites(grid, &colors, u)
    } else {
        draw_points(
            grid,
            &colors,
            (MAX_RENDER / span).min(2.0 * size as f64 / span),
        )
    };

    // Fit inside the canvas with a small margin, keeping proportions.
    let margin = (size as f64 * 0.04).round();
    let room = size as f64 - 2.0 * margin;
    let (dw, dh) = (drawing.width() as f64, drawing.height() as f64);
    let scale = (room / dw).min(room / dh);
    let (w, h) = (
        ((dw * scale).round() as u32).clamp(1, size),
        ((dh * scale).round() as u32).clamp(1, size),
    );
    let fitted = image::imageops::resize(&premultiply(drawing), w, h, FilterType::Triangle);
    let fitted = unpremultiply(fitted);
    image::imageops::overlay(
        &mut canvas,
        &fitted,
        ((size - w) / 2) as i64,
        ((size - h) / 2) as i64,
    );
    canvas
}

/// [`render`], encoded as PNG.
pub fn render_png(
    grid: &BlockGrid,
    color_of: impl Fn(&str) -> Option<[u8; 3]>,
    size: u32,
) -> Vec<u8> {
    let image = render(grid, color_of, size);
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encoding a PNG in memory cannot fail");
    png
}

/// Blocks in back-to-front order.
fn painter_order(grid: &BlockGrid) -> Vec<u32> {
    let mut order: Vec<u32> = (0..grid.len() as u32).collect();
    order.sort_by_key(|&i| {
        let [x, y, z] = grid.coords[i as usize];
        x as i64 + y as i64 + z as i64
    });
    order
}

/// Screen position of a block's top-left corner of its bounding box, in
/// units of u, before offsetting into the image: the hexagon's leftmost
/// point is at x - z - 1 and its top at (x + z) / 2 - y - 1.
fn project(c: [i32; 3], u: i64) -> (i64, i64) {
    let [x, y, z] = c.map(|v| v as i64);
    ((x - z) * u, (x + z) * u / 2 - y * u)
}

/// Full-quality drawing: every block is the same `2u` × `2u` sprite.
fn draw_sprites(grid: &BlockGrid, colors: &[[[u8; 4]; 3]], u: i64) -> RgbaImage {
    let sprite = sprite(u);
    let (min_x, min_y, max_x, max_y) = bounds(grid, u);
    let (w, h) = ((max_x - min_x) as u32, (max_y - min_y) as u32);
    let mut img = RgbaImage::new(w.max(1), h.max(1));
    let side = 2 * u;

    for i in painter_order(grid) {
        let i = i as usize;
        let (px, py) = project(grid.coords[i], u);
        let (ox, oy) = (px - u - min_x, py - u - min_y);
        let faces = &colors[grid.blocks[i] as usize];
        for (k, &face) in sprite.iter().enumerate() {
            if face == 0 {
                continue;
            }
            let (x, y) = (ox + k as i64 % side, oy + k as i64 / side);
            if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
                img.put_pixel(x as u32, y as u32, Rgba(faces[face as usize - 1]));
            }
        }
    }
    img
}

/// Pixel bounds of the whole drawing at unit `u`: every block's sprite spans
/// `[px - u, px + u)` horizontally and `[py - u, py + u)` vertically.
fn bounds(grid: &BlockGrid, u: i64) -> (i64, i64, i64, i64) {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for &c in &grid.coords {
        let (px, py) = project(c, u);
        min_x = min_x.min(px - u);
        max_x = max_x.max(px + u);
        min_y = min_y.min(py - u);
        max_y = max_y.max(py + u);
    }
    (min_x, min_y, max_x, max_y)
}

/// Which face each pixel of a block's `2u` × `2u` box shows: 0 none, 1 top,
/// 2 left (+Z), 3 right (+X).
///
/// Relative to the box, the top face is the rhombus with corners (0, u/2),
/// (u, 0), (2u, u/2) and (u, u); the sides hang from its lower edges down to
/// the bottom corner (u, 2u). Pixel centers decide membership.
fn sprite(u: i64) -> Vec<u8> {
    let side = 2 * u;
    let mut px = vec![0u8; (side * side) as usize];
    let uf = u as f64;
    for y in 0..side {
        for x in 0..side {
            let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
            // Distance from the vertical center line.
            let dx = (cx - uf).abs();
            // The hexagon lies between its upper edges (y = dx/2) and its
            // lower ones (y = 2u - dx/2); the top face is the rhombus in its
            // upper half, and what is left is the two sides.
            let in_hexagon = cy >= dx / 2.0 && cy <= 2.0 * uf - dx / 2.0;
            let in_top = dx / 2.0 + (cy - uf / 2.0).abs() <= uf / 2.0;
            let face = if !in_hexagon {
                0
            } else if in_top {
                1
            } else if cx < uf {
                2
            } else {
                3
            };
            px[(y * side + x) as usize] = face;
        }
    }
    px
}

/// Drawing for grids too big for sprites: one pixel per block at the block's
/// center, colored by its top face, still back to front.
fn draw_points(grid: &BlockGrid, colors: &[[[u8; 4]; 3]], scale: f64) -> RgbaImage {
    let scale = scale.max(1e-3);
    let place = |c: [i32; 3]| {
        let [x, y, z] = c.map(|v| v as f64);
        ((x - z) * scale, ((x + z) / 2.0 - y) * scale)
    };
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &c in &grid.coords {
        let (x, y) = place(c);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let w = (max_x - min_x).ceil() as u32 + 1;
    let h = (max_y - min_y).ceil() as u32 + 1;
    let mut img = RgbaImage::new(w, h);
    for i in painter_order(grid) {
        let i = i as usize;
        let (x, y) = place(grid.coords[i]);
        let (x, y) = ((x - min_x) as u32, (y - min_y) as u32);
        img.put_pixel(
            x.min(w - 1),
            y.min(h - 1),
            Rgba(colors[grid.blocks[i] as usize][0]),
        );
    }
    img
}

/// Scaling mixes neighbors; with straight alpha, a transparent black
/// neighbor darkens every edge. Premultiplying first keeps edges true.
fn premultiply(mut img: RgbaImage) -> RgbaImage {
    for p in img.pixels_mut() {
        let a = p[3] as u32;
        for c in 0..3 {
            p[c] = ((p[c] as u32 * a + 127) / 255) as u8;
        }
    }
    img
}

fn unpremultiply(mut img: RgbaImage) -> RgbaImage {
    for p in img.pixels_mut() {
        let a = p[3] as u32;
        for c in 0..3 {
            // Fully transparent pixels (a = 0) are left as they are.
            if let Some(v) = (p[c] as u32 * 255 + a / 2).checked_div(a) {
                p[c] = v.min(255) as u8;
            }
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(n: i32) -> BlockGrid {
        let mut coords = Vec::new();
        for x in 0..n {
            for y in 0..n {
                for z in 0..n {
                    coords.push([x, y, z]);
                }
            }
        }
        let names = vec!["minecraft:red_concrete"; coords.len()];
        BlockGrid::from_names(coords, names, [0.0; 3], 1.0)
    }

    #[test]
    fn sprite_is_a_hexagon_of_three_faces() {
        let s = sprite(4);
        let count = |f: u8| s.iter().filter(|&&p| p == f).count();
        // Top rhombus is half the 8×4 box above it; each side is a
        // parallelogram of the same area.
        assert_eq!(count(1), 16);
        assert_eq!(count(2), 16);
        assert_eq!(count(3), 16);
        // Corners of the box are outside the hexagon.
        assert_eq!(s[0], 0);
        assert_eq!(s[7], 0);
        assert_eq!(s[8 * 7], 0);
    }

    #[test]
    fn draws_shaded_faces_on_transparent_background() {
        let img = render(&cube(4), |_| Some([200, 40, 40]), 64);
        assert_eq!(img.dimensions(), (64, 64));
        assert_eq!(img.get_pixel(0, 0)[3], 0, "corners stay transparent");
        let center = img.get_pixel(32, 32);
        assert_eq!(center[3], 255);
        // Top, left and right faces all appear, brightest on top.
        let reds: std::collections::BTreeSet<u8> =
            img.pixels().filter(|p| p[3] == 255).map(|p| p[0]).collect();
        assert!(
            reds.contains(&200) && reds.contains(&160) && reds.contains(&124),
            "{reds:?}"
        );
    }

    #[test]
    fn front_blocks_hide_back_blocks() {
        // A blue block directly in front of (and hiding) a red one.
        let g = BlockGrid::from_names(
            vec![[0, 0, 0], [1, 1, 1]],
            ["minecraft:red_concrete", "minecraft:blue_concrete"],
            [0.0; 3],
            1.0,
        );
        let img = render(
            &g,
            |n| {
                Some(if n.contains("red") {
                    [255, 0, 0]
                } else {
                    [0, 0, 255]
                })
            },
            64,
        );
        let center = img.get_pixel(32, 32);
        assert!(center[2] > center[0], "front block wins: {center:?}");
    }

    #[test]
    fn huge_grids_fall_back_to_points() {
        let g = BlockGrid::from_names(
            vec![[0, 0, 0], [4000, 0, 0], [0, 3000, 0], [0, 0, 4000]],
            ["minecraft:stone"; 4],
            [0.0; 3],
            1.0,
        );
        let img = render(&g, |_| None, 128);
        assert_eq!(img.dimensions(), (128, 128));
        assert!(img.pixels().any(|p| p[3] > 0));
    }

    #[test]
    fn empty_grid_is_transparent() {
        let g = BlockGrid::from_names(vec![], std::iter::empty(), [0.0; 3], 1.0);
        assert!(render(&g, |_| None, 32).pixels().all(|p| p[3] == 0));
    }

    #[test]
    fn png_bytes_are_png() {
        let png = render_png(&cube(2), |_| Some([1, 2, 3]), 32);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }
}
