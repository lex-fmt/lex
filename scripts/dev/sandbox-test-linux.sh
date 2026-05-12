#!/usr/bin/env bash
# Run sandbox tests inside a Linux container — for macOS contributors
# who need to exercise the seccomp+landlock path without a VM. CI runs
# the same suite on ubuntu-latest directly via .github/workflows/sandbox-tests.yml.
#
# Usage:
#   scripts/dev/sandbox-test-linux.sh                # nextest sandbox:: in lex-extension-host
#   scripts/dev/sandbox-test-linux.sh <cmd> [args]   # run an arbitrary command in the container
#
# Notes:
# - The image is built on first run; subsequent runs reuse it. To force
#   a rebuild, `docker rmi lex-sandbox-dev:latest` first.
# - Named volumes cache /target and the cargo registry so incremental
#   rebuilds are fast across runs.
# - No --privileged / --cap-add needed: landlock works unprivileged on
#   kernels ≥5.13 and seccomp filter mode works with no_new_privs.
# - Defaults to the host's native arch (arm64 on Apple Silicon). Set
#   `SANDBOX_PLATFORM=linux/amd64` to validate the x86_64 path under
#   emulation.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
IMAGE="lex-sandbox-dev:latest"
DOCKERFILE="$REPO_ROOT/scripts/dev/Dockerfile.sandbox"
PLATFORM_FLAG=()
if [[ -n "${SANDBOX_PLATFORM:-}" ]]; then
    PLATFORM_FLAG=(--platform "$SANDBOX_PLATFORM")
fi

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    docker build "${PLATFORM_FLAG[@]}" -t "$IMAGE" -f "$DOCKERFILE" "$REPO_ROOT/scripts/dev"
fi

if [[ $# -eq 0 ]]; then
    set -- cargo nextest run -p lex-extension-host sandbox::
fi

exec docker run --rm \
    "${PLATFORM_FLAG[@]}" \
    -v "$REPO_ROOT:/work" \
    -v lex-sandbox-target:/target \
    -v lex-sandbox-cargo-registry:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/target \
    -w /work \
    "$IMAGE" \
    "$@"
