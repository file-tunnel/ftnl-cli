#!/usr/bin/env python3
"""Enforce the org dependency graph declared in `.zpkg.toml`.

A CLI is the bottom of its org's stack: it consumes the shared contracts
(`*-interfaces`), the shared behaviour (`*-lib`) where the org has one, and the
transport (`*-clients`). Getting the org segment wrong — or naming a repo that
does not exist — produces a manifest that looks right and never resolves, which
is exactly the failure this script exists to catch.

Every declared edge must also be a real Cargo dependency, so the zed graph
describes the build rather than decorating it.
"""

from pathlib import Path
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11
    sys.exit(
        "check-zed-dependencies needs Python 3.11+ for tomllib "
        f"(running {sys.version.split()[0]})"
    )

ROOT = Path(__file__).resolve().parents[1]

# org/repo -> the Cargo package name that edge must also appear as.
EXPECTED: dict[str, str] = {
    "file-tunnel/ftnl-interfaces": "ftnl-interfaces",
    "file-tunnel/ftnl-clients": "ftnl-client",
    "file-tunnel/ftnl-sync": "ftnl-sync",
}

PACKAGE_NAME = "ftnl-cli"


def main() -> int:
    manifest = tomllib.loads((ROOT / ".zpkg.toml").read_text(encoding="utf-8"))
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    errors: list[str] = []

    if manifest.get("package", {}).get("name") != PACKAGE_NAME:
        errors.append(f"package.name must be {PACKAGE_NAME}")

    declared = set(manifest.get("dependencies", {}))
    missing = set(EXPECTED) - declared
    unexpected = declared - set(EXPECTED)
    if missing:
        errors.append("missing zed dependencies: " + ", ".join(sorted(missing)))
    if unexpected:
        errors.append("unexpected zed dependencies: " + ", ".join(sorted(unexpected)))

    cargo_dependencies = set(cargo.get("dependencies", {}))
    for edge, crate in EXPECTED.items():
        if crate and crate not in cargo_dependencies:
            errors.append(f"{edge} is declared in .zpkg.toml but {crate} is not a Cargo dependency")

    if manifest.get("install", {}).get("dir") != ".vendor/.zed":
        errors.append("install.dir must be .vendor/.zed")
    if manifest.get("install", {}).get("adapter") != "none":
        # A CLI is a universal executable package; it must not be wired into
        # node_modules, a Java classpath, or a Go workspace.
        errors.append("install.adapter must be none")

    if errors:
        print(f"{PACKAGE_NAME} zed dependency validation failed:", file=sys.stderr)
        for error in errors:
            print(f" - {error}", file=sys.stderr)
        return 1

    print(f"validated the {PACKAGE_NAME} zed dependency graph")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
