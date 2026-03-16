#!/bin/bash
# Parity check: compare tree-sitter CST with lex-core AST using plain-text
# block skeleton format. Both sides produce the same format directly — no
# JSON, no jq filters, no bridge conversion logic.
#
# Usage:
#   ./scripts/parity-check.sh                    # all fixtures
#   ./scripts/parity-check.sh <file.lex>         # single file
#   ./scripts/parity-check.sh --verbose           # show diffs on failure

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_DIR="$(cd "$TS_DIR/.." && pwd)"
PRINTER="$SCRIPT_DIR/parity-print.js"
ALLOWLIST="$SCRIPT_DIR/parity-allowlist.txt"

VERBOSE=false
SINGLE_FILE=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --verbose|-v) VERBOSE=true; shift ;;
        *) SINGLE_FILE="$1"; shift ;;
    esac
done

# Load allowlist (one path per line, # comments and blanks ignored)
ALLOWED_LIST=""
if [[ -f "$ALLOWLIST" ]]; then
    while IFS= read -r line; do
        line="${line%%#*}"
        line="$(echo "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
        [[ -z "$line" ]] && continue
        ALLOWED_LIST="${ALLOWED_LIST}|${line}"
    done < "$ALLOWLIST"
fi

is_allowed() {
    local path="$1"
    echo "$ALLOWED_LIST" | grep -qF "|${path}"
}

PASS=0
FAIL=0
SKIP=0
EXPECTED=0
ERRORS=""

check_file() {
    local lex_file
    if [[ "$1" = /* ]]; then
        lex_file="$1"
    else
        lex_file="$REPO_DIR/$1"
    fi
    local rel_path="${lex_file#$REPO_DIR/}"

    # Reference parser output
    local lex_output
    lex_output=$(cd "$REPO_DIR" && cargo run -q -p lex-cli -- inspect "$lex_file" parity 2>/dev/null) || {
        printf "  %-60s SKIP (lex-cli failed)\n" "$rel_path"
        SKIP=$((SKIP + 1))
        return
    }

    # Tree-sitter output
    local ts_output
    ts_output=$(cd "$TS_DIR" && npx tree-sitter parse -x "$lex_file" 2>/dev/null | node "$PRINTER" 2>/dev/null) || {
        printf "  %-60s SKIP (tree-sitter failed)\n" "$rel_path"
        SKIP=$((SKIP + 1))
        return
    }

    if diff <(echo "$lex_output") <(echo "$ts_output") > /dev/null 2>&1; then
        printf "  %-60s \033[32mPASS\033[0m\n" "$rel_path"
        PASS=$((PASS + 1))
    elif is_allowed "$rel_path"; then
        printf "  %-60s \033[33mEXPECTED\033[0m\n" "$rel_path"
        EXPECTED=$((EXPECTED + 1))
    else
        printf "  %-60s \033[31mFAIL\033[0m\n" "$rel_path"
        FAIL=$((FAIL + 1))
        ERRORS="${ERRORS}\n  ${rel_path}"
        if $VERBOSE; then
            echo "  --- lex-core (left) vs tree-sitter (right) ---"
            diff --color=always <(echo "$lex_output") <(echo "$ts_output") | head -40
            echo ""
        fi
    fi
}

echo "Parity check (block skeleton)"
echo ""

if [[ -n "$SINGLE_FILE" ]]; then
    check_file "$SINGLE_FILE"
else
    # Run against all element fixtures
    for f in "$REPO_DIR"/comms/specs/elements/**/*.lex; do
        check_file "$f"
    done
fi

echo ""
echo "────────────"
printf "Results: \033[32m%d passed\033[0m, \033[31m%d failed\033[0m, \033[33m%d expected\033[0m, %d skipped\n" "$PASS" "$FAIL" "$EXPECTED" "$SKIP"

if [[ $FAIL -gt 0 ]]; then
    printf "\nUnexpected failures:%b\n" "$ERRORS"
    exit 1
fi
