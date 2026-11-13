// SPDX-License-Identifier: MPL-2.0
//! The runtime instance: tokio, lifecycle state, directories, settings.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::error::RuntimeError;
use crate::host::{LifecycleState, PlatformHost};
use crate::paths::{DEFAULT_APP_DIR_NAME, Platform, PlatformDirs};
use crate::settings::{SETTINGS_FILE, Settings};

/// Upper bound of default worker threads: the core shares the device with
/// the UI and the player, and runs on small NAS boxes.
const MAX_DEFAULT_WORKERS: usize = 4;

/// How long [`Runtime::shutdown`] waits for running tasks to stop.
pub(crate) const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Name prefix of the core's worker threads (visible in debuggers and crash
/// reports).
const THREAD_NAME: &str = "pa-core";

/// Everything [`Runtime::start`] needs. [`RuntimeConfig::default`] fits a
/// desktop app on the current platform.
#[derive(Clone)]
pub struct RuntimeConfig {
    /// Platform; decides the default directories.
    pub platform: Platform,
    /// Name of the app directory inside the platform locations.
    pub app_dir_name: String,
    /// Explicit directories. When `None`, [`PlatformHost::dirs`] is asked,
    /// then [`PlatformDirs::platform_default`] is used.
    pub dirs: Option<PlatformDirs>,
    /// Number of tokio worker threads; default: CPU count, at most 4.
    pub worker_threads: Option<usize>,
    /// Start in [`LifecycleState::Background`] (Android service, background
    /// fetch) instead of `Foreground`.
    pub start_in_background: bool,
    /// Callbacks to the platform.
    pub host: Option<Arc<dyn PlatformHost>>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            platform: Platform::current(),
            app_dir_name: DEFAULT_APP_DIR_NAME.to_owned(),
            dirs: None,
            worker_threads: None,
            start_in_background: false,
            host: None,
        }
    }
}

impl fmt::Debug for RuntimeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeConfig")
            .field("platform", &self.platform)
            .field("app_dir_name", &self.app_dir_name)
            .field("dirs", &self.dirs)
            .field("worker_threads", &self.worker_threads)
            .field("start_in_background", &self.start_in_background)
            .field("host", &self.host.is_some())
            .finish()
    }
}

impl RuntimeConfig {
    /// Uses `dirs` instead of the platform defaults.
    #[must_use]
    pub fn with_dirs(mut self, dirs: PlatformDirs) -> Self {
        self.dirs = Some(dirs);
        self
    }

    /// Sets the platform callbacks.
    #[must_use]
    pub fn with_host(mut self, host: Arc<dyn PlatformHost>) -> Self {
        self.host = Some(host);
        self
    }

    /// Sets the number of tokio worker threads (at least 1).
    #[must_use]
    pub fn with_worker_threads(mut self, threads: usize) -> Self {
        self.worker_threads = Some(threads.max(1));
        self
    }

    /// Starts in [`LifecycleState::Background`].
    #[must_use]
    pub fn in_background(mut self) -> Self {
        self.start_in_background = true;
        self
    }
}

/// A running core. Cheap to clone: all clones are the same instance.
///
/// The instance stays usable until [`shutdown`](Self::shutdown); after that
/// every call returns [`RuntimeError::ShutDown`] and a new runtime can be
/// started with [`Runtime::start`].
#[derive(Clone)]
pub struct Runtime {
    inner: Arc<Inner>,
}

struct Inner {
    platform: Platform,
    dirs: PlatformDirs,
    settings: Settings,
    host: Option<Arc<dyn PlatformHost>>,
    handle: Handle,
    tokio: Mutex<Option<tokio::runtime::Runtime>>,
    state: watch::Sender<LifecycleState>,
    /// Serializes state transitions so the host sees them in order.
    transition: Mutex<()>,
}

