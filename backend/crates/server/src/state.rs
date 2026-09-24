//! What every request handler shares.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::Semaphore;

use schemgen_core::{PaletteSet, Settings};

use crate::jobs::JobStore;
use crate::textures::Textures;

pub struct AppState {
    /// The color table, and each target's palette from it.
    pub palettes: Arc<PaletteSet>,
    pub jobs: JobStore,
    /// Uploaded models waiting to be converted.
    pub uploads: PathBuf,
    /// Finished schematics and thumbnails.
    pub outputs: PathBuf,
    /// Settings a request starts from before its own are applied.
    pub defaults: Settings,
    /// Bearer token every API call but `/api/health` must carry, when set.
    pub token: Option<String>,
    /// Host names requests may be addressed to (DNS-rebinding defense).
    pub allowed_hosts: Vec<String>,
    /// How long finished jobs and their files are kept.
    pub job_ttl: Option<Duration>,
    /// Conversions allowed to run at the same time, server-wide.
    pub slots: Arc<Semaphore>,
    /// Largest single upload accepted, in bytes.
    pub max_upload: u64,
    /// Block textures for the preview; found on first use.
    pub textures: OnceLock<Textures>,
    /// Where `--textures` said to take them from.
    pub textures_path: Option<PathBuf>,
}

impl AppState {
    /// The block textures, looking for them the first time they are wanted.
    pub fn textures(&self) -> &Textures {
        self.textures
            .get_or_init(|| Textures::discover(self.textures_path.as_deref()))
    }
}
