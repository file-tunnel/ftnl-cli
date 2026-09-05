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

## Repository-local Git worktrees

- Create or use a Git worktree only when the human operator explicitly authorizes it for the current task. Concurrency or a dirty checkout is not permission by itself.
- Put every authorized worktree at `<repository-root>/tmp/worktrees/<name>`; from the repository root, use `./tmp/worktrees/<name>`. Never place worktrees beside repositories or organization directories.
- Keep `tmp`, `temp`, `tmp/worktrees`, and `temp/worktrees` ignored in the repository-root `.gitignore`. Do not commit files from those directories.
- Relocate or remove a worktree only when the operator explicitly requests it. Before removal, preserve and publish intended changes, verify its commit is represented on the target branch, and confirm there are no tracked, untracked, ignored-sensitive, or in-use files that must survive. Remove it with `git worktree remove <path>` without `--force`; never delete a worktree directory with `rm`.
