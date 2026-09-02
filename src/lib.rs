//! `ftnl` — the command-line client for [File Tunnel](https://github.com/file-tunnel).
//!
//! A tunnel moves files between two devices that never talk to each other
//! directly: a desktop creates a tunnel and shows a pairing URI as a QR code, a
//! phone claims it once, declares a file, and uploads; the desktop then
//! downloads. This CLI drives both sides of that flow.
//!
//! The binary in `src/main.rs` is deliberately thin: argv in, exit code out.
//! Everything else is a module with one job:
//!
//! | module | job |
//! | --- | --- |
//! | [`flags`] | argv + environment → a validated [`flags::CliArgs`], via flags-2-env |
//! | [`cli_config`] | the env-keyed struct generated from `.cli-flags.toml` |
//! | [`help`] | `--help` tables and shell completions, rendered by the C core |
//! | [`secrets`] | capabilities and pairing secrets — environment-only, never flags |
//! | [`status`] | typed tunnel/file status from `ftnl-interfaces` |
//! | [`commands`] | one module per subcommand, each returning an [`output::Report`] |
//! | [`output`] | human table vs. `--json` |
//! | [`error`] | [`error::CliError`] and the exit codes it maps to |
//! | [`telemetry`] | bounded lifecycle events for an injected next-loggers transport |
//!
//! Every request goes through `ftnl-client`; this crate contains no routes,
//! no header names, and no retry policy of its own.

// Regenerate after editing `.cli-flags.toml`:
//   flags2env generate rust .cli-flags.toml --name CliConfig > src/cli_config.rs
// CI diffs the file against fresh generator output, so it stays byte-identical.
// Command-scoped flags land there as `Option` even when they declare a default,
// because a scoped default only applies when its own command runs.
pub mod cli_config;
pub mod commands;
pub mod error;
pub mod flags;
pub mod help;
pub mod output;
pub mod secrets;
pub mod status;
pub mod sync_state;
pub mod telemetry;

pub use error::CliError;
pub use output::{Format, Report};

/// The program name used in help tables, completion scripts, and diagnostics.
pub const PROGRAM: &str = "ftnl";

/// Parses `argv`, runs the selected command, and returns the process exit code.
///
/// The stock binary uses a no-op transport. Embedders that own an
/// `ores.otel.log` or OpenTelemetry provider should call
/// [`run_with_telemetry`] and inject the adapter explicitly.
pub fn run(argv: &[String]) -> i32 {
    let telemetry = telemetry::CliTelemetry::new(telemetry::NoopNextLoggersTransport);
    run_with_telemetry(argv, &telemetry)
}

/// Runs the CLI while emitting only bounded, metadata-minimal lifecycle events.
///
/// Telemetry delivery is best-effort and cannot change the command's exit
/// status. The transport never receives argv, error strings, capabilities,
/// pairing fragments, tickets, filenames, paths, file metadata, or content.
pub fn run_with_telemetry<T>(argv: &[String], telemetry: &telemetry::CliTelemetry<T>) -> i32
where
    T: telemetry::NextLoggersTransport,
{
    let _ = telemetry.invocation_started();
    let exit_code = run_without_telemetry(argv);
    let _ = telemetry.invocation_finished(exit_code);
    let _ = telemetry.flush();
    exit_code
}

fn run_without_telemetry(argv: &[String]) -> i32 {
    let config_path = match flags::resolve_config_path() {
        Ok(path) => path,
        Err(error) => return report(&CliError::config(error), None, argv),
    };

    if help::is_help_requested(argv) {
        return match help::help_table(&config_path, PROGRAM, argv) {
            Ok(table) => {
                print!("{table}");
                0
            }
            Err(error) => report(&error, None, argv),
        };
    }

    let args = match flags::parse_cli_args(argv, &config_path) {
        Ok(args) => args,
        Err(error) => return report(&CliError::usage(error), Some(&config_path), argv),
    };

    match commands::dispatch(&args, &config_path) {
        Ok(code) => code,
        Err(error) => report(&error, Some(&config_path), argv),
    }
}

/// Prints a diagnostic on stderr, follows usage errors with the generated help
/// table, and returns the error's exit code.
fn report(error: &CliError, config_path: Option<&std::path::Path>, argv: &[String]) -> i32 {
    eprintln!("{PROGRAM}: {error}");
    if error.wants_help() {
        if let Some(config_path) = config_path {
            if let Ok(table) = help::help_table(config_path, PROGRAM, argv) {
                eprint!("\n{table}");
            }
        }
    }
    error.exit_code()
}
