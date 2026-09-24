//! Server-sent events for one job: `GET /api/jobs/{id}/events`.
//!
//! The stream opens with the job's current state, then sends one event per
//! change: `progress` while it runs, and exactly one of `done`, `error` or
//! `cancelled` as the last event before the stream closes. Every event's data
//! is the same JSON object `GET /api/jobs/{id}` returns. A comment line goes
//! out every 15 s so proxies do not close a quiet connection.

use std::sync::Arc;
use std::time::Duration;

use actix_web::web::Bytes;
use actix_web::HttpResponse;
use futures_util::stream;
use tokio::sync::watch;

use crate::jobs::{Job, JobState, Status};

const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// How long a client waits before reconnecting after a dropped stream.
const RETRY_MS: u32 = 3000;

struct Cursor {
    job: Arc<Job>,
    rx: watch::Receiver<JobState>,
    first: bool,
    finished: bool,
}

pub fn events(job: Arc<Job>) -> HttpResponse {
    let rx = job.subscribe();
    let start = Cursor {
        job,
        rx,
        first: true,
        finished: false,
    };
    let body = stream::unfold(start, |mut c| async move {
        if c.finished {
            return None;
        }
        let mut prefix = String::new();
        if c.first {
            c.first = false;
            prefix = format!("retry: {RETRY_MS}\n\n");
        } else {
            match tokio::time::timeout(KEEP_ALIVE, c.rx.changed()).await {
                Ok(Ok(())) => {}
                // The job is gone; nothing more will happen.
                Ok(Err(_)) => return None,
                Err(_) => return Some((Ok(Bytes::from_static(b": keep-alive\n\n")), c)),
            }
        }
        let state = c.rx.borrow_and_update().clone();
        let event = match state.status {
            Status::Done => "done",
            Status::Error => "error",
            Status::Cancelled => "cancelled",
            Status::Queued | Status::Running => "progress",
        };
        c.finished = state.status.is_finished();
        let data = serde_json::to_string(&c.job.view_of(state)).unwrap_or_else(|_| "{}".into());
        let frame = format!("{prefix}event: {event}\ndata: {data}\n\n");
        Some((Ok::<_, actix_web::Error>(Bytes::from(frame)), c))
    });

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(body)
}
