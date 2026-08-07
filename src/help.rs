//! `--help` and shell completions, rendered by the flags-2-env C core.
//!
//! Everything here is derived from `.cli-flags.toml` at runtime, so there is no
//! second copy of the flag list to keep in sync. The table is subcommand-aware:
//! `ftnl --help` lists the commands, `ftnl status --help` shows that
//! command's own flags plus the inherited global ones.
//!
//! The `flags2env` crate compiles the vendored C parser through its build
//! script and statically links it into this binary, which is why these symbols
//! resolve without a `libflags2env` shared library at runtime. Cargo only links
//! that object when the `flags2env` Rust crate is actually referenced, so every
//! entry point here starts by constructing the client — see [`native_core`].

//! FFI is confined to this module: the crate denies `unsafe_code` everywhere
//! else, and this is the only place that needs it.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::Path;

use flags2env::BundledFlags2Env;

use crate::error::CliError;

unsafe extern "C" {
    fn f2e_is_help_requested_json_argv(argv_json: *const c_char) -> c_int;
    fn f2e_help_table_for_json_argv_from_file(
        config_path: *const c_char,
        command_name: *const c_char,
        argv_json: *const c_char,
        terminal_columns: c_int,
    ) -> *mut c_char;
    fn f2e_completion_script_from_file(
        config_path: *const c_char,
        shell: *const c_char,
        command_name: *const c_char,
    ) -> *mut c_char;
    fn f2e_free(value: *mut c_char);
}

/// Shells `flags2env completion` knows how to emit.
pub const SUPPORTED_SHELLS: [&str; 2] = ["bash", "zsh"];

/// Anchors the statically linked C core into the link graph. Constructing the
/// zero-sized client is free; dropping this call would let Cargo omit the
/// native object and the `f2e_*` symbols above would fail to resolve.
#[inline]
fn native_core() -> BundledFlags2Env {
    BundledFlags2Env::new()
}

/// True when argv contains the exact `--help`/`-h` token, per the core's own
/// rules (so the binary and `flags2env` agree on what "asked for help" means).
pub fn is_help_requested(argv: &[String]) -> bool {
    let _core = native_core();
    let Ok(argv_json) = encode_argv(argv) else {
        return false;
    };
    // SAFETY: `argv_json` is a valid NUL-terminated JSON array of strings and
    // outlives the call; the core only reads it.
    unsafe { f2e_is_help_requested_json_argv(argv_json.as_ptr()) == 1 }
}

/// The help table for whichever command `argv` selects.
pub fn help_table(config_path: &Path, program: &str, argv: &[String]) -> Result<String, CliError> {
    let _core = native_core();
    let config = c_string(config_path_str(config_path)?)?;
    let program = c_string(program)?;
    let argv_json = encode_argv(argv)?;
    // SAFETY: all three CStrings outlive the call, and the returned pointer is
    // owned by the caller — `take_owned` releases it through `f2e_free`.
    let table = unsafe {
        take_owned(f2e_help_table_for_json_argv_from_file(
            config.as_ptr(),
            program.as_ptr(),
            argv_json.as_ptr(),
            terminal_columns(),
        ))
    };
    table.ok_or_else(|| CliError::config("flags-2-env could not render the help table"))
}

/// A static completion script for `shell` — it does no TOML reading or process
/// spawning at tab-completion time.
pub fn completion_script(
    config_path: &Path,
    shell: &str,
    program: &str,
) -> Result<String, CliError> {
    let _core = native_core();
    if !SUPPORTED_SHELLS.contains(&shell) {
        return Err(CliError::usage(format!(
            "unsupported shell {shell:?}; expected one of: {}",
            SUPPORTED_SHELLS.join(", ")
        )));
    }
    let config = c_string(config_path_str(config_path)?)?;
    let shell = c_string(shell)?;
    let program = c_string(program)?;
    // SAFETY: as above — borrowed inputs outlive the call, result is owned.
    let script = unsafe {
        take_owned(f2e_completion_script_from_file(
            config.as_ptr(),
            shell.as_ptr(),
            program.as_ptr(),
        ))
    };
    script.ok_or_else(|| CliError::config("flags-2-env could not render the completion script"))
}

/// Width used for the help table. `COLUMNS` is honoured when it is a sane
/// number so piped output stays reproducible; otherwise the core picks a
/// layout for the default width.
fn terminal_columns() -> c_int {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.trim().parse::<c_int>().ok())
        .filter(|columns| (40..=400).contains(columns))
        .unwrap_or(100)
}

fn encode_argv(argv: &[String]) -> Result<CString, CliError> {
    let json = serde_json::to_string(argv)?;
    c_string(&json)
}

fn c_string(value: &str) -> Result<CString, CliError> {
    CString::new(value)
        .map_err(|_| CliError::usage("arguments must not contain interior NUL bytes"))
}

fn config_path_str(config_path: &Path) -> Result<&str, CliError> {
    config_path
        .to_str()
        .ok_or_else(|| CliError::config(".cli-flags.toml path is not valid UTF-8"))
}

/// Takes ownership of a heap string returned by the C core, copying it into a
/// `String` and releasing the original through `f2e_free`.
///
/// # Safety
///
/// `value` must be null or a pointer returned by an `F2E_OWNED_RESULT` function
/// that has not already been freed.
unsafe fn take_owned(value: *mut c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let owned = unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned();
    unsafe { f2e_free(value) };
    Some(owned)
}
