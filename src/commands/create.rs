//! `ftnl create` — open a tunnel and print the pairing URI.
//!
//! The desktop side of the flow. The printed `pairing_uri` is what gets
//! rendered as a QR code; its fragment carries a one-time secret, which is why
//! the phone side reads it from `FTNL_PAIRING_URI` rather than a flag.

use ftnl_client::{CreateTunnelRequest, FileTunnelClient};
use serde::Serialize;

use crate::commands::request_failed;
use crate::error::CliError;
use crate::flags::CliArgs;
use crate::output::{emit, Format, Report};
use crate::secrets::{CAPABILITY_ENV, PAIRING_URI_ENV};

#[derive(Debug, Serialize)]
pub struct CreatedTunnel {
    pub tunnel_id: String,
    pub pairing_uri: String,
    pub desktop_capability: String,
    pub expires_at: String,
    pub status: String,
}

impl Report for CreatedTunnel {
    fn render_human(&self) -> String {
        format!(
            "tunnel      {}\nstatus      {}\nexpires at  {}\npairing uri {}\n\n\
             Both values below are credentials. Export them rather than passing them as flags:\n  \
             export {CAPABILITY_ENV}='{}'\n  export {PAIRING_URI_ENV}='{}'",
            self.tunnel_id,
            self.status,
            self.expires_at,
            self.pairing_uri,
            self.desktop_capability,
            self.pairing_uri,
        )
    }
}

pub async fn run(client: &FileTunnelClient, args: &CliArgs) -> Result<i32, CliError> {
    let request = CreateTunnelRequest {
        application_id: args.application_id.clone(),
        accept: args.accept.clone(),
        max_files: args.max_files,
        max_file_bytes: args.max_file_bytes,
        expires_in_seconds: args.expires_in_seconds,
    };
    let tunnel = client
        .create_tunnel(&request)
        .await
        .map_err(|error| request_failed("creating the tunnel", &error))?;

    let report = CreatedTunnel {
        tunnel_id: tunnel.tunnel_id.to_string(),
        pairing_uri: tunnel.pairing_uri,
        desktop_capability: tunnel.desktop_capability,
        expires_at: tunnel.expires_at,
        status: tunnel.status,
    };
    emit(&report, Format::from_json_flag(args.json))
}
