//! `ftnl` — command-line tool for file-tunnel.
//!
//! This file stays thin on purpose: argv in, exit code out. The CLI surface is
//! declared in `.cli-flags.toml` and the behaviour lives in the library modules
//! documented in `src/lib.rs`.

fn main() {
    let argv = std::env::args().collect::<Vec<_>>();
    std::process::exit(ftnl_cli::run(&argv));
}
