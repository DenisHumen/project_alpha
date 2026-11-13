// SPDX-License-Identifier: MPL-2.0
//! Lifecycle of the runtime: start, foreground/background, shutdown, restart.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pa_runtime::{
    LifecycleState, Platform, PlatformDirs, PlatformHost, Runtime, RuntimeConfig, RuntimeError,
    SETTINGS_FILE, SettingsError,
};

fn dirs_in(root: &std::path::Path) -> PlatformDirs {
    PlatformDirs::new(root.join("data"), root.join("config"), root.join("cache"))
}

fn config(root: &std::path::Path) -> RuntimeConfig {
    RuntimeConfig::default()
        .with_dirs(dirs_in(root))
        .with_worker_threads(2)
}

/// Records every lifecycle callback.
#[derive(Default)]
struct RecordingHost {
    dirs: Option<PlatformDirs>,
    seen: Mutex<Vec<LifecycleState>>,
}

impl RecordingHost {
    fn seen(&self) -> Vec<LifecycleState> {
        self.seen.lock().unwrap().clone()
    }
}

impl PlatformHost for RecordingHost {
    fn dirs(&self) -> Option<PlatformDirs> {
        self.dirs.clone()
    }

    fn on_lifecycle_changed(&self, state: LifecycleState) {
        self.seen.lock().unwrap().push(state);
    }
}

#[test]
fn start_creates_dirs_and_settings_db() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = Runtime::start(config(tmp.path())).unwrap();
    assert_eq!(rt.state(), LifecycleState::Foreground);
    for dir in ["data", "config", "cache"] {
        assert!(tmp.path().join(dir).is_dir(), "{dir} not created");
    }
    assert!(tmp.path().join("config").join(SETTINGS_FILE).is_file());
    rt.shutdown().unwrap();
}

#[test]
fn transitions_reach_host_and_subscribers_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    let host = Arc::new(RecordingHost::default());
    let rt = Runtime::start(config(tmp.path()).with_host(host.clone())).unwrap();
    let mut rx = rt.subscribe();

    assert!(rt.enter_background().unwrap());
    assert!(!rt.enter_background().unwrap(), "same state is a no-op");
    assert_eq!(rt.state(), LifecycleState::Background);
    assert!(rx.has_changed().unwrap());
    assert_eq!(*rx.borrow_and_update(), LifecycleState::Background);

    assert!(rt.enter_foreground().unwrap());
    rt.shutdown().unwrap();
    assert_eq!(*rx.borrow_and_update(), LifecycleState::ShutDown);

    assert_eq!(
        host.seen(),
        [
            LifecycleState::Foreground,
            LifecycleState::Background,
            LifecycleState::Foreground,
            LifecycleState::ShutDown,
        ]
    );
}

#[test]
fn can_start_in_background() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = Runtime::start(config(tmp.path()).in_background()).unwrap();
    assert_eq!(rt.state(), LifecycleState::Background);
    rt.shutdown().unwrap();
}

#[test]
fn async_tasks_run_and_see_background() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = Runtime::start(config(tmp.path())).unwrap();

    let answer = rt.block_on(async { 6 * 7 }).unwrap();
    assert_eq!(answer, 42);

    // A subsystem task that waits for the app to go to the background.
    let mut rx = rt.subscribe();
    let task = rt
        .spawn(async move {
            rx.wait_for(|s| *s == LifecycleState::Background)
                .await
                .map(|s| *s)
        })
        .unwrap();
    rt.enter_background().unwrap();
    let seen = rt
        .block_on(async { tokio::time::timeout(Duration::from_secs(5), task).await })
        .unwrap()
        .expect("task finished in time")
        .expect("task did not panic")
        .expect("channel open");
    assert_eq!(seen, LifecycleState::Background);
    rt.shutdown().unwrap();
}

