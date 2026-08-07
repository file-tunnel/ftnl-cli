//! argv + environment → a validated [`CliArgs`], through flags-2-env.
//!
//! The order of operations is the same in every one of our CLIs:
//!
//! 1. **audit** `.cli-flags.toml` — a malformed contract is a config error, not
//!    a mysterious parse failure later;
//! 2. **`parse_structured`** — argv-derived values, the resolved command, and
//!    the diagnostic channels come back *separately*, so a real environment
//!    variable can never be mistaken for something the user typed;
//! 3. **fail closed** on unknown options, invalid values, and stray operands;
//! 4. **layer** schema defaults < process environment < argv, then `coerce`;
//! 5. **range-check** the typed values here, where the message can name the flag.
//!
//! Step 4 is why `provided_flags` is used rather than `flags`: `flags` carries
//! TOML defaults, and spreading those over the real environment would let a
//! default silently beat an environment value the operator set.
//!
//! Credentials never appear in any of this — see [`crate::secrets`].

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use flags2env::BundledFlags2Env;
use uuid::Uuid;

use crate::cli_config::CliConfig;
use crate::commands::Command;
use crate::error::CliError;
use crate::help::SUPPORTED_SHELLS;

/// Everything a command needs, already validated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliArgs {
    pub command: Command,
    pub base_url: String,
    /// Parsed once here so no command has to re-validate the UUID.
    pub tunnel: Option<Uuid>,
    pub timeout: Duration,
    pub json: bool,

    // `create`
    pub application_id: String,
    pub accept: Vec<String>,
    pub max_files: u16,
    pub max_file_bytes: u64,
    pub expires_in_seconds: u32,

    // `send`
    pub file: Option<PathBuf>,
    pub media_type: String,

    // `recv`
    pub file_id: Option<Uuid>,
    pub out: Option<PathBuf>,

    // `completion`
    pub shell: String,
}

impl CliArgs {
    /// The tunnel every command except `create`/`completion` operates on.
    pub fn require_tunnel(&self) -> Result<Uuid, CliError> {
        self.tunnel
            .ok_or_else(|| CliError::usage("--tunnel is required (a tunnel UUID)"))
    }

    /// The local file `send` uploads.
    pub fn require_file(&self) -> Result<&Path, CliError> {
        self.file
            .as_deref()
            .ok_or_else(|| CliError::usage("--file is required (path of the file to upload)"))
    }
}

/// Finds `.cli-flags.toml`: an explicit override, then the working directory,
/// then next to the installed binary — which is what makes a globally installed
/// `ftnl` work from any directory.
pub fn resolve_config_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("FTNL_FLAGS_CONFIG").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        return path
            .is_file()
            .then_some(path)
            .ok_or_else(|| "FTNL_FLAGS_CONFIG does not point to a readable file".to_owned());
    }

    let mut candidates = Vec::new();
    if let Ok(current) = std::env::current_dir() {
        candidates.push(current.join(".cli-flags.toml"));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join(".cli-flags.toml"));
            candidates.push(parent.join("../share/ftnl-cli/.cli-flags.toml"));
        }
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            "cannot locate .cli-flags.toml; set FTNL_FLAGS_CONFIG to its path".to_owned()
        })
}

pub fn parse_cli_args(argv: &[String], config_path: &Path) -> Result<CliArgs, String> {
    let environment = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)));
    parse_cli_args_with_env(argv, config_path, environment)
}

