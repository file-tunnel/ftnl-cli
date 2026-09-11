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
- Run `nix develop --command agent-check` before publishing; it covers format,
  locked Clippy/tests, generated-config diff, dependency validation, and
  actionlint.
- Pull and merge remote work before pushing; avoid git rebase in favor of git merge.
- Never discard unrelated or uncommitted user work.

## Functional programming conformance

This repository carries an FP conformance ratchet. Before you land a change:

```sh
python3 tools/fp-conformance/fp_conformance.py .
```

CI compares your findings against `tools/fp-conformance/budget.json` and fails
only when a rule's count *increases*. Do not raise the budget to get green — fix
the new violations. When you clear a class of violation, lower the budget in the
same commit with `--write-budget`.

The principles, the rule codes and the remedy for each are in `FP-GUIDELINES.md`.
