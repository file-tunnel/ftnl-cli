//! Durable, metadata-only upload state.
//!
//! This module owns platform path selection and the stable writer identity.
//! The actual protocol-v1 queue and SQLite invariants stay in `ftnl-sync`.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use ftnl_sync::DurableUploadQueue;
use uuid::Uuid;

use crate::error::CliError;

const WRITER_FILE: &str = "writer-id";
const DATABASE_FILE: &str = "uploads.sqlite3";

pub fn open(override_path: Option<&Path>) -> Result<DurableUploadQueue, CliError> {
    let root = match override_path {
        Some(path) => path.to_path_buf(),
        None => default_state_dir()?,
    };
    ensure_private_directory(&root)?;
    let writer_id = load_or_create_writer_id(&root.join(WRITER_FILE))?;
    let database = root.join(DATABASE_FILE);
    let queue = DurableUploadQueue::open(&database, writer_id).map_err(|error| {
        CliError::runtime(format!(
            "cannot open durable upload metadata at {}: {error}",
            database.display()
        ))
    })?;
    make_private_file(&database)?;
    Ok(queue)
}

fn default_state_dir() -> Result<PathBuf, CliError> {
    #[cfg(target_os = "windows")]
    if let Some(base) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(base).join("FileTunnel").join("ftnl"));
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("FileTunnel")
            .join("ftnl"));
    }

    if let Some(base) = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(base).join("file-tunnel").join("ftnl"));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("file-tunnel")
            .join("ftnl"));
    }
    Err(CliError::runtime(
        "cannot determine the platform state directory; set FTNL_STATE_DIR or --state-dir",
    ))
}

fn ensure_private_directory(path: &Path) -> Result<(), CliError> {
    fs::create_dir_all(path).map_err(|error| {
        CliError::runtime(format!(
            "cannot create durable upload state directory {}: {error}",
            path.display()
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            CliError::runtime(format!(
                "cannot restrict durable upload state directory {}: {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

fn load_or_create_writer_id(path: &Path) -> Result<String, CliError> {
    let candidate = format!("ftnlcli.{}", Uuid::new_v4().simple());
    match private_create_new(path) {
        Ok(mut file) => {
            file.write_all(candidate.as_bytes())
                .and_then(|_| file.write_all(b"\n"))
                .and_then(|_| file.sync_all())
                .map_err(|error| {
                    CliError::runtime(format!(
                        "cannot persist upload writer identity {}: {error}",
                        path.display()
                    ))
                })?;
            Ok(candidate)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let mut value = String::new();
            fs::File::open(path)
                .and_then(|mut file| file.read_to_string(&mut value))
                .map_err(|read_error| {
                    CliError::runtime(format!(
                        "cannot read upload writer identity {}: {read_error}",
                        path.display()
                    ))
                })?;
            let value = value.trim();
            if !value.starts_with("ftnlcli.")
                || value.len() > 128
                || value.contains('-')
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
            {
                return Err(CliError::runtime(format!(
                    "upload writer identity {} is invalid",
                    path.display()
                )));
            }
            Ok(value.to_owned())
        }
        Err(error) => Err(CliError::runtime(format!(
            "cannot create upload writer identity {}: {error}",
            path.display()
        ))),
    }
}

fn private_create_new(path: &Path) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn make_private_file(path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            CliError::runtime(format!(
                "cannot restrict durable upload database {}: {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_identity_is_stable_and_queue_reopens() {
        let directory = tempfile::tempdir().unwrap();
        let first = open(Some(directory.path())).unwrap();
        assert_eq!(first.pending_count().unwrap(), 0);
        drop(first);
        let second = open(Some(directory.path())).unwrap();
        assert_eq!(second.pending_count().unwrap(), 0);
    }

    #[test]
    fn invalid_existing_writer_identity_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join(WRITER_FILE),
            "not-valid-with-dashes\n",
        )
        .unwrap();
        assert!(open(Some(directory.path())).is_err());
    }
}
