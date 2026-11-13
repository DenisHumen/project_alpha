// SPDX-License-Identifier: MPL-2.0
//! Lifecycle and settings through the exported FFI functions.
//!
//! One test function on purpose: the core is a process-wide singleton.

use std::sync::{Arc, Mutex};

use pa_ffi::{
    CoreCallbacks, CoreConfig, CoreDirs, CoreError, CoreLifecycle, core_dirs,
    core_enter_background, core_enter_foreground, core_init, core_lifecycle, core_shutdown,
    settings_export, settings_get, settings_import, settings_keys, settings_remove, settings_set,
};

#[derive(Default)]
struct Recorder(Mutex<Vec<CoreLifecycle>>);

impl CoreCallbacks for Recorder {
    fn on_lifecycle_changed(&self, state: CoreLifecycle) {
        self.0.lock().unwrap().push(state);
    }
}

#[test]
fn lifecycle_and_settings_over_ffi() {
    let tmp = tempfile::tempdir().unwrap();
    let path = |p: &str| tmp.path().join(p).to_string_lossy().into_owned();
    let dirs = CoreDirs {
        data: path("files"),
        config: path("files"),
        cache: path("cache"),
    };
    let config = CoreConfig {
        dirs: Some(dirs.clone()),
        worker_threads: Some(1),
        start_in_background: false,
    };

    assert_eq!(core_lifecycle(), CoreLifecycle::NotInitialized);
    assert!(matches!(settings_keys(), Err(CoreError::NotInitialized)));

    let recorder = Arc::new(Recorder::default());
    core_init(config.clone(), Some(recorder.clone())).unwrap();
    assert!(matches!(
        core_init(config.clone(), None),
        Err(CoreError::AlreadyInitialized)
    ));
    assert_eq!(core_dirs().unwrap(), dirs);

    settings_set("ui.theme".into(), r#""dark""#.into()).unwrap();
    settings_set("player.subtitles".into(), r#"{"size":42}"#.into()).unwrap();
    assert_eq!(
        settings_get("ui.theme".into()).unwrap().as_deref(),
        Some(r#""dark""#)
    );
    assert!(matches!(
        settings_set("Bad Key".into(), "1".into()),
        Err(CoreError::InvalidSettingsKey { .. })
    ));
    assert!(matches!(
        settings_set("ui.theme".into(), "not json".into()),
        Err(CoreError::InvalidSettingsData { .. })
    ));
    let backup = settings_export().unwrap();

    core_enter_background().unwrap();
    assert_eq!(core_lifecycle(), CoreLifecycle::Background);
    core_enter_foreground().unwrap();
    core_shutdown().unwrap();
    core_shutdown().unwrap();
    assert_eq!(core_lifecycle(), CoreLifecycle::NotInitialized);
    assert_eq!(
        *recorder.0.lock().unwrap(),
        [
            CoreLifecycle::Foreground,
            CoreLifecycle::Background,
            CoreLifecycle::Foreground,
            CoreLifecycle::ShutDown,
        ]
    );

    // Restart: settings are still there; import restores a backup.
    core_init(config, None).unwrap();
    assert_eq!(settings_keys().unwrap(), ["player.subtitles", "ui.theme"]);
    assert!(settings_remove("ui.theme".into()).unwrap());
    settings_set("ui.extra".into(), "1".into()).unwrap();
    assert_eq!(settings_import(backup, true).unwrap(), 2);
    assert_eq!(settings_keys().unwrap(), ["player.subtitles", "ui.theme"]);
    assert!(matches!(
        settings_import("{}".into(), false),
        Err(CoreError::InvalidSettingsData { .. })
    ));
    core_shutdown().unwrap();
}

#[test]
fn relative_dirs_are_a_dirs_error() {
    // Fails before touching the global slot, so it can run in parallel.
    let config = CoreConfig {
        dirs: Some(CoreDirs {
            data: "data".into(),
            config: "config".into(),
            cache: "cache".into(),
        }),
        ..CoreConfig::default()
    };
    // AlreadyInitialized is possible if the other test holds the core.
    match core_init(config, None) {
        Err(CoreError::Dirs { .. } | CoreError::AlreadyInitialized) => {}
        other => panic!("unexpected: {other:?}"),
    }
}
