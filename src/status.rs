//! Typed tunnel and file status, decoded from `ftnl-interfaces`.
//!
//! `ftnl-client` hands back `status` as a `String` because the transport must
//! tolerate a status a newer server invented. The CLI still needs to *decide*
//! things — which file is downloadable, whether a tunnel is finished — so the
//! strings are decoded here against the shared wire enums instead of being
//! compared to string literals scattered through the command modules.
//!
//! Unknown values are preserved rather than rejected: the contract says clients
//! must ignore unknown event kinds, and a CLI that refuses to print a tunnel
//! because the server learned a new status would be worse than one that shows
//! it verbatim.

use ftnl_interfaces::{FileStatus, TunnelStatus};

/// A status string decoded against the wire contract, keeping the original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Known<T> {
    Known(T),
    /// A status this build does not know about, kept as sent.
    Unknown(String),
}

impl<T> Known<T> {
    pub fn as_known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }
}

fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Known<T> {
    // The enums are `#[serde(rename_all = "snake_case")]`, so the wire string
    // is the deserializer's input directly.
    match serde_json::from_value::<T>(serde_json::Value::String(raw.to_owned())) {
        Ok(value) => Known::Known(value),
        Err(_) => Known::Unknown(raw.to_owned()),
    }
}

pub fn tunnel_status(raw: &str) -> Known<TunnelStatus> {
    decode(raw)
}

pub fn file_status(raw: &str) -> Known<FileStatus> {
    decode(raw)
}

/// True when a tunnel has reached a state no further transfer can happen in.
/// `ftnl status` exits non-zero for these so a polling script can stop.
pub fn is_terminal(raw: &str) -> bool {
    matches!(
        tunnel_status(raw).as_known(),
        Some(TunnelStatus::Cancelled | TunnelStatus::Expired)
    )
}

/// True when a declared file has bytes that can actually be downloaded.
pub fn is_downloadable(raw: &str) -> bool {
    matches!(
        file_status(raw).as_known(),
        Some(FileStatus::Available | FileStatus::Downloaded)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_strings_decode_to_the_shared_enums() {
        assert_eq!(
            tunnel_status("transferring"),
            Known::Known(TunnelStatus::Transferring)
        );
        assert_eq!(
            file_status("available"),
            Known::Known(FileStatus::Available)
        );
    }

    #[test]
    fn unknown_statuses_are_preserved_rather_than_rejected() {
        // A newer server may invent a status; the contract says ignore what you
        // do not know, not fail.
        assert_eq!(
            tunnel_status("quantum-entangled"),
            Known::Unknown("quantum-entangled".to_owned())
        );
        assert!(!is_terminal("quantum-entangled"));
        assert!(!is_downloadable("quantum-entangled"));
    }

    #[test]
    fn terminal_and_downloadable_match_the_contract() {
        assert!(is_terminal("cancelled"));
        assert!(is_terminal("expired"));
        assert!(!is_terminal("waiting"));
        assert!(
            !is_terminal("complete"),
            "complete still has files to fetch"
        );

        assert!(is_downloadable("available"));
        assert!(is_downloadable("downloaded"));
        assert!(!is_downloadable("declared"));
        assert!(!is_downloadable("uploading"));
    }
}
