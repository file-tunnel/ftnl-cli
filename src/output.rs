//! Human tables versus `--json`.
//!
//! Every command returns a value that knows both renderings, so the `--json`
//! branch is decided once here instead of in each command body.

use serde::Serialize;

use crate::error::CliError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
}

impl Format {
    pub fn from_json_flag(json: bool) -> Self {
        if json {
            Self::Json
        } else {
            Self::Human
        }
    }
}

/// A command result that can be printed either way. `Serialize` covers `--json`;
/// `render_human` covers the default table.
pub trait Report: Serialize {
    fn render_human(&self) -> String;

    /// Exit code for a *successful parse* whose result is still a failure —
    /// e.g. every region was unreachable. Defaults to success.
    fn exit_code(&self) -> i32 {
        0
    }
}

/// Prints `report` in the requested format and returns its exit code.
pub fn emit<R: Report>(report: &R, format: Format) -> Result<i32, CliError> {
    match format {
        Format::Human => println!("{}", report.render_human()),
        Format::Json => println!("{}", serde_json::to_string_pretty(report)?),
    }
    Ok(report.exit_code())
}
