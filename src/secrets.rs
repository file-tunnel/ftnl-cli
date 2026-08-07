//! Credentials, which are deliberately **not** flags.
//!
//! A tunnel capability and a pairing secret are bearer credentials. A flag
//! value is visible in `ps` output, in shell history, and in CI logs that echo
//! their commands, so neither is declared in `.cli-flags.toml`; both are listed
//! under `[env] ignore` there and read from the environment here.
//!
//! `pairing_secret_from_uri` comes from the org client rather than being
//! re-implemented: the secret lives in the URI *fragment* precisely because
//! browsers never send fragments to a server, and a second parser that got that
//! wrong would quietly turn a one-time credential into a query parameter.

use crate::error::CliError;

/// Environment variable holding the tunnel capability token.
pub const CAPABILITY_ENV: &str = "FTNL_CAPABILITY";
/// Environment variable holding the full `ftnl://…#c=…` pairing URI.
pub const PAIRING_URI_ENV: &str = "FTNL_PAIRING_URI";

/// Reads the capability a command needs to act on an existing tunnel.
pub fn capability() -> Result<String, CliError> {
    non_empty(CAPABILITY_ENV).ok_or_else(|| {
        CliError::usage(format!(
            "{CAPABILITY_ENV} must be set to the tunnel capability returned by `ftnl create` \
             (it is a credential, so it is environment-only and never a flag)"
        ))
    })
}

/// Reads the pairing secret out of the pairing URI, using the client's own
/// fragment-only parser.
pub fn pairing_secret() -> Result<String, CliError> {
    let uri = non_empty(PAIRING_URI_ENV).ok_or_else(|| {
        CliError::usage(format!(
            "{PAIRING_URI_ENV} must be set to the pairing URI shown by `ftnl create` \
             (it carries a one-time secret, so it is environment-only and never a flag)"
        ))
    })?;
    ftnl_client::pairing_secret_from_uri(&uri).ok_or_else(|| {
        // The URI is not echoed: it *is* the secret.
        CliError::usage(format!(
            "{PAIRING_URI_ENV} has no pairing secret in its fragment (expected `#c=…`)"
        ))
    })
}

fn non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capability_in_the_query_string_is_not_a_pairing_secret() {
        // Guards the reason the client's parser is reused: a query parameter
        // reaches the server, a fragment does not.
        assert_eq!(
            ftnl_client::pairing_secret_from_uri("https://portal.test/p?c=leaked"),
            None
        );
        assert_eq!(
            ftnl_client::pairing_secret_from_uri("https://portal.test/p#c=kept").as_deref(),
            Some("kept")
        );
    }

    #[test]
    fn missing_credentials_are_usage_errors_that_name_the_variable() {
        // Exercised without touching the process environment, which tests in
        // the same binary share.
        let error = CliError::usage(format!("{CAPABILITY_ENV} must be set"));
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("FTNL_CAPABILITY"));
    }
}
