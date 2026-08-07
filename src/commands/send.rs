//! `ftnl send` — claim a tunnel, declare a file, and upload it.
//!
//! The phone side of the flow. Claiming exchanges the one-time pairing secret
//! for a phone capability, so `FTNL_PAIRING_URI` is enough on its own and the
//! caller never has to handle the capability by hand.

use bytes::Bytes;
use ftnl_client::{DeclareFileRequest, FileTunnelClient};
use serde::Serialize;

use crate::commands::request_failed;
use crate::error::CliError;
use crate::flags::CliArgs;
use crate::output::{emit, Format, Report};
use crate::secrets;

#[derive(Debug, Serialize)]
pub struct Sent {
    pub tunnel_id: String,
    pub file_id: String,
    pub name: String,
    pub media_type: String,
    pub size_bytes: u64,
}

impl Report for Sent {
    fn render_human(&self) -> String {
        format!(
            "uploaded   {}\nfile id    {}\nmedia type {}\nsize       {} bytes\ntunnel     {}",
            self.name, self.file_id, self.media_type, self.size_bytes, self.tunnel_id
        )
    }
}

pub async fn run(client: &FileTunnelClient, args: &CliArgs) -> Result<i32, CliError> {
    let tunnel_id = args.require_tunnel()?;
    let path = args.require_file()?;

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::usage("--file must end in a valid UTF-8 file name"))?
        .to_owned();

    let bytes = std::fs::read(path)
        .map_err(|error| CliError::runtime(format!("cannot read {}: {error}", path.display())))?;
    let size_bytes = bytes.len() as u64;

    // The capability comes either from a completed claim or from the
    // environment, so `send` works both right after pairing and on a later run.
    let capability = match secrets::pairing_secret() {
        Ok(secret) => client
            .claim_tunnel(tunnel_id, &secret)
            .await
            .map_err(|error| request_failed("claiming the tunnel", &error))?,
        Err(pairing_error) => secrets::capability().map_err(|capability_error| {
            CliError::usage(format!("{pairing_error}; or {capability_error}"))
        })?,
    };

    let declared = client
        .declare_file(
            tunnel_id,
            &capability,
            &DeclareFileRequest {
                name: name.clone(),
                media_type: args.media_type.clone(),
                size_bytes,
                // Neither is required by the contract, and both are metadata the
                // CLI has no reliable source for: a mtime is not portable across
                // the devices a tunnel spans, and a digest the sender computes
                // proves nothing the transport does not already check.
                last_modified_ms: None,
                sha256: None,
            },
        )
        .await
        .map_err(|error| request_failed("declaring the file", &error))?;

    client
        .upload(tunnel_id, declared.file_id, &capability, Bytes::from(bytes))
        .await
        .map_err(|error| request_failed("uploading the file", &error))?;

    emit(
        &Sent {
            tunnel_id: tunnel_id.to_string(),
            file_id: declared.file_id.to_string(),
            name: declared.name,
            media_type: declared.media_type,
            size_bytes: declared.size_bytes,
        },
        Format::from_json_flag(args.json),
    )
}
