//! `ftnl status` — a tunnel's state and the files declared on it.
//!
//! Exits non-zero when the tunnel has reached a terminal state, so a polling
//! loop (`until ftnl status; do sleep 1; done`) terminates on cancel/expiry
//! instead of spinning forever.

use ftnl_client::FileTunnelClient;
use serde::Serialize;

use crate::commands::request_failed;
use crate::error::CliError;
use crate::flags::CliArgs;
use crate::output::{emit, Format, Report};
use crate::secrets;
use crate::status::is_terminal;

#[derive(Debug, Serialize)]
pub struct FileRow {
    pub file_id: String,
    pub name: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub bytes_transferred: u64,
    pub status: String,
    /// False for a status this build does not recognise, which is reported
    /// verbatim rather than rejected.
    pub known_status: bool,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub tunnel_id: String,
    pub status: String,
    pub known_status: bool,
    pub terminal: bool,
    pub expires_at: String,
    pub files: Vec<FileRow>,
}

impl Report for Snapshot {
    fn render_human(&self) -> String {
        let mut out = format!(
            "tunnel     {}\nstatus     {}{}\nexpires at {}\nfiles      {}",
            self.tunnel_id,
            self.status,
            if self.known_status {
                ""
            } else {
                "  (unrecognised by this build)"
            },
            self.expires_at,
            self.files.len(),
        );
        if !self.files.is_empty() {
            out.push_str(&format!(
                "\n\n{:<38} {:>12} {:>12}  {:<12} name",
                "file id", "size", "transferred", "status"
            ));
            for file in &self.files {
                out.push_str(&format!(
                    "\n{:<38} {:>12} {:>12}  {:<12} {}",
                    file.file_id, file.size_bytes, file.bytes_transferred, file.status, file.name
                ));
            }
        }
        out
    }

    fn exit_code(&self) -> i32 {
        i32::from(self.terminal)
    }
}

pub async fn run(client: &FileTunnelClient, args: &CliArgs) -> Result<i32, CliError> {
    let tunnel_id = args.require_tunnel()?;
    let capability = secrets::capability()?;

    let snapshot = client
        .snapshot(tunnel_id, &capability)
        .await
        .map_err(|error| request_failed("reading the tunnel snapshot", &error))?;

    let report = Snapshot {
        tunnel_id: snapshot.tunnel_id.to_string(),
        known_status: crate::status::tunnel_status(&snapshot.status)
            .as_known()
            .is_some(),
        terminal: is_terminal(&snapshot.status),
        status: snapshot.status,
        expires_at: snapshot.expires_at,
        files: snapshot
            .files
            .into_iter()
            .map(|file| FileRow {
                file_id: file.file_id.to_string(),
                name: file.name,
                media_type: file.media_type,
                size_bytes: file.size_bytes,
                bytes_transferred: file.bytes_transferred,
                known_status: crate::status::file_status(&file.status)
                    .as_known()
                    .is_some(),
                status: file.status,
            })
            .collect(),
    };
    emit(&report, Format::from_json_flag(args.json))
}
