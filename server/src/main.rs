// SPDX-License-Identifier: MPL-2.0
//! `alpha-media`: the Kinolune media server binary.

use std::path::PathBuf;
use std::process::ExitCode;

use pa_runtime::{Runtime, RuntimeConfig};
use pa_server::{Config, Server, config, logging, shutdown_signal};

const USAGE: &str = "\
Kinolune media server

Usage: alpha-media [--config <file.toml>]

Options:
  -c, --config <file>  TOML configuration (default: none, built-in defaults)
  -h, --help           Print this help
  -V, --version        Print the version

Environment (overrides the file):
  ALPHA_MEDIA_CONFIG      configuration file, same as --config
  ALPHA_MEDIA_LISTEN      listen address, default 127.0.0.1:8096
  ALPHA_MEDIA_DATA_DIR    state directory (library database, settings, cache)
  ALPHA_MEDIA_LOG         log filter, default info
  ALPHA_MEDIA_LOG_FORMAT  text or json
";

enum Command {
    Run(Option<PathBuf>),
    Exit(&'static str),
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut config = std::env::var_os("ALPHA_MEDIA_CONFIG").map(PathBuf::from);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Exit(USAGE)),
            "-V" | "--version" => return Ok(Command::Exit(pa_server::VERSION)),
            "-c" | "--config" => {
                let path = args.next().ok_or("--config needs a file")?;
                config = Some(PathBuf::from(path));
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Command::Run(config))
}

fn main() -> ExitCode {
    let config_path = match parse_args(std::env::args().skip(1)) {
        Ok(Command::Run(path)) => path,
        Ok(Command::Exit(text)) => {
            println!("{}", text.trim_end());
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("alpha-media: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let config = match Config::load(config_path.as_deref()) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("alpha-media: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = logging::init(&config.log) {
        eprintln!("alpha-media: {e}");
        return ExitCode::from(2);
    }
    match run(&config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "server failed");
            ExitCode::FAILURE
        }
    }
}

fn run(config: &Config) -> Result<(), pa_server::Error> {
    let dirs = config.dirs()?;
    let runtime = Runtime::start(RuntimeConfig {
        app_dir_name: config::APP_DIR_NAME.to_owned(),
        ..RuntimeConfig::default().with_dirs(dirs)
    })?;
    tracing::info!(data = %runtime.dirs().data.display(), "core runtime started");
    let result = runtime.block_on(async {
        let server = Server::bind(config, &runtime.dirs().data).await?;
        server.run(shutdown_signal()).await
    });
    // Stops background tasks and checkpoints the settings database.
    let stopped = runtime.shutdown();
    result??;
    stopped?;
    tracing::info!("stopped");
    Ok(())
}
