#!/usr/bin/env bash
# Publish one workspace crate to crates.io, tolerating "already uploaded".
#
# Usage: scripts/ci-publish-crate.sh <crate-name>
#
# After a successful publish (or detection that the version already exists),
# polls crates.io until the exact version is indexed, so the next dependent
# crate in the release pipeline can see it.

set -euo pipefail

crate="${1:?crate name required}"

version=$(
    cargo metadata --format-version 1 --no-deps \
        | python3 -c "
import json, sys
pkgs = json.load(sys.stdin)['packages']
for p in pkgs:
    if p['name'] == '$crate':
        print(p['version'])
        break
"
)

if [[ -z "$version" ]]; then
    echo "✗ could not resolve version for crate '$crate'" >&2
    exit 1
fi

echo "→ publishing $crate $version"

already_uploaded() {
    curl -sf -o /dev/null "https://crates.io/api/v1/crates/$crate/$version"
}

if already_uploaded; then
    echo "✓ $crate $version already on crates.io — skipping publish"
else
    log=$(mktemp)
    if cargo publish -p "$crate" 2>&1 | tee "$log"; then
        echo "✓ $crate $version publish invoked"
    elif grep -q "already uploaded" "$log" \
        || grep -q "crate version .* is already uploaded" "$log"; then
        echo "✓ $crate $version already uploaded (race) — continuing"
    else
        echo "✗ $crate $version publish failed" >&2
        exit 1
    fi
fi

echo "→ waiting for $crate $version to index"
for _ in $(seq 1 60); do
    if already_uploaded; then
        echo "✓ $crate $version indexed"
        exit 0
    fi
    sleep 5
done

echo "✗ timed out waiting for $crate $version to index" >&2
exit 1
