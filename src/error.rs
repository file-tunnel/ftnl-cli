//! One error type for the whole binary, carrying the process exit code.
//!
//! Exit codes are part of the CLI contract — scripts branch on them — so they
//! live next to the variants that produce them rather than being scattered
//! across `std::process::exit` calls.

use std::fmt;

#[derive(Debug)]
pub enum CliError {
    /// Bad invocation: unknown flag, unknown command, out-of-range value.
    Usage(String),
    /// `.cli-flags.toml` is missing, unreadable, or fails the flags2env audit.
    Config(String),
    /// The command ran but the work failed (unreachable region, HTTP error).
    Runtime(String),
}

impl CliError {
    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime(message.into())
    }

    /// `2` for usage (the shell convention), `3` for a broken config, `1` for
    /// everything else.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Config(_) => 3,
            Self::Runtime(_) => 1,
        }
    }

    /// Usage errors print the help table after the message; runtime errors do
    /// not, because the invocation was fine.
    pub fn wants_help(&self) -> bool {
        matches!(self, Self::Usage(_))
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Config(message) | Self::Runtime(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for CliError {}

impl From<serde_json::Error> for CliError {
    fn from(error: serde_json::Error) -> Self {
        Self::Runtime(format!("could not encode JSON output: {error}"))
    }
}