fn parse_cli_args_with_env(
    argv: &[String],
    config_path: &Path,
    environment: impl IntoIterator<Item = (String, String)>,
) -> Result<CliArgs, String> {
    let config_path = config_path
        .to_str()
        .ok_or_else(|| ".cli-flags.toml path is not valid UTF-8".to_owned())?;
    let parser = BundledFlags2Env::new();
    parser
        .audit_config(Some(config_path))
        .map_err(|error| format!("flags-2-env configuration audit failed: {error}"))?;
    let parsed = parser
        .parse_structured(argv, Some(config_path))
        .map_err(|error| format!("flags-2-env parse failed: {error}"))?;

    if !parsed.unknown_options.is_empty() {
        let option_names = parsed
            .unknown_options
            .iter()
            .map(|option| diagnostic_option_name(option))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("unknown command-line option(s): {option_names}"));
    }
    if !parsed.errors.is_empty() {
        return Err(format!(
            "invalid command-line value(s): {}",
            parsed.errors.join("; ")
        ));
    }
    if !parsed.extras.is_empty() {
        // Values are not echoed: an operand is as likely to be a credential as
        // a typo, and the count is enough to spot the mistake.
        return Err(format!(
            "unknown command or unexpected positional argument(s): {}",
            parsed.extras.len()
        ));
    }

    let mut raw_config = environment.into_iter().collect::<HashMap<_, _>>();
    // Command metadata is parser output, never operator input.
    raw_config.remove("FLAGS2ENV_COMMAND");
    raw_config.extend(parsed.provided_flags);
    let typed = parser
        .coerce::<CliConfig, _>(&raw_config, Some(config_path))
        .map_err(|error| format!("invalid typed configuration: {error}"))?;

    let command = match typed.FLAGS2ENV_COMMAND.as_deref() {
        None | Some("") => {
            return Err(
                "a command is required: create, status, send, recv, cancel, or completion"
                    .to_owned(),
            )
        }
        Some(label) => Command::parse(label).map_err(|error| error.to_string())?,
    };

    let base_url = typed.FTNL_BASE_URL;
    if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Err("--url must start with http:// or https://".to_owned());
    }

    let tunnel = optional_uuid(typed.FTNL_TUNNEL_ID.as_deref(), "--tunnel")?;
    let file_id = optional_uuid(typed.FTNL_FILE_ID.as_deref(), "--file-id")?;

    let timeout_ms = bounded(typed.FTNL_TIMEOUT_MS, "FTNL_TIMEOUT_MS", 100, 600_000)?;

    // Scoped defaults only apply when their command runs, so each scoped flag
    // restates its default here for the other commands' sake.
    let max_files = bounded(
        typed.FTNL_MAX_FILES.unwrap_or(10),
        "FTNL_MAX_FILES",
        1,
        1_000,
    )? as u16;
    let max_file_bytes = bounded(
        typed.FTNL_MAX_FILE_BYTES.unwrap_or(52_428_800),
        "FTNL_MAX_FILE_BYTES",
        1,
        5_368_709_120,
    )? as u64;
    let expires_in_seconds = bounded(
        typed.FTNL_EXPIRES_IN_SECONDS.unwrap_or(600),
        "FTNL_EXPIRES_IN_SECONDS",
        30,
        86_400,
    )? as u32;

    let accept = match typed.FTNL_ACCEPT {
        None => vec!["image/*".to_owned()],
        Some(values) => {
            let accept = values
                .into_iter()
                .map(|value| match value {
                    serde_json::Value::String(media_type) => Ok(media_type),
                    _ => Err("--accept must be a JSON array of strings".to_owned()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if accept.is_empty() {
                return Err("--accept must list at least one media type".to_owned());
            }
            accept
        }
    };

    let application_id = typed.FTNL_APPLICATION_ID.unwrap_or_default();
    if command == Command::Create && application_id.trim().is_empty() {
        return Err("--application-id is required by create".to_owned());
    }

    let shell = typed.FTNL_COMPLETION_SHELL.unwrap_or_else(|| "bash".into());
    if command == Command::Completion && !SUPPORTED_SHELLS.contains(&shell.as_str()) {
        return Err(format!(
            "--shell must be one of: {}",
            SUPPORTED_SHELLS.join(", ")
        ));
    }

    Ok(CliArgs {
        command,
        base_url,
        tunnel,
        timeout: Duration::from_millis(timeout_ms as u64),
        json: typed.FTNL_JSON,
        application_id,
        accept,
        max_files,
        max_file_bytes,
        expires_in_seconds,
        file: typed.FTNL_FILE.map(PathBuf::from),
        media_type: typed
            .FTNL_MEDIA_TYPE
            .unwrap_or_else(|| "application/octet-stream".to_owned()),
        file_id,
        out: typed.FTNL_OUT.map(PathBuf::from),
        shell,
    })
}

fn optional_uuid(value: Option<&str>, flag: &str) -> Result<Option<Uuid>, String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(raw) => Uuid::parse_str(raw)
            .map(Some)
            // A tunnel id is a routing identifier, not a credential, so echoing
            // the malformed value is safe and makes the typo obvious.
            .map_err(|_| format!("{flag} must be a UUID, got {raw:?}")),
    }
}

/// Strips any `=value` before an unknown option reaches a diagnostic, so a
/// mistyped `--capability=secret` cannot echo the secret.
fn diagnostic_option_name(option: &str) -> String {
    if let Some(long) = option.strip_prefix("--") {
        return format!("--{}", long.split('=').next().unwrap_or_default());
    }
    option.chars().take(2).collect()
}

