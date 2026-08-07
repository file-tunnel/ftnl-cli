# ftnl-cli

The `ftnl` command-line client for [File Tunnel](https://github.com/file-tunnel).

A tunnel moves files between two devices that never talk to each other directly.
A desktop creates a tunnel and shows a pairing URI as a QR code; a phone claims
it **once**, declares a file, and uploads; the desktop downloads. This CLI drives
both sides.

```sh
# desktop: open a tunnel
ftnl create --application-id my-app --accept '["image/*","application/pdf"]'
# tunnel      6f1c…            <- --tunnel for every later command
# pairing uri ftnl://…#c=…     <- render as a QR code

export FTNL_CAPABILITY='…'          # printed by create; a credential
export FTNL_PAIRING_URI='ftnl://…'  # printed by create; a one-time secret

# phone: claim, declare, and upload in one step
ftnl send --tunnel 6f1c… --file ./photo.png --media-type image/png

# desktop: watch, then fetch
ftnl status --tunnel 6f1c…
ftnl recv   --tunnel 6f1c… --out ./photo.png

ftnl cancel --tunnel 6f1c…
```

## Credentials are environment-only, by design

Capabilities and pairing secrets are bearer credentials. A flag value is visible
in `ps` output, in shell history, and in CI logs that echo their commands, so
**neither is a flag**. Both are declared under `[env] ignore` in
[`.cli-flags.toml`](.cli-flags.toml) and read from the environment by
[`src/secrets.rs`](src/secrets.rs):

| Variable | Meaning |
|----------|---------|
| `FTNL_CAPABILITY` | tunnel capability, from `ftnl create` or a claim |
| `FTNL_PAIRING_URI` | the full pairing URI; `send` extracts the secret from its fragment |

Passing either as a flag is a rejected unknown option, and the diagnostic names
the option without echoing its value. The pairing secret is read from the URI
**fragment** using the org client's own parser — fragments are never sent to a
server, and a second parser that got that wrong would quietly turn a one-time
credential into a query parameter.

## Configuration — flags-2-env

Flags are declared once in [`.cli-flags.toml`](.cli-flags.toml), the
[flags-2-env](https://github.com/ORESoftware/flags-2-env) config format. Each
flag maps to an environment variable, and precedence is
**CLI flags > environment > TOML defaults**:

```sh
export FTNL_BASE_URL=https://api.file-tunnel.dev   # = --url / -u
export FTNL_TIMEOUT_MS=15000                       # = --timeout
export FTNL_JSON=1                                 # = --json / -j
ftnl status --tunnel 6f1c…                         # uses the environment
ftnl status --tunnel 6f1c… --timeout 5000          # the flag still wins
```

`--help` is rendered by the flags-2-env core from that file at runtime — there
is no usage string in the Rust source to drift — and it is subcommand-aware:

```sh
ftnl --help          # global flags + the command table
ftnl create --help   # create's own flags, plus inherited global ones
```

Some flags are **command-scoped**: `--application-id` exists under `create`,
`--file` under `send`, `--out` under `recv`. Outside its command a scoped flag is
a rejected unknown option rather than a silently ignored one.

Shell completions come from the same contract and are **static** — the generated
script does no TOML reading and spawns no process while you are pressing Tab:

```sh
ftnl completion --shell bash > "${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions/ftnl"
ftnl completion --shell zsh  > "${ZDOTDIR:-$HOME}/.zfunc/_ftnl"
```

[`src/cli_config.rs`](src/cli_config.rs) is generated from the contract and CI
diffs it against fresh generator output, so the typed struct cannot drift from
the flags:

```sh
flags2env generate rust .cli-flags.toml --name CliConfig > src/cli_config.rs
```

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | success |
| `1` | the invocation was valid but the work failed (transport or API error, unreadable file) |
| `2` | bad invocation: unknown flag or command, malformed UUID, missing credential, ambiguous `recv` target |
| `3` | `.cli-flags.toml` could not be found, read, or audited |

`ftnl status` also exits `1` when the tunnel has reached a terminal state
(`cancelled`, `expired`), so a polling loop terminates instead of spinning.

## Org dependencies

This CLI sits at the bottom of the File Tunnel stack and owns no routes, header
names, or retry policy of its own. The same two edges are declared in
[`Cargo.toml`](Cargo.toml) (pinned by full commit rev) and in
[`.zpkg.toml`](.zpkg.toml) as the [zed](https://github.com/zed-pkg/zed-cli)
dependency graph:

```toml
[dependencies]
"file-tunnel/ftnl-interfaces" = "^0.1.0"   # wire shapes: TunnelStatus, FileStatus
"file-tunnel/ftnl-clients" = "^0.1.0"      # transport: every request goes through it
```

`file-tunnel` has no `ftnl-lib`; when one lands it belongs between those two.
[`scripts/check-zed-dependencies.py`](scripts/check-zed-dependencies.py) fails CI
if the two manifests disagree, or if a declared zed edge is not also a real Cargo
dependency — a manifest that names a repo that does not exist looks right and
never resolves.

Status strings from the transport are decoded against the shared
`ftnl-interfaces` enums in [`src/status.rs`](src/status.rs) rather than compared
to string literals. A status this build does not recognise is shown verbatim, not
rejected: the contract says clients must ignore what they do not know.

## Layout

No module does two jobs, and `main.rs` does almost nothing:

| File | Responsibility |
|------|----------------|
| `src/main.rs` | argv in, exit code out |
| `src/lib.rs` | module wiring + top-level `run` |
| `src/flags.rs` | contract audit, parse, precedence, coercion, range checks |
| `src/cli_config.rs` | generated typed representation of `.cli-flags.toml` |
| `src/help.rs` | help tables + completion scripts from the native core |
| `src/secrets.rs` | environment-only credentials |
| `src/status.rs` | typed tunnel/file status from `ftnl-interfaces` |
| `src/commands/` | one module per subcommand |
| `src/output.rs` / `src/error.rs` | human-vs-JSON, and `CliError` |

`unsafe_code` is denied crate-wide; `src/help.rs` is the single module that opts
itself out, because binding to the flags-2-env C core needs it.

## Build

```sh
cargo build --locked --release      # target/release/ftnl
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
python3 scripts/check-zed-dependencies.py   # needs Python 3.11+
```

Building compiles the vendored C parser that `flags2env` links statically, so the
released binary needs no `libflags2env` at runtime. The Rust `cc` crate picks the
platform compiler; a packaged `ftnl` binary needs no compiler installed.
