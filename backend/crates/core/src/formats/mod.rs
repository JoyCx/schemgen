//! Schematic writers. Each turns a [`crate::BlockGrid`] into a file for a
//! [`crate::Target`].

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

pub mod litematic;
pub mod nbt;

/// What a schematic says about itself.
#[derive(Debug, Clone)]
pub struct Metadata {
    pub name: String,
    pub author: String,
    pub description: String,
    /// Creation time, milliseconds since the Unix epoch.
    pub time_ms: i64,
}

impl Metadata {
    /// Metadata for a conversion made now.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            author: "SchemGen2".to_string(),
            description: "Converted from a 3D model by SchemGen2".to_string(),
            time_ms: now_ms(),
        }
    }
}

/// The current time in milliseconds — or `SOURCE_DATE_EPOCH` (seconds, the
/// reproducible-builds convention) when it is set, so the same model and
/// settings can still produce a byte-identical file.
pub fn now_ms() -> i64 {
    if let Some(secs) = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
    {
        return secs.saturating_mul(1000);
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Write a file through `encode`, atomically: into a temporary file beside
/// `path`, renamed over it only once complete. A reader — Litematica watching
/// its schematics folder, say — never sees half a file, and a failed write
/// leaves nothing behind.
pub fn write_file(
    path: &Path,
    encode: impl FnOnce(&mut io::BufWriter<std::fs::File>) -> io::Result<()>,
) -> Result<()> {
    let tmp = temp_path(path);
    let result = (|| {
        let file = std::fs::File::create(&tmp)?;
        let mut out = io::BufWriter::with_capacity(1 << 16, file);
        encode(&mut out)?;
        out.flush()?;
        drop(out);
        std::fs::rename(&tmp, path)
    })();
    result.map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        Error::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// A sibling of `path` no other writer will pick: hidden, and unique per
/// process and call.
pub fn temp_path(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.{n}.partial", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_writes_leave_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("schemgen_wf_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.bin");
        let err = write_file(&path, |out| {
            out.write_all(b"partial")?;
            Err(io::Error::other("boom"))
        });
        assert!(err.is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn temp_paths_are_unique_siblings() {
        let p = Path::new("/tmp/out/castle.litematic");
        let (a, b) = (temp_path(p), temp_path(p));
        assert_ne!(a, b);
        assert_eq!(a.parent(), p.parent());
    }
}
