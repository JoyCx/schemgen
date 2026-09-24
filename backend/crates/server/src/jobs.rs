//! Conversion jobs: what each one is, where its files are, and running it.
//!
//! A job's changing state lives in a [`watch`] channel. Pollers read the
//! latest value; event streams subscribe and are woken on every change, and a
//! subscriber that arrives late still starts from the current state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use serde::Serialize;
use tokio::sync::{watch, Semaphore};

use schemgen_core::formats::{self, Metadata};
use schemgen_core::{pipeline, thumbnail, Cancel, Material, PaletteSet, Progress, Settings, Stage};

use crate::savedir;
use crate::state::AppState;

/// Edge of the thumbnail rendered for every finished job.
pub const THUMBNAIL_SIZE: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Waiting for a free conversion slot.
    Queued,
    Running,
    Done,
    Error,
    Cancelled,
}

impl Status {
    pub fn is_finished(self) -> bool {
        matches!(self, Status::Done | Status::Error | Status::Cancelled)
    }
}

/// What a finished conversion produced.
#[derive(Debug, Clone, Serialize)]
pub struct JobResult {
    /// Blocks placed.
    pub blocks: usize,
    /// Distinct block types.
    pub unique_blocks: usize,
    /// Schematic size, x × y × z.
    pub dims: [u32; 3],
    pub seconds: f32,
    /// Every block used and how many, most used first.
    pub materials: Vec<Material>,
    pub target: String,
    pub data_version: i32,
}

/// The part of a job that changes while it runs.
#[derive(Debug, Clone, Serialize)]
pub struct JobState {
    pub status: Status,
    /// 0–100. Stays below 100 until the file is written and delivered.
    pub progress: f32,
    pub stage: &'static str,
    pub message: String,
    pub error: Option<String>,
    pub result: Option<JobResult>,
    /// Where the schematic was copied, when an output folder was given.
    pub saved_path: Option<String>,
    /// Why that copy failed, if it did. The conversion itself still succeeded.
    pub save_error: Option<String>,
    pub finished_ms: Option<i64>,
}

impl JobState {
    fn queued() -> Self {
        Self {
            status: Status::Queued,
            progress: 0.0,
            stage: "queued",
            message: "Queued…".to_string(),
            error: None,
            result: None,
            saved_path: None,
            save_error: None,
            finished_ms: None,
        }
    }
}

/// One conversion: a model, its settings and its files.
pub struct Job {
    pub id: String,
    pub created_ms: i64,
    /// File name the model was uploaded as.
    pub input_name: String,
    /// Schematic name.
    pub name: String,
    /// Name the schematic downloads and is delivered as.
    pub file_name: String,
    pub settings: Settings,
    /// Folder to copy the finished schematic into.
    pub deliver_to: Option<PathBuf>,
    pub upload: PathBuf,
    pub output: PathBuf,
    pub thumbnail: PathBuf,
    pub cancel: Cancel,
    state: watch::Sender<JobState>,
}

/// Everything a new job needs besides the server's folders.
pub struct NewJob {
    pub input_name: String,
    pub upload: PathBuf,
    pub settings: Settings,
    pub name: String,
    pub file_name: String,
    pub deliver_to: Option<PathBuf>,
}

impl Job {
    pub fn new(outputs: &Path, new: NewJob) -> Arc<Job> {
        let id = uuid::Uuid::new_v4().to_string();
        let (state, _) = watch::channel(JobState::queued());
        Arc::new(Job {
            output: outputs.join(format!("{id}.{}", new.settings.format().extension())),
            thumbnail: outputs.join(format!("{id}.png")),
            id,
            created_ms: formats::now_ms(),
            input_name: new.input_name,
            name: new.name,
            file_name: new.file_name,
            settings: new.settings,
            deliver_to: new.deliver_to,
            upload: new.upload,
            cancel: Cancel::new(),
            state,
        })
    }