fn bounded(value: i64, name: &str, min: i64, max: i64) -> Result<i64, String> {
    (min..=max)
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| format!("{name} must be between {min} and {max}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".cli-flags.toml")
    }

    fn parse(tokens: &[&str], environment: &[(&str, &str)]) -> Result<CliArgs, String> {
        parse_cli_args_with_env(
            &tokens
                .iter()
                .map(|token| (*token).to_owned())
                .collect::<Vec<_>>(),
            &config_path(),
            environment
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
        )
    }

    #[test]
    fn parses_a_create_invocation() {
        let parsed = parse(
            &["ftnl", "create", "--app=desk", "--max-files=3", "--json"],
            &[],
        )
        .expect("valid create");
        assert_eq!(parsed.command, Command::Create);
        assert_eq!(parsed.application_id, "desk");
        assert_eq!(parsed.max_files, 3);
        assert!(parsed.json);
        // Scoped default still applies under its own command.
        assert_eq!(parsed.accept, vec!["image/*".to_owned()]);
    }

    #[test]
    fn a_command_is_required() {
        let error = parse(&["ftnl", "--json"], &[]).expect_err("no command");
        assert!(error.contains("a command is required"));
    }

    #[test]
    fn credentials_are_not_accepted_as_flags() {
        // The whole point of src/secrets.rs: these must fail closed as unknown
        // options rather than being quietly accepted onto the command line.
        for token in ["--capability=secret", "--pairing-uri=https://p.test/#c=s"] {
            let error = parse(
                &[
                    "ftnl",
                    "status",
                    "--tunnel",
                    &Uuid::nil().to_string(),
                    token,
                ],
                &[],
            )
            .expect_err("credential flag");
            assert!(
                error.contains("unknown command-line option"),
                "{token}: {error}"
            );
            assert!(
                !error.contains("secret"),
                "{token} leaked its value: {error}"
            );
        }
    }

    #[test]
    fn environment_supplies_credentials_without_appearing_in_the_contract() {
        // FTNL_CAPABILITY is in `[env] ignore`, so it is neither required from
        // the contract nor rejected from the environment.
        let parsed = parse(
            &["ftnl", "status", "--tunnel", &Uuid::nil().to_string()],
            &[("FTNL_CAPABILITY", "cap-token")],
        )
        .expect("ignored env key is accepted");
        assert_eq!(parsed.command, Command::Status);
    }

    #[test]
    fn tunnel_ids_must_be_uuids() {
        let error = parse(&["ftnl", "status", "--tunnel=not-a-uuid"], &[]).expect_err("bad uuid");
        assert!(error.contains("--tunnel must be a UUID"));
    }

    #[test]
    fn command_scoped_flags_stay_in_their_command() {
        let parsed = parse(
            &[
                "ftnl",
                "send",
                "--file=/tmp/x",
                "--tunnel",
                &Uuid::nil().to_string(),
            ],
            &[],
        )
        .expect("scoped flag under its command");
        assert_eq!(parsed.file.as_deref(), Some(Path::new("/tmp/x")));

        let error = parse(&["ftnl", "cancel", "--file=/tmp/x"], &[]).expect_err("out of scope");
        assert!(error.contains("unknown command-line option"));
    }

    #[test]
    fn cli_flags_beat_environment_which_beats_defaults() {
        assert_eq!(
            parse(&["ftnl", "status"], &[]).expect("defaults").timeout,
            Duration::from_millis(30_000)
        );
        assert_eq!(
            parse(&["ftnl", "status"], &[("FTNL_TIMEOUT_MS", "1200")])
                .expect("env")
                .timeout,
            Duration::from_millis(1_200)
        );
        assert_eq!(
            parse(
                &["ftnl", "status", "--timeout=900"],
                &[("FTNL_TIMEOUT_MS", "1200")]
            )
            .expect("flag wins")
            .timeout,
            Duration::from_millis(900)
        );
    }

    #[test]
    fn out_of_range_values_are_rejected_by_name() {
        let error =
            parse(&["ftnl", "create", "--app=d", "--max-files=0"], &[]).expect_err("below range");
        assert!(error.contains("FTNL_MAX_FILES"));
    }

    #[test]
    fn create_requires_an_application_id() {
        assert!(parse(&["ftnl", "create"], &[]).is_err());
    }

    #[test]
    fn accept_must_be_a_json_array_of_strings() {
        let parsed = parse(
            &[
                "ftnl",
                "create",
                "--app=d",
                "--accept=[\"image/*\",\"application/pdf\"]",
            ],
            &[],
        )
        .expect("valid array");
        assert_eq!(parsed.accept, vec!["image/*", "application/pdf"]);

        assert!(parse(&["ftnl", "create", "--app=d", "--accept=[1]"], &[]).is_err());
        assert!(parse(&["ftnl", "create", "--app=d", "--accept=[]"], &[]).is_err());
    }
}
