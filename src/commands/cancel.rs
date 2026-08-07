//! `ftnl cancel` — close a tunnel and invalidate its capabilities.

use ftnl_client::FileTunnelClient;
use serde::Serialize;

use crate::commands::request_failed;
use crate::error::CliError;
use crate::flags::CliArgs;
use crate::output::{emit, Format, Report};
use crate::secrets;

#[derive(Debug, Serialize)]
pub struct Cancelled {
    pub tunnel_id: String,
    pub cancelled: bool,
}

impl Report for Cancelled {
    fn render_human(&self) -> String {
        format!("cancelled tunnel {}", self.tunnel_id)
    }
}

pub async fn run(client: &FileTunnelClient, args: &CliArgs) -> Result<i32, CliError> {
    let tunnel_id = args.require_tunnel()?;
    let capability = secrets::capability()?;

    client
        .cancel(tunnel_id, &capability)
        .await
        .map_err(|error| request_failed("cancelling the tunnel", &error))?;

    emit(
        &Cancelled {
            tunnel_id: tunnel_id.to_string(),
            cancelled: true,
        },
        Format::from_json_flag(args.json),
    )
}