impl fmt::Debug for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Runtime")
            .field("platform", &self.inner.platform)
            .field("state", &self.state())
            .field("dirs", &self.inner.dirs)
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // The guarded data stays consistent even if a holder panicked.
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Runtime {
    /// Resolves and creates the data directories, opens (and migrates) the
    /// settings database and starts the tokio runtime.
    ///
    /// The host gets [`PlatformHost::on_lifecycle_changed`] with the initial
    /// state.
    pub fn start(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        let RuntimeConfig {
            platform,
            app_dir_name,
            dirs,
            worker_threads,
            start_in_background,
            host,
        } = config;

        let dirs = match dirs.or_else(|| host.as_ref().and_then(|h| h.dirs())) {
            Some(dirs) => dirs,
            None => PlatformDirs::platform_default(platform, &app_dir_name)?,
        };
        dirs.create_all()?;
        let settings = Settings::open(&dirs.config.join(SETTINGS_FILE))?;

        let workers = worker_threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map_or(1, usize::from)
                .min(MAX_DEFAULT_WORKERS)
        });
        let tokio = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers.max(1))
            .thread_name(THREAD_NAME)
            .enable_all()
            .build()
            .map_err(RuntimeError::AsyncRuntime)?;

        let initial = if start_in_background {
            LifecycleState::Background
        } else {
            LifecycleState::Foreground
        };
        let runtime = Self {
            inner: Arc::new(Inner {
                platform,
                dirs,
                settings,
                host,
                handle: tokio.handle().clone(),
                tokio: Mutex::new(Some(tokio)),
                state: watch::Sender::new(initial),
                transition: Mutex::new(()),
            }),
        };
        if let Some(host) = &runtime.inner.host {
            host.on_lifecycle_changed(initial);
        }
        Ok(runtime)
    }

    /// Platform the runtime was started for.
    pub fn platform(&self) -> Platform {
        self.inner.platform
    }

    /// Current lifecycle state.
    pub fn state(&self) -> LifecycleState {
        *self.inner.state.borrow()
    }

    /// Whether [`shutdown`](Self::shutdown) has run.
    pub fn is_shut_down(&self) -> bool {
        self.state() == LifecycleState::ShutDown
    }

    /// Receiver of lifecycle changes for subsystems: pause work on
    /// `Background`, stop on `ShutDown`.
    pub fn subscribe(&self) -> watch::Receiver<LifecycleState> {
        self.inner.state.subscribe()
    }

    /// The directories in use (already created).
    pub fn dirs(&self) -> &PlatformDirs {
        &self.inner.dirs
    }

    /// The settings store. Closed after shutdown.
    pub fn settings(&self) -> &Settings {
        &self.inner.settings
    }

    /// Handle of the tokio runtime, to spawn tasks from other crates.
    pub fn handle(&self) -> Result<Handle, RuntimeError> {
        self.ensure_running()?;
        Ok(self.inner.handle.clone())
    }

    /// Spawns a task on the core runtime.
    pub fn spawn<F>(&self, future: F) -> Result<JoinHandle<F::Output>, RuntimeError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        Ok(self.handle()?.spawn(future))
    }

    /// Runs `future` to completion on the core runtime, blocking the calling
    /// thread. This is the bridge for synchronous FFI calls.
    ///
    /// # Panics
    ///
    /// Panics when called from inside an async context (a task of any tokio
    /// runtime); use [`spawn`](Self::spawn) there.
    pub fn block_on<F: Future>(&self, future: F) -> Result<F::Output, RuntimeError> {
        Ok(self.handle()?.block_on(future))
    }

    /// The app became visible. Returns whether the state changed.
    pub fn enter_foreground(&self) -> Result<bool, RuntimeError> {
        self.transition(LifecycleState::Foreground)
    }

    /// The app went to the background. Flushes the settings WAL so a kill by
    /// the OS loses nothing. Returns whether the state changed.
    pub fn enter_background(&self) -> Result<bool, RuntimeError> {
        self.transition(LifecycleState::Background)
    }

    fn transition(&self, to: LifecycleState) -> Result<bool, RuntimeError> {
        let _serial = lock(&self.inner.transition);
        let from = self.state();
        if from == LifecycleState::ShutDown {
            return Err(RuntimeError::ShutDown);
        }
        if from == to {
            return Ok(false);
        }
        if to == LifecycleState::Background {
            // Best effort: a busy reader only makes the checkpoint partial.
            let _ = self.inner.settings.checkpoint();
        }
        self.inner.state.send_replace(to);
        if let Some(host) = &self.inner.host {
            host.on_lifecycle_changed(to);
        }
        Ok(true)
    }

    /// Stops the core: publishes [`LifecycleState::ShutDown`], stops the
    /// tokio runtime (waiting up to 5 s for tasks, or not at all when called
    /// from inside a task) and closes the settings database.
    ///
    /// Calling it again does nothing. Safe to call from any thread, including
    /// a task of this runtime.
    pub fn shutdown(&self) -> Result<(), RuntimeError> {
        let _serial = lock(&self.inner.transition);
        if self.is_shut_down() {
            return Ok(());
        }
        self.inner.state.send_replace(LifecycleState::ShutDown);
        if let Some(host) = &self.inner.host {
            host.on_lifecycle_changed(LifecycleState::ShutDown);
        }
        let tokio = lock(&self.inner.tokio).take();
        if let Some(tokio) = tokio {
            stop_tokio(tokio, SHUTDOWN_TIMEOUT);
        }
        self.inner.settings.close()?;
        Ok(())
    }

    fn ensure_running(&self) -> Result<(), RuntimeError> {
        if self.is_shut_down() {
            Err(RuntimeError::ShutDown)
        } else {
            Ok(())
        }
    }
}

/// Stops a tokio runtime without panicking: waiting for tasks is only
/// allowed outside an async context.
fn stop_tokio(tokio: tokio::runtime::Runtime, timeout: Duration) {
    if Handle::try_current().is_ok() {
        tokio.shutdown_background();
    } else {
        tokio.shutdown_timeout(timeout);
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // The last clone was dropped without shutdown(), possibly inside a
        // task: a plain drop of the tokio runtime would panic there.
        let tokio = self
            .tokio
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(tokio) = tokio {
            stop_tokio(tokio, Duration::ZERO);
        }
    }
}
