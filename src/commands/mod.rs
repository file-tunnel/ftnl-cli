//! One module per subcommand, plus the dispatch table.
//!
//! Which command ran is decided by flags-2-env from the `[commands.*]` tables
//! in `.cli-flags.toml`, never by hand-matching argv here. [`Command`] is the
//! closed set that contract may resolve to, and `command_set_matches_config`
//! fails the build if the two lists disagree.

pub mod cancel;
pub mod completion;
pub mod create;
pub mod recv;
pub mod send;
pub mod status;

use std::path::Path;

use ftnl_client::FileTunnelClient;

use crate::error::CliError;
use crate::flags::CliArgs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Create a tunnel and print its pairing URI.
    Create,
    /// Show a tunnel's status and declared files.
    Status,
    /// Declare and upload a local file.
    Send,
    /// Download a file from a tunnel.
    Recv,
    /// Cancel a tunnel.
    Cancel,
    /// Print a shell completion script.
    Completion,
}

impl Command {
    /// The canonical `[commands.*]` key, which is also what flags-2-env reports
    /// in `FLAGS2ENV_COMMAND`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Status => "status",
            Self::Send => "send",
            Self::Recv => "recv",
            Self::Cancel => "cancel",
            Self::Completion => "completion",
        }
    }

    pub fn parse(label: &str) -> Result<Self, CliError> {
        match label {
            "create" => Ok(Self::Create),
            "status" | "snapshot" => Ok(Self::Status),
            "send" => Ok(Self::Send),
            "recv" | "receive" => Ok(Self::Recv),
            "cancel" => Ok(Self::Cancel),
            "completion" => Ok(Self::Completion),
            other => Err(CliError::usage(format!("unsupported command {other:?}"))),
        }
    }

    pub const ALL: [Self; 6] = [
        Self::Create,
        Self::Status,
        Self::Send,
        Self::Recv,
        Self::Cancel,
        Self::Completion,
    ];
}

/// Runs the selected command and returns its exit code.
///
/// `completion` is handled before the runtime is started: it is pure string
/// generation and must work with no network and no credentials.
pub fn dispatch(args: &CliArgs, config_path: &Path) -> Result<i32, CliError> {
    if args.command == Command::Completion {
        return completion::run(args, config_path);
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            CliError::runtime(format!("could not start the async runtime: {error}"))
        })?;

    runtime.block_on(async {
        let client = client(args)?;
        match args.command {
            Command::Create => create::run(&client, args).await,
            Command::Status => status::run(&client, args).await,
            Command::Send => send::run(&client, args).await,
            Command::Recv => recv::run(&client, args).await,
            Command::Cancel => cancel::run(&client, args).await,
            Command::Completion => unreachable!("handled before the runtime starts"),
        }
    })
}

fn client(args: &CliArgs) -> Result<FileTunnelClient, CliError> {
    FileTunnelClient::new(&args.base_url)
        .map_err(|error| CliError::usage(format!("invalid --url: {error}")))
}

/// Maps a transport failure to a CLI error without echoing a response body.
///
/// `ftnl_client::Error` is `Display`, and its `Api` variant prints only the
/// status and the problem `code` — never the detail — so this stays a thin
/// wrapper that adds what the CLI was doing at the time.
pub fn request_failed(what: &str, error: &ftnl_client::Error) -> CliError {
    CliError::runtime(format!("{what} failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_set_matches_config() {
        let config = include_str!("../../.cli-flags.toml");
        for command in Command::ALL {
            let table = format!("[commands.{}]", command.as_str());
            assert!(
                config.contains(&table),
                "{table} is missing from .cli-flags.toml"
            );
        }

        let declared = config
            .lines()
            .filter_map(|line| line.trim().strip_prefix("[commands."))
            .filter_map(|line| line.strip_suffix(']'))
            // Ignore nested tables such as `[commands.x.flags.y]`.
            .filter(|name| !name.contains('.'))
            .count();
        assert_eq!(
            declared,
            Command::ALL.len(),
            ".cli-flags.toml declares {declared} commands but Command::ALL has {}",
            Command::ALL.len()
        );
    }

    #[test]
    fn aliases_collapse_to_one_canonical_command() {
        assert_eq!(Command::parse("snapshot").unwrap(), Command::Status);
        assert_eq!(Command::parse("receive").unwrap(), Command::Recv);
    }

    #[test]
    fn unknown_commands_are_usage_errors() {
        assert_eq!(Command::parse("upload").unwrap_err().exit_code(), 2);
    }
}
