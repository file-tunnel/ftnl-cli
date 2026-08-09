//! `ftnl recv` — download a file from a tunnel.
//!
//! With no `--file-id`, the tunnel must hold exactly one downloadable file.
//! Picking "the first one" from a list whose order the server chooses would make
//! the result depend on something the caller cannot see.

use ftnl_client::{FileDescriptor, FileTunnelClient};
use serde::Serialize;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

use crate::commands::request_failed;
use crate::error::CliError;
use crate::flags::CliArgs;
use crate::output::{emit, Format, Report};
use crate::secrets;
use crate::status::is_downloadable;

#[derive(Debug, Serialize)]
pub struct Received {
    pub tunnel_id: String,
    pub file_id: String,
    pub name: String,
    pub written_to: String,
    pub size_bytes: u64,
}

impl Report for Received {
    fn render_human(&self) -> String {
        format!(
            "downloaded {}\nfile id    {}\nwrote      {}\nsize       {} bytes",
            self.name, self.file_id, self.written_to, self.size_bytes
        )
    }
}

pub async fn run(client: &FileTunnelClient, args: &CliArgs) -> Result<i32, CliError> {
    let tunnel_id = args.require_tunnel()?;
    let capability = secrets::capability()?;

    let snapshot = client
        .snapshot(tunnel_id, &capability)
        .await
        .map_err(|error| request_failed("reading the tunnel snapshot", &error))?;

    let file = choose_file(&snapshot.files, args.file_id)?;
    let destination = args
        .out
        .clone()
        .map(Ok)
        .unwrap_or_else(|| safe_default_destination(&file.name))?;

    let bytes = client
        .download(tunnel_id, file.file_id, &capability)
        .await
        .map_err(|error| request_failed("downloading the file", &error))?;

    let received_size = u64::try_from(bytes.len())
        .map_err(|_| CliError::runtime("downloaded file length does not fit u64"))?;
    if received_size != file.size_bytes {
        return Err(CliError::runtime(format!(
            "downloaded byte count did not match declared size (expected {}, received {})",
            file.size_bytes, received_size
        )));
    }
    write_atomically(&destination, &bytes, args.force)?;

    emit(
        &Received {
            tunnel_id: tunnel_id.to_string(),
            file_id: file.file_id.to_string(),
            name: file.name.clone(),
            written_to: destination.display().to_string(),
            size_bytes: received_size,
        },
        Format::from_json_flag(args.json),
    )
}

fn safe_default_destination(name: &str) -> Result<PathBuf, CliError> {
    let path = Path::new(name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) if !component.is_empty() => {
            Ok(PathBuf::from(component))
        }
        _ => Err(CliError::runtime(
            "server returned an unsafe file name; provide an explicit --out path",
        )),
    }
}

fn write_atomically(destination: &Path, bytes: &[u8], force: bool) -> Result<(), CliError> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty());
    let parent = parent.unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(CliError::runtime(format!(
            "output directory {} does not exist",
            parent.display()
        )));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        CliError::runtime(format!(
            "cannot create a temporary output beside {}: {error}",
            destination.display()
        ))
    })?;
    temporary
        .write_all(bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| {
            CliError::runtime(format!(
                "cannot write temporary output for {}: {error}",
                destination.display()
            ))
        })?;
    let result = if force {
        temporary.persist(destination)
    } else {
        temporary.persist_noclobber(destination)
    };
    result.map_err(|error| {
        let action = if force { "replace" } else { "create" };
        CliError::runtime(format!(
            "cannot {action} {} atomically: {}{}",
            destination.display(),
            error.error,
            if force {
                ""
            } else {
                " (use --force to replace an existing file)"
            }
        ))
    })?;
    Ok(())
}

/// Resolves `--file-id`, or the single downloadable file when it is omitted.
fn choose_file(
    files: &[FileDescriptor],
    requested: Option<Uuid>,
) -> Result<&FileDescriptor, CliError> {
    if let Some(file_id) = requested {
        return files
            .iter()
            .find(|file| file.file_id == file_id)
            .ok_or_else(|| CliError::usage(format!("no file {file_id} on this tunnel")));
    }

    let mut downloadable = files.iter().filter(|file| is_downloadable(&file.status));
    match (downloadable.next(), downloadable.next()) {
        (Some(only), None) => Ok(only),
        (None, _) => Err(CliError::usage(
            "no downloadable file on this tunnel yet; check `ftnl status`",
        )),
        (Some(_), Some(_)) => Err(CliError::usage(format!(
            "--file-id is required: {} downloadable files on this tunnel",
            files
                .iter()
                .filter(|file| is_downloadable(&file.status))
                .count()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(name: &str, status: &str) -> FileDescriptor {
        FileDescriptor {
            file_id: Uuid::new_v4(),
            name: name.to_owned(),
            media_type: "application/octet-stream".to_owned(),
            size_bytes: 1,
            bytes_transferred: 1,
            status: status.to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn a_single_available_file_is_chosen_without_a_flag() {
        let files = vec![
            descriptor("a.png", "available"),
            descriptor("b.png", "declared"),
        ];
        // Only one is downloadable, so the declared one must not make it
        // ambiguous.
        assert_eq!(choose_file(&files, None).unwrap().name, "a.png");
    }

    #[test]
    fn several_downloadable_files_require_an_explicit_id() {
        let files = vec![
            descriptor("a.png", "available"),
            descriptor("b.png", "downloaded"),
        ];
        let error = choose_file(&files, None).unwrap_err();
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("--file-id is required"));
    }

    #[test]
    fn an_explicit_id_is_matched_exactly() {
        let files = vec![
            descriptor("a.png", "available"),
            descriptor("b.png", "available"),
        ];
        let wanted = files[1].file_id;
        assert_eq!(choose_file(&files, Some(wanted)).unwrap().name, "b.png");
        assert!(choose_file(&files, Some(Uuid::nil())).is_err());
    }

    #[test]
    fn nothing_downloadable_is_a_usage_error_not_a_panic() {
        let files = vec![descriptor("a.png", "uploading")];
        assert!(choose_file(&files, None).is_err());
        assert!(choose_file(&[], None).is_err());
    }

    #[test]
    fn default_destination_rejects_paths_from_the_server() {
        assert_eq!(
            safe_default_destination("photo.jpg").unwrap(),
            PathBuf::from("photo.jpg")
        );
        assert!(safe_default_destination("../photo.jpg").is_err());
        assert!(safe_default_destination("folder/photo.jpg").is_err());
        assert!(safe_default_destination("/tmp/photo.jpg").is_err());
    }

    #[test]
    fn atomic_write_refuses_overwrite_without_force() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("photo.jpg");
        write_atomically(&destination, b"first", false).unwrap();
        assert!(write_atomically(&destination, b"second", false).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"first");
        write_atomically(&destination, b"second", true).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"second");
    }
}
