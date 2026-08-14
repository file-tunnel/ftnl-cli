# shellcheck shell=bash
set -euo pipefail

readonly flags2env_rev="8c8465561075a8d7ebb58074b3a8087138e97d8f"
work_dir="$(mktemp -d)"
trap 'rm -rf -- "$work_dir"' EXIT

cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
python3 scripts/check-zed-dependencies.py
actionlint

git clone --filter=blob:none --no-checkout https://github.com/ORESoftware/flags-2-env.git "$work_dir/flags-2-env"
git -C "$work_dir/flags-2-env" fetch --depth 1 origin "$flags2env_rev"
git -C "$work_dir/flags-2-env" checkout --detach FETCH_HEAD
make -C "$work_dir/flags-2-env" cli
"$work_dir/flags-2-env/build/flags2env" audit .cli-flags.toml
diff -u src/cli_config.rs <(
  "$work_dir/flags-2-env/build/flags2env" generate rust .cli-flags.toml --name CliConfig
)
