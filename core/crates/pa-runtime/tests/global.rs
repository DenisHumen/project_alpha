// SPDX-License-Identifier: MPL-2.0
//! The process-wide runtime used by the FFI facade.
//!
//! One test function on purpose: the global slot is shared by every test in
//! this binary, and tests run in parallel.

use pa_runtime::{LifecycleState, PlatformDirs, RuntimeConfig, RuntimeError, global};

#[test]
fn init_shutdown_init_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let config = || {
        RuntimeConfig::default()
            .with_dirs(PlatformDirs::new(
                tmp.path().join("data"),
                tmp.path().join("config"),
                tmp.path().join("cache"),
            ))
            .with_worker_threads(1)
    };

    assert!(!global::is_initialized());
    assert!(matches!(
        global::current(),
        Err(RuntimeError::NotInitialized)
    ));
    global::shutdown().unwrap(); // nothing to do

    let rt = global::init(config()).unwrap();
    assert!(matches!(
        global::init(config()),
        Err(RuntimeError::AlreadyInitialized)
    ));
    rt.settings().set("ui.theme", "dark").unwrap();
    global::current().unwrap().enter_background().unwrap();
    assert_eq!(rt.state(), LifecycleState::Background);

    global::shutdown().unwrap();
    assert!(rt.is_shut_down());
    assert!(matches!(
        global::current(),
        Err(RuntimeError::NotInitialized)
    ));

    // A shut-down instance kept by someone does not block a new init.
    for round in 0..3 {
        let rt = global::init(config()).unwrap();
        assert_eq!(rt.state(), LifecycleState::Foreground, "round {round}");
        assert_eq!(
            rt.settings().get::<String>("ui.theme").unwrap().as_deref(),
            Some("dark")
        );
        global::shutdown().unwrap();
    }

    // Shutting down an instance directly also frees the slot for init.
    let rt = global::init(config()).unwrap();
    rt.shutdown().unwrap();
    assert!(!global::is_initialized());
    global::init(config()).unwrap();
    global::shutdown().unwrap();
}
