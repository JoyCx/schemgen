//! Forgetting old work: finished jobs and their files after the job TTL, and
//! files earlier runs of the server left in its work folders — jobs live in
//! memory, so a restart used to strand every upload and output for good.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::state::AppState;

/// Sweep now, then every quarter of the TTL (between 30 s and 10 min).
pub fn spawn(app: Arc<AppState>) {
    let Some(ttl) = app.job_ttl else { return };
    let every = (ttl / 4).clamp(Duration::from_secs(30), Duration::from_secs(600));
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        loop {
            tick.tick().await;
            let app = Arc::clone(&app);
            let removed = tokio::task::spawn_blocking(move || sweep(&app, ttl))
                .await
                .unwrap_or(0);
            if removed > 0 {
                log::info!("Swept {removed} expired job(s) and file(s)");
            }
        }
    });
}

/// Remove what is older than `ttl`; returns how many jobs and stray files went.
pub fn sweep(app: &AppState, ttl: Duration) -> usize {
    let cutoff_ms = schemgen_core::formats::now_ms() - ttl.as_millis() as i64;
    let mut removed = 0;

    for job in app.jobs.list() {
        let state = job.state();
        if state.status.is_finished() && state.finished_ms.is_some_and(|t| t < cutoff_ms) {
            app.jobs.remove(&job.id);
            job.remove_files();
            removed += 1;
        }
    }

    let live: HashSet<PathBuf> = app
        .jobs
        .list()
        .iter()
        .flat_map(|j| [j.upload.clone(), j.output.clone(), j.thumbnail.clone()])
        .collect();
    let cutoff = SystemTime::now()
        .checked_sub(ttl)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for dir in [&app.uploads, &app.outputs] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|modified| modified < cutoff);
            if stale
                && path.is_file()
                && !live.contains(&path)
                && std::fs::remove_file(&path).is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
}
