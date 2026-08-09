# File Tunnel CLI agent instructions

These instructions apply to this repository and every directory beneath it.

- `.cli-flags.toml` is the sole flag/env/help contract. Regenerate
  `src/cli_config.rs` with the pinned flags-2-env revision and keep CI's diff
  check exact.
- Capabilities and pairing URIs are environment-only credentials. Never add
  them as flags, persist them, or echo rejected values.
- All HTTP routes, authorization headers, and response redaction belong to
  `ftnl-client`; do not duplicate transport code here.
- Upload lifecycle metadata goes through `ftnl-sync` protocol-v1 durability.
  Never add file bytes, local paths, capabilities, tickets, or provider URLs to
  `UploadJob` or its SQLite queue.
- Download paths supplied by a server must remain one safe path component.
  Preserve atomic output and no-clobber-by-default behavior.
- `.zpkg.toml` edges must correspond to real Cargo dependencies. Do not edit
  `.zpkg.lock` or materialize `zed_modules` as part of application changes.
- Run format, locked Clippy/tests, generated-config diff, dependency validation,
  and actionlint before publishing.
