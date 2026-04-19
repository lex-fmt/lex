#!/usr/bin/env bash
# Bump workspace versions, tag, and push — CI takes over from there.
#
# Usage:  scripts/release.sh <patch|minor|major|X.Y.Z>
#
# With release.toml's `publish = false`, cargo-release only commits, tags,
# and pushes. .github/workflows/release.yml then builds binaries and
# publishes crates to crates.io.

set -euo pipefail

level="${1:?usage: scripts/release.sh <patch|minor|major|X.Y.Z>}"

exec cargo release "$level" --workspace --execute --no-confirm
