//! SchemGen2 conversion core.
//!
//! Everything that turns a mesh into blocks lives here, with no opinion on
//! where files come from or where they go: settings and their schema, the
//! palette and CIEDE2000 matching, the pipeline, and the schematic writers.
//! The HTTP server and the command line are thin layers on top, which is what
//! keeps a given model and settings converting to the same blocks through
//! either door.
//!
//! ```text
//! voxelize → sample colors → adjust → dither → match → BlockGrid → formats::*
//! ```

pub mod availability;
pub mod blocks;
pub mod color_table;
pub mod dither;
pub mod error;
pub mod formats;
pub mod grid;
pub mod mesh;
pub mod palette;
pub mod pipeline;
pub mod rng;
pub mod sample;
pub mod schema;
pub mod settings;
pub mod targets;
pub mod thumbnail;
pub mod types;
pub mod voxel;
pub mod voxelizer;

pub use availability::{BlockVersions, PaletteSet};
pub use error::{Error, Result};
pub use formats::Format;
pub use grid::{BlockGrid, Material};
pub use palette::Palette;
pub use pipeline::{run, Cancel, NoProgress, Progress, Stage};
pub use settings::Settings;
pub use targets::Target;

/// The SchemGen2 release this core belongs to.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