    pub fn state(&self) -> JobState {
        self.state.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<JobState> {
        self.state.subscribe()
    }

    pub(crate) fn update(&self, change: impl FnOnce(&mut JobState)) {
        self.state.send_modify(change);
    }

    /// Everything a client may want to know, as one serializable value.
    pub fn view(&self) -> JobView {
        self.view_of(self.state())
    }

    pub fn view_of(&self, state: JobState) -> JobView {
        let base = format!("/api/jobs/{}", self.id);
        JobView {
            id: self.id.clone(),
            status: state.status,
            progress: state.progress,
            stage: state.stage,
            message: state.message,
            input_name: self.input_name.clone(),
            name: self.name.clone(),
            download_name: self.file_name.clone(),
            format: self.settings.format().id(),
            target: self.settings.target.clone(),
            created_ms: self.created_ms,
            finished_ms: state.finished_ms,
            error: state.error,
            result: state.result,
            saved_path: state.saved_path,
            save_error: state.save_error,
            links: Links {
                download: format!("{base}/download"),
                thumbnail: format!("{base}/thumbnail.png"),
                events: format!("{base}/events"),
            },
        }
    }

    /// Ask the job to stop. A queued job is cancelled at once; a running one
    /// stops at the pipeline's next checkpoint and reports `cancelled` then.
    pub fn request_cancel(&self) {
        self.cancel.cancel();
        let mut was_queued = false;
        self.update(|s| {
            if s.status == Status::Queued {
                s.status = Status::Cancelled;
                s.stage = "cancelled";
                s.message = "Cancelled".to_string();
                s.finished_ms = Some(formats::now_ms());
                was_queued = true;
            }
        });
        // A queued job never started, so nothing else will clean up after it.
        if was_queued {
            self.remove_files();
        }
    }

    /// Delete every file this job owns.
    pub fn remove_files(&self) {
        for path in [&self.upload, &self.output, &self.thumbnail] {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Links {
    pub download: String,
    pub thumbnail: String,
    pub events: String,
}

/// A job as the API returns it.
#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    pub id: String,
    pub status: Status,
    pub progress: f32,
    pub stage: &'static str,
    pub message: String,
    pub input_name: String,
    pub name: String,
    pub download_name: String,
    pub format: &'static str,
    pub target: String,
    pub created_ms: i64,
    pub finished_ms: Option<i64>,
    pub error: Option<String>,
    pub result: Option<JobResult>,
    pub saved_path: Option<String>,
    pub save_error: Option<String>,
    pub links: Links,
}

/// Every job this server knows, by id.
#[derive(Default)]
pub struct JobStore {
    jobs: RwLock<HashMap<String, Arc<Job>>>,
}

impl JobStore {
    pub fn insert(&self, job: Arc<Job>) {
        self.jobs
            .write()
            .expect("job store poisoned")
            .insert(job.id.clone(), job);
    }

    pub fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs
            .read()
            .expect("job store poisoned")
            .get(id)
            .cloned()
    }

    pub fn remove(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs.write().expect("job store poisoned").remove(id)
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Arc<Job>> {
        let mut jobs: Vec<Arc<Job>> = self
            .jobs
            .read()
            .expect("job store poisoned")
            .values()
            .cloned()
            .collect();
        jobs.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(a.id.cmp(&b.id)));
        jobs
    }
}

/// Reports pipeline progress into the job; pipeline progress fills 0–90%,
/// writing and delivering the file the rest.
struct JobProgress<'a> {
    job: &'a Job,
}

impl Progress for JobProgress<'_> {
    fn update(&mut self, stage: Stage, fraction: f32, message: &str) {
        let pct = (fraction.clamp(0.0, 1.0) * 900.0).round() / 10.0;
        let message = message.to_string();
        self.job.update(|s| {
            if s.status == Status::Running {
                s.progress = s.progress.max(pct);
                s.stage = stage.as_str();
                s.message = message;
            }
        });
    }

    fn is_cancelled(&self) -> bool {
        self.job.cancel.is_cancelled()
    }
}

/// Run `jobs`, at most `concurrency` of them at once and never more than the
/// server's own limit across all requests.
pub fn start(app: Arc<AppState>, jobs: Vec<Arc<Job>>, concurrency: usize) {
    let local = Arc::new(Semaphore::new(concurrency.max(1)));
    for job in jobs {
        let app = Arc::clone(&app);
        let local = Arc::clone(&local);
        tokio::spawn(async move {
            let _local = local.acquire_owned().await;
            let _global = Arc::clone(&app.slots).acquire_owned().await;
            // Start only if nobody cancelled while this job waited; checking
            // inside the update makes the check and the transition one step.
            let mut started = false;
            job.update(|s| {
                if s.status == Status::Queued && !job.cancel.is_cancelled() {
                    s.status = Status::Running;
                    s.stage = "start";
                    s.message = "Starting…".to_string();
                    started = true;
                }
            });
            if !started {
                job.request_cancel();
                return;
            }
            let palettes = Arc::clone(&app.palettes);
            let worker = Arc::clone(&job);
            let outcome = tokio::task::spawn_blocking(move || convert(&worker, &palettes))
                .await
                .unwrap_or_else(|e| {
                    Err(schemgen_core::Error::Io(std::io::Error::other(format!(
                        "conversion task failed: {e}"
                    ))))
                });
            finish(&job, outcome).await;
        });
    }
}

