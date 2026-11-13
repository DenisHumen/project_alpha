// SPDX-License-Identifier: MPL-2.0
//! CORE-011 "done when": a job survives a killed process and resumes its
//! download from the checkpoint.
//!
//! The test runs itself as a child process (`kill_child_entry`), which
//! starts copying a file chunk by chunk like a resumable download. The
//! parent kills it with SIGKILL in the middle, reopens the database and
//! checks that the copy continues from the saved offset and the result is
//! byte-identical.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pa_jobs::serde_json::json;
use pa_jobs::{JobContext, JobQueue, JobState, NewJob, QueueConfig, RunOutcome};

const CHILD_ENV: &str = "PA_JOBS_KILL_CHILD_DB";
const KIND: &str = "offline_copy";
const SIZE: usize = 256 * 1024;
const CHUNK: usize = 4096;

/// A resumable copy: continues `dst.part` from the checkpoint offset and
/// renames it to `dst` at the end.
fn copy_handler(
    delay: Duration,
    starts: Arc<Mutex<Vec<u64>>>,
) -> impl Fn(&JobContext) -> RunOutcome + Send + Sync + 'static {
    move |ctx: &JobContext| {
        let src = PathBuf::from(ctx.params()["src"].as_str().unwrap());
        let dst = PathBuf::from(ctx.params()["dst"].as_str().unwrap());
        let part = dst.with_extension("part");
        let mut offset = ctx
            .checkpoint()
            .and_then(|c| c["offset"].as_u64())
            .unwrap_or(0);
        starts.lock().unwrap().push(offset);
        let mut run = || -> std::io::Result<RunOutcome> {
            let total = fs::metadata(&src)?.len();
            let mut out = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&part)?;
            // Bytes after the checkpoint may be torn: drop them.
            out.set_len(offset)?;
            out.seek(SeekFrom::Start(offset))?;
            let mut input = File::open(&src)?;
            input.seek(SeekFrom::Start(offset))?;
            let mut buf = vec![0; CHUNK];
            loop {
                if ctx.stop_requested() {
                    return Ok(RunOutcome::Interrupted);
                }
                let n = input.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                out.sync_data()?;
                offset += n as u64;
                let running = ctx
                    .save_checkpoint(&json!({ "offset": offset }), offset)
                    .map_err(std::io::Error::other)?;
                if !running {
                    return Ok(RunOutcome::Interrupted);
                }
                ctx.report_bytes(offset, Some(total));
                thread::sleep(delay);
            }
            drop(out);
            fs::rename(&part, &dst)?;
            Ok(RunOutcome::Done(Some(json!({ "bytes": offset }))))
        };
        run().unwrap_or_else(|e| RunOutcome::Retryable(e.to_string()))
    }
}

fn config() -> QueueConfig {
    QueueConfig {
        progress_interval: Duration::ZERO,
        tick_interval: Duration::from_millis(20),
        ..QueueConfig::default()
    }
}

/// Entry point of the child process; a no-op in a normal test run.
#[test]
fn kill_child_entry() {
    let Ok(db) = std::env::var(CHILD_ENV) else {
        return;
    };
    let queue = JobQueue::open(&db, config()).unwrap();
    queue.register(
        KIND,
        copy_handler(Duration::from_millis(25), Arc::default()),
    );
    let _driver = queue.start();
    // Killed by the parent long before this.
    thread::sleep(Duration::from_secs(120));
    panic!("the parent did not kill the child");
}

fn bytes_done(db: &Path) -> (String, u64) {
    let conn = pa_jobs::JobStore::open(db).unwrap();
    let job = &conn.list(&[]).unwrap()[0];
    (job.state.to_string(), job.bytes_done)
}

