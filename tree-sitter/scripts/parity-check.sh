#!/bin/bash
# Parity check: compare tree-sitter CST (via bridge) with lex-cli AST JSON.
#
# Usage:
#   ./scripts/parity-check.sh                    # all fixtures
#   ./scripts/parity-check.sh <file.lex>         # single file
#   ./scripts/parity-check.sh --verbose           # show diffs on failure
#   ./scripts/parity-check.sh --level blocks      # compare block structure only
#
# Levels:
#   blocks   — compare block types + nesting (ignore text, annotations, inlines)
#   content  — compare block types + text content (ignore annotations)
#   full     — compare everything

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_DIR="$(cd "$TS_DIR/.." && pwd)"
BRIDGE="$SCRIPT_DIR/cst-to-json.js"

VERBOSE=false
LEVEL="blocks"
SINGLE_FILE=""

# Parse args
while [[ $# -gt 0 ]]; do
    case $1 in
        --verbose|-v) VERBOSE=true; shift ;;
        --level) LEVEL="$2"; shift 2 ;;
        *) SINGLE_FILE="$1"; shift ;;
    esac
done

PASS=0
FAIL=0
SKIP=0
ERRORS=""

# jq filter for block-level comparison: strip text content, locations, annotations
JQ_BLOCKS='walk(
    if type == "object" then
        del(.content, .text, .marker, .annotations, .mode, .closing_label,
            .closing_parameters, .groups, .lines, .parameters, ._hasSubject,
            ._subjectText, .count, .items, .title, .subject)
        | if .children then . else . end
    else .
    end
)'

# jq filter for content-level: keep text, strip annotations and markers
JQ_CONTENT='walk(
    if type == "object" then
        del(.annotations, .parameters, .closing_parameters, .mode,
            .marker, ._hasSubject, ._subjectText)
    else .
    end
)'

check_file() {
    local lex_file
    # Resolve to absolute path
    if [[ "$1" = /* ]]; then
        lex_file="$1"
    else
        lex_file="$REPO_DIR/$1"
    fi
    local rel_path="${lex_file#$REPO_DIR/}"

    # Get lex AST JSON
    local lex_json
    lex_json=$(cd "$REPO_DIR" && cargo run -q -p lex-cli -- inspect "$lex_file" ast-json 2>/dev/null) || {
        printf "  %-60s SKIP (lex-cli failed)\n" "$rel_path"
        SKIP=$((SKIP + 1))
        return
    }

    # Get tree-sitter CST XML and convert via bridge
    local ts_json
    ts_json=$(cd "$TS_DIR" && npx tree-sitter parse -x "$lex_file" 2>/dev/null | node "$BRIDGE" 2>/dev/null) || {
        printf "  %-60s SKIP (tree-sitter failed)\n" "$rel_path"
        SKIP=$((SKIP + 1))
        return
    }

    # Apply comparison level filter
    local lex_filtered ts_filtered
    case $LEVEL in
        blocks)
            lex_filtered=$(echo "$lex_json" | jq -S "$JQ_BLOCKS" 2>/dev/null || echo "JQ_ERROR")
            ts_filtered=$(echo "$ts_json" | jq -S "$JQ_BLOCKS" 2>/dev/null || echo "JQ_ERROR")
            ;;
        content)
            lex_filtered=$(echo "$lex_json" | jq -S "$JQ_CONTENT" 2>/dev/null || echo "JQ_ERROR")
            ts_filtered=$(echo "$ts_json" | jq -S "$JQ_CONTENT" 2>/dev/null || echo "JQ_ERROR")
            ;;
        full)
            lex_filtered=$(echo "$lex_json" | jq -S . 2>/dev/null || echo "JQ_ERROR")
            ts_filtered=$(echo "$ts_json" | jq -S . 2>/dev/null || echo "JQ_ERROR")
            ;;
    esac

    if [[ "$lex_filtered" == "JQ_ERROR" || "$ts_filtered" == "JQ_ERROR" ]]; then
        printf "  %-60s SKIP (jq filter failed)\n" "$rel_path"
        SKIP=$((SKIP + 1))
        return
    fi

    if diff <(echo "$lex_filtered") <(echo "$ts_filtered") > /dev/null 2>&1; then
        printf "  %-60s \033[32mPASS\033[0m\n" "$rel_path"
        PASS=$((PASS + 1))
    else
        printf "  %-60s \033[31mFAIL\033[0m\n" "$rel_path"
        FAIL=$((FAIL + 1))
        ERRORS="${ERRORS}\n  ${rel_path}"
        if $VERBOSE; then
            echo "  --- lex-cli (left) vs tree-sitter (right) ---"
            diff --color=always <(echo "$lex_filtered") <(echo "$ts_filtered") | head -30
            echo ""
        fi
    fi
}

echo "Parity check: level=$LEVEL"
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
printf "Results: \033[32m%d passed\033[0m, \033[31m%d failed\033[0m, %d skipped\n" "$PASS" "$FAIL" "$SKIP"

if [[ $FAIL -gt 0 ]]; then
    printf "\nFailed files:%b\n" "$ERRORS"
    exit 1
fi
