//! End-to-end assertions on the built binary.
//!
//! The unit tests in `src/` cover parsing and selection; these cover the things
//! only a real process can show: that `--help` is rendered from
//! `.cli-flags.toml` rather than a Rust string, that credentials are not
//! accepted as flags, and that the documented exit codes are what a script
//! actually observes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn ftnl(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ftnl"))
        .args(args)
        .current_dir(repo_root())
        // Pin the width so the table layout does not depend on the terminal
        // running the test, and clear inherited credentials so the assertions
        // below do not depend on the developer's shell.
        .env("COLUMNS", "100")
        .env_remove("FTNL_CAPABILITY")
        .env_remove("FTNL_PAIRING_URI")
        .output()
        .expect("the ftnl binary should run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn root_help_lists_every_declared_command() {
    let output = ftnl(&["--help"]);
    assert!(output.status.success());
    let help = stdout(&output);

    for command in ["create", "status", "send", "recv", "cancel", "completion"] {
        assert!(help.contains(command), "root help omits {command}:\n{help}");
    }
    for flag in ["--url", "--tunnel", "--json", "FTNL_TIMEOUT_MS"] {
        assert!(help.contains(flag), "root help omits {flag}:\n{help}");
    }
}

#[test]
fn credentials_never_appear_in_the_help_table() {
    // They are `[env] ignore` entries read by src/secrets.rs, not flags. If one
    // ever becomes a flag, its value would show up in `ps` and shell history.
    let help = stdout(&ftnl(&["--help"]));
    for secret in ["FTNL_CAPABILITY", "FTNL_PAIRING_URI", "--capability"] {
        assert!(
            !help.contains(secret),
            "{secret} is documented as a flag:\n{help}"
        );
    }
}

#[test]
fn command_scoped_flags_appear_only_under_their_command() {
    let scoped = stdout(&ftnl(&["create", "--help"]));
    assert!(scoped.contains("--application-id"), "{scoped}");

    let root = stdout(&ftnl(&["--help"]));
    assert!(
        !root.contains("--application-id"),
        "a command-scoped flag leaked into the root help table:\n{root}"
    );

    let output = ftnl(&["cancel", "--application-id=x"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown command-line option"));
}

#[test]
fn completion_scripts_are_emitted_for_both_shells() {
    for shell in ["bash", "zsh"] {
        let output = ftnl(&["completion", "--shell", shell]);
        assert!(output.status.success(), "{shell} completion failed");
        assert!(stdout(&output).contains("ftnl"), "{shell} names no command");
    }
    assert_eq!(
        ftnl(&["completion", "--shell", "fish"]).status.code(),
        Some(2)
    );
}

#[test]
fn exit_codes_match_the_documented_contract() {
    // 2 — bad invocation.
    assert_eq!(ftnl(&["status", "--nope"]).status.code(), Some(2));
    assert_eq!(ftnl(&["definitely-not-a-command"]).status.code(), Some(2));
    assert_eq!(
        ftnl(&["status", "--tunnel=not-a-uuid"]).status.code(),
        Some(2)
    );
    // A missing credential is a usage error, and it must not reach the network.
    assert_eq!(
        ftnl(&["status", "--tunnel=00000000-0000-0000-0000-000000000000"])
            .status
            .code(),
        Some(2)
    );

    // 3 — the contract itself could not be read.
    let broken = Command::new(env!("CARGO_BIN_EXE_ftnl"))
        .arg("status")
        .current_dir(repo_root())
        .env("FTNL_FLAGS_CONFIG", "/nonexistent/.cli-flags.toml")
        .output()
        .expect("the ftnl binary should run");
    assert_eq!(broken.status.code(), Some(3));
}

#[test]
fn a_missing_credential_names_the_variable_it_wants() {
    let output = ftnl(&["status", "--tunnel=00000000-0000-0000-0000-000000000000"]);
    let message = stderr(&output);
    assert!(message.contains("FTNL_CAPABILITY"), "{message}");
    assert!(message.contains("never a flag"), "{message}");
}

#[test]
fn rejected_option_values_are_not_reflected_back() {
    let sentinel = "must-remain-environment-only";
    let output = ftnl(&["status", &format!("--capability={sentinel}")]);
    assert_eq!(output.status.code(), Some(2));
    let combined = format!("{}{}", stdout(&output), stderr(&output));
    assert!(combined.contains("--capability"));
    assert!(!combined.contains(sentinel));
}