/// The blocking part of a job: pipeline, file, thumbnail.
fn convert(job: &Job, palettes: &PaletteSet) -> schemgen_core::Result<JobResult> {
    let started = Instant::now();
    let target = job.settings.target();
    let palette = palettes.for_target(&target)?;
    let grid = pipeline::run(
        &job.upload,
        &job.settings,
        &palette,
        &mut JobProgress { job },
    )?;
    if job.cancel.is_cancelled() {
        return Err(schemgen_core::Error::Cancelled);
    }

    let format = job.settings.format();
    job.update(|s| {
        s.progress = 92.0;
        s.stage = "write";
        s.message = format!("Writing .{}…", format.extension());
    });
    format.write(&job.output, &grid, &Metadata::new(&job.name), &target)?;

    job.update(|s| {
        s.progress = 96.0;
        s.stage = "thumbnail";
        s.message = "Rendering thumbnail…".to_string();
    });
    let png = thumbnail::render_png(&grid, |n| palettes.color_of(n), THUMBNAIL_SIZE);
    if let Err(e) = std::fs::write(&job.thumbnail, png) {
        log::warn!("Job {}: could not save thumbnail: {e}", job.id);
    }

    Ok(JobResult {
        blocks: grid.len(),
        unique_blocks: grid.names.len(),
        dims: grid.size,
        seconds: started.elapsed().as_secs_f32(),
        materials: grid.materials(),
        target: target.key(),
        data_version: target.data_version,
    })
}

/// Record the outcome. A successful schematic is delivered to its folder
/// *before* the job reports done, so a client reacting to `done` always finds
/// the file in place.
async fn finish(job: &Arc<Job>, outcome: schemgen_core::Result<JobResult>) {
    let _ = std::fs::remove_file(&job.upload);
    let now = formats::now_ms();
    match outcome {
        Ok(result) => {
            let delivered = match &job.deliver_to {
                Some(dir) => {
                    let (src, dir, name) = (job.output.clone(), dir.clone(), job.file_name.clone());
                    Some(
                        tokio::task::spawn_blocking(move || savedir::deliver(&src, &dir, &name))
                            .await
                            .unwrap_or_else(|e| Err(format!("copy task failed: {e}"))),
                    )
                }
                None => None,
            };
            let mut message = format!(
                "Done! {} blocks, {} unique, {:.1}s",
                result.blocks, result.unique_blocks, result.seconds
            );
            let (saved_path, save_error) = match delivered {
                Some(Ok(dest)) => {
                    message.push_str(&format!(" → saved to {}", dest.display()));
                    (Some(dest.display().to_string()), None)
                }
                Some(Err(e)) => {
                    log::error!("Job {}: save to output folder failed: {e}", job.id);
                    message.push_str(&format!(" — but saving to your folder failed: {e}"));
                    (None, Some(e))
                }
                None => (None, None),
            };
            log::info!("Job {} ({}): {message}", job.id, job.input_name);
            job.update(|s| {
                s.status = Status::Done;
                s.progress = 100.0;
                s.stage = "done";
                s.message = message;
                s.result = Some(result);
                s.saved_path = saved_path;
                s.save_error = save_error;
                s.finished_ms = Some(now);
            });
        }
        Err(schemgen_core::Error::Cancelled) => {
            job.remove_files();
            log::info!("Job {} ({}): cancelled", job.id, job.input_name);
            job.update(|s| {
                s.status = Status::Cancelled;
                s.stage = "cancelled";
                s.message = "Cancelled".to_string();
                s.finished_ms.get_or_insert(now);
            });
        }
        Err(e) => {
            let _ = std::fs::remove_file(&job.output);
            let _ = std::fs::remove_file(&job.thumbnail);
            let text = e.to_string();
            log::error!("Job {} ({}): {text}", job.id, job.input_name);
            job.update(|s| {
                s.status = Status::Error;
                s.stage = "error";
                s.message = text.clone();
                s.error = Some(text);
                s.finished_ms = Some(now);
            });
        }
    }
}