#[test]
fn shut_down_instance_refuses_work() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = Runtime::start(config(tmp.path())).unwrap();
    let clone = rt.clone();
    rt.shutdown().unwrap();
    rt.shutdown().unwrap(); // idempotent

    assert!(clone.is_shut_down());
    assert!(matches!(clone.handle(), Err(RuntimeError::ShutDown)));
    assert!(matches!(clone.spawn(async {}), Err(RuntimeError::ShutDown)));
    assert!(matches!(
        clone.block_on(async {}),
        Err(RuntimeError::ShutDown)
    ));
    assert!(matches!(
        clone.enter_foreground(),
        Err(RuntimeError::ShutDown)
    ));
    assert!(matches!(
        clone.settings().get_value("ui.theme"),
        Err(SettingsError::Closed)
    ));
}

#[test]
fn shutdown_waits_for_running_tasks() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = Runtime::start(config(tmp.path())).unwrap();
    let done = Arc::new(Mutex::new(false));
    let flag = done.clone();
    rt.spawn(async move {
        // A blocking section models a task in the middle of a write.
        tokio::task::spawn_blocking(move || {
            std::thread::sleep(Duration::from_millis(100));
            *flag.lock().unwrap() = true;
        })
        .await
        .unwrap();
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(20));
    rt.shutdown().unwrap();
    assert!(
        *done.lock().unwrap(),
        "shutdown returned before the task finished"
    );
}

#[test]
fn shutdown_from_inside_a_task_does_not_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = Runtime::start(config(tmp.path())).unwrap();
    let inner = rt.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    rt.spawn(async move {
        tx.send(inner.shutdown().is_ok()).unwrap();
    })
    .unwrap();
    let ok = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(ok);
    assert!(rt.is_shut_down());
}

#[test]
fn dropping_last_handle_inside_a_task_does_not_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = Runtime::start(config(tmp.path())).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = rt.handle().unwrap();
    handle.spawn(async move {
        drop(rt);
        tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
}

#[test]
fn restart_after_shutdown_keeps_settings() {
    let tmp = tempfile::tempdir().unwrap();

    let first = Runtime::start(config(tmp.path())).unwrap();
    first.settings().set("ui.theme", "dark").unwrap();
    first.settings().set("player.volume", &75u8).unwrap();
    first.enter_background().unwrap();
    first.shutdown().unwrap();

    let second = Runtime::start(config(tmp.path())).unwrap();
    assert_eq!(second.state(), LifecycleState::Foreground);
    assert_eq!(
        second
            .settings()
            .get::<String>("ui.theme")
            .unwrap()
            .as_deref(),
        Some("dark")
    );
    assert_eq!(
        second.settings().get::<u8>("player.volume").unwrap(),
        Some(75)
    );
    assert_eq!(second.block_on(async { 1 }).unwrap(), 1);
    second.shutdown().unwrap();
}

#[test]
fn settings_survive_a_process_that_never_shut_down() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = Runtime::start(config(tmp.path())).unwrap();
    rt.settings().set("ui.theme", "light").unwrap();
    // Simulates a kill: the instance is leaked, nothing is closed or
    // checkpointed. Committed WAL frames are read back by the next open.
    std::mem::forget(rt);

    let rt = Runtime::start(config(tmp.path())).unwrap();
    assert_eq!(
        rt.settings().get::<String>("ui.theme").unwrap().as_deref(),
        Some("light")
    );
    rt.shutdown().unwrap();
}

#[test]
fn host_provides_dirs_when_config_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    let host = Arc::new(RecordingHost {
        dirs: Some(dirs_in(tmp.path())),
        ..RecordingHost::default()
    });
    let config = RuntimeConfig {
        platform: Platform::Android,
        ..RuntimeConfig::default()
    }
    .with_host(host);
    let rt = Runtime::start(config).unwrap();
    assert_eq!(rt.dirs(), &dirs_in(tmp.path()));
    rt.shutdown().unwrap();
}

#[test]
fn android_without_dirs_fails_cleanly() {
    let config = RuntimeConfig {
        platform: Platform::Android,
        ..RuntimeConfig::default()
    };
    assert!(matches!(
        Runtime::start(config),
        Err(RuntimeError::DirsRequired {
            platform: Platform::Android
        })
    ));
}

#[test]
fn relative_dirs_fail_cleanly() {
    let config = RuntimeConfig::default().with_dirs(PlatformDirs::new("data", "config", "cache"));
    assert!(matches!(
        Runtime::start(config),
        Err(RuntimeError::RelativeDir { .. })
    ));
}