#[test]
fn job_survives_kill_and_resumes_download() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.db");
    let src = dir.path().join("source.bin");
    let dst = dir.path().join("copy.bin");
    let data: Vec<u8> = (0..SIZE).map(|i| (i * 31 % 251) as u8).collect();
    fs::write(&src, &data).unwrap();

    // Enqueue in a first "process", without handlers.
    let id = {
        let queue = JobQueue::open(&db, config()).unwrap();
        queue
            .enqueue(&NewJob::new(KIND, json!({ "src": src, "dst": dst })).bytes_total(SIZE as u64))
            .unwrap()
    };

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "kill_child_entry",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, &db)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    // Wait until a part of the file is copied, then kill -9.
    let deadline = Instant::now() + Duration::from_secs(60);
    let killed_at = loop {
        let (state, done) = bytes_done(&db);
        if state == "running" && done >= (SIZE / 4) as u64 {
            child.kill().unwrap();
            child.wait().unwrap();
            break bytes_done(&db).1;
        }
        assert!(Instant::now() < deadline, "child made no progress");
        assert!(child.try_wait().unwrap().is_none(), "child exited early");
        thread::sleep(Duration::from_millis(10));
    };
    assert!(killed_at > 0 && killed_at < SIZE as u64, "{killed_at}");
    assert_eq!(
        bytes_done(&db).0,
        "running",
        "the dead process left it running"
    );
    assert!(!dst.exists());

    // Restart: the job is queued again with its checkpoint and resumes.
    let starts = Arc::new(Mutex::new(Vec::new()));
    let queue = JobQueue::open(&db, config()).unwrap();
    let job = queue.get(id).unwrap();
    assert_eq!(job.state, JobState::Queued);
    assert_eq!(job.bytes_done, killed_at);
    assert_eq!(job.checkpoint, Some(json!({ "offset": killed_at })));

    queue.register(KIND, copy_handler(Duration::ZERO, Arc::clone(&starts)));
    assert!(queue.run_until_idle(Duration::from_secs(30)).unwrap());

    let job = queue.get(id).unwrap();
    assert_eq!(job.state, JobState::Done, "{job:?}");
    assert_eq!(*starts.lock().unwrap(), vec![killed_at]);
    // The killed run is not a failed attempt.
    assert_eq!(job.attempts, 1);
    assert_eq!(job.bytes_done, SIZE as u64);
    assert_eq!(job.progress, Some(1.0));
    assert_eq!(fs::read(&dst).unwrap(), data);
    assert!(!dst.with_extension("part").exists());
}

#[test]
fn shutdown_interrupts_and_next_start_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.db");
    let src = dir.path().join("source.bin");
    let dst = dir.path().join("copy.bin");
    let data: Vec<u8> = (0..SIZE).map(|i| (i * 7 % 253) as u8).collect();
    fs::write(&src, &data).unwrap();

    let starts = Arc::new(Mutex::new(Vec::new()));
    let id;
    {
        let queue = JobQueue::open(&db, config()).unwrap();
        queue.register(
            KIND,
            copy_handler(Duration::from_millis(5), Arc::clone(&starts)),
        );
        id = queue
            .enqueue(&NewJob::new(KIND, json!({ "src": src, "dst": dst })))
            .unwrap();
        let driver = queue.start();
        let deadline = Instant::now() + Duration::from_secs(30);
        while queue.get(id).unwrap().bytes_done < CHUNK as u64 * 4 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        drop(driver);
        assert!(queue.shutdown(Duration::from_secs(10)));
        let job = queue.get(id).unwrap();
        assert_eq!(job.state, JobState::Queued, "graceful stop requeues");
        assert_eq!(job.attempts, 0);
    }

    let queue = JobQueue::open(&db, config()).unwrap();
    queue.register(KIND, copy_handler(Duration::ZERO, Arc::clone(&starts)));
    assert!(queue.run_until_idle(Duration::from_secs(30)).unwrap());
    assert_eq!(queue.get(id).unwrap().state, JobState::Done);
    let starts = starts.lock().unwrap();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0], 0);
    assert!(starts[1] > 0);
    assert_eq!(fs::read(&dst).unwrap(), data);
}
