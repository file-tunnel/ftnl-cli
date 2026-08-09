//! `ftnl send` — claim a tunnel, declare a file, and upload it.
//!
//! The phone side of the flow. Claiming exchanges the one-time pairing secret
//! for a phone capability, so `FTNL_PAIRING_URI` is enough on its own and the
//! caller never has to handle the capability by hand.

use bytes::Bytes;
use ftnl_client::{DeclareFileRequest, FileTunnelClient};
use ftnl_sync::{DurableUploadQueue, ReasonCode, UploadJob, UploadStatus};
use serde::Serialize;
use uuid::Uuid;

use crate::commands::request_failed;
use crate::error::CliError;
use crate::flags::CliArgs;
use crate::output::{emit, Format, Report};
use crate::secrets;
use crate::sync_state;

#[derive(Debug, Serialize)]
pub struct Sent {
    pub job_id: String,
    pub tunnel_id: String,
    pub file_id: String,
    pub name: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub resumed: bool,
}

impl Report for Sent {
    fn render_human(&self) -> String {
        format!(
            "uploaded   {}\njob id     {}\nfile id    {}\nmedia type {}\nsize       {} bytes\ntunnel     {}\nresumed    {}",
            self.name,
            self.job_id,
            self.file_id,
            self.media_type,
            self.size_bytes,
            self.tunnel_id,
            self.resumed
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
    let job_id = args.job_id.unwrap_or_else(Uuid::new_v4);
    let mut queue = sync_state::open(args.state_dir.as_deref())?;
    let existing = queue
        .load(job_id)
        .map_err(|error| CliError::runtime(format!("cannot read durable upload job: {error}")))?;
    let resumed = existing.is_some();
    let mut job = match existing {
        Some(job) => {
            ensure_same_upload(&job, tunnel_id, &name, &args.media_type, size_bytes)?;
            job
        }
        None => {
            let queued = queue_job(
                &mut queue,
                UploadJob {
                    id: job_id,
                    tunnel_id,
                    file_id: None,
                    name: name.clone(),
                    media_type: args.media_type.clone(),
                    size_bytes,
                    bytes_transferred: 0,
                    status: UploadStatus::Queued,
                    attempt: 0,
                    reason_code: None,
                    // DurableUploadQueue always replaces this with its HLC.
                    updated_at: String::new(),
                    synced_at: None,
                },
            )?;
            eprintln!(
                "queued upload job {}; resume with --job-id {} if interrupted",
                queued.id, queued.id
            );
            queued
        }
    };

    if matches!(job.status, UploadStatus::Available | UploadStatus::Imported) {
        let file_id = job.file_id.ok_or_else(|| {
            CliError::runtime("durable upload job is complete but has no file id")
        })?;
        return emit_sent(&job, file_id, resumed, args);
    }

    job.attempt = job
        .attempt
        .checked_add(1)
        .filter(|attempt| *attempt <= 100)
        .ok_or_else(|| CliError::runtime("durable upload job reached its 100-attempt limit"))?;
    job.reason_code = None;
    job.status = if job.file_id.is_some() {
        UploadStatus::Uploading
    } else {
        UploadStatus::Declaring
    };
    job = queue_job(&mut queue, job)?;

    // The capability comes either from a completed claim or from the
    // environment, so `send` works both right after pairing and on a later run.
    let capability = match secrets::pairing_secret() {
        Ok(secret) => match client.claim_tunnel(tunnel_id, &secret).await {
            Ok(capability) => capability,
            Err(error) => {
                persist_failure(&mut queue, &mut job, reason_for(&error))?;
                return Err(request_failed("claiming the tunnel", &error));
            }
        },
        Err(pairing_error) => match secrets::capability() {
            Ok(capability) => capability,
            Err(capability_error) => {
                persist_failure(&mut queue, &mut job, ReasonCode::PermissionRequired)?;
                return Err(CliError::usage(format!(
                    "{pairing_error}; or {capability_error}"
                )));
            }
        },
    };

    let file_id = match job.file_id {
        Some(file_id) => file_id,
        None => {
            let declared = match client
                .declare_file(
                    tunnel_id,
                    &capability,
                    &DeclareFileRequest {
                        name: name.clone(),
                        media_type: args.media_type.clone(),
                        size_bytes,
                        last_modified_ms: None,
                        sha256: None,
                    },
                )
                .await
            {
                Ok(declared) => declared,
                Err(error) => {
                    persist_failure(&mut queue, &mut job, reason_for(&error))?;
                    return Err(request_failed("declaring the file", &error));
                }
            };
            job.file_id = Some(declared.file_id);
            job.status = UploadStatus::Uploading;
            job = queue_job(&mut queue, job)?;
            declared.file_id
        }
    };

    if let Err(error) = client
        .upload(tunnel_id, file_id, &capability, Bytes::from(bytes))
        .await
    {
        persist_failure(&mut queue, &mut job, reason_for(&error))?;
        return Err(request_failed("uploading the file", &error));
    }

    job.file_id = Some(file_id);
    job.status = UploadStatus::Available;
    job.bytes_transferred = job.size_bytes;
    job.reason_code = None;
    job = queue_job(&mut queue, job)?;

    emit_sent(&job, file_id, resumed, args)
}

fn emit_sent(
    job: &UploadJob,
    file_id: Uuid,
    resumed: bool,
    args: &CliArgs,
) -> Result<i32, CliError> {
    emit(
        &Sent {
            job_id: job.id.to_string(),
            tunnel_id: job.tunnel_id.to_string(),
            file_id: file_id.to_string(),
            name: job.name.clone(),
            media_type: job.media_type.clone(),
            size_bytes: job.size_bytes,
            resumed,
        },
        Format::from_json_flag(args.json),
    )
}

fn queue_job(queue: &mut DurableUploadQueue, job: UploadJob) -> Result<UploadJob, CliError> {
    queue
        .queue(job)
        .map(|queued| queued.job)
        .map_err(|error| CliError::runtime(format!("cannot persist durable upload job: {error}")))
}

fn persist_failure(
    queue: &mut DurableUploadQueue,
    job: &mut UploadJob,
    reason: ReasonCode,
) -> Result<(), CliError> {
    job.status = UploadStatus::Failed;
    job.reason_code = Some(reason);
    *job = queue_job(queue, job.clone())?;
    Ok(())
}

fn ensure_same_upload(
    job: &UploadJob,
    tunnel_id: Uuid,
    name: &str,
    media_type: &str,
    size_bytes: u64,
) -> Result<(), CliError> {
    if job.tunnel_id != tunnel_id
        || job.name != name
        || job.media_type != media_type
        || job.size_bytes != size_bytes
    {
        return Err(CliError::usage(format!(
            "--job-id {} belongs to a different tunnel or file metadata",
            job.id
        )));
    }
    Ok(())
}

fn reason_for(error: &ftnl_client::Error) -> ReasonCode {
    match error {
        ftnl_client::Error::Transport(_) => ReasonCode::NetworkUnavailable,
        ftnl_client::Error::Api { status, .. } if matches!(status.as_u16(), 404 | 410) => {
            ReasonCode::TunnelExpired
        }
        ftnl_client::Error::Api { .. } => ReasonCode::FileRejected,
        ftnl_client::Error::InvalidBaseUrl(_)
        | ftnl_client::Error::UnsupportedScheme(_)
        | ftnl_client::Error::InsecureTransport(_)
        | ftnl_client::Error::InvalidTimeout => ReasonCode::UploadInterrupted,
    }
}
