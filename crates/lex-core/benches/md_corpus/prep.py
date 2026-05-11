#!/usr/bin/env python3
"""
Prep step for `benches/parse_vs_markdown.rs`.

Two of the four fixtures (`040-on-parsing`, `080-gentle-introduction`)
do not have a hand-authored `.md` counterpart in `comms/specs/benchmark/`.
This script generates them via `lexd ... --to markdown`, which uses
the same `lex-babel` codepath that production lex tooling does.

Output goes under `crates/lex-core/benches/md_corpus/auto/` (gitignored).
Re-running the script overwrites; that's fine — the converter is
deterministic.

Prerequisites:
  - `lexd` on PATH (any v0.10+ release covers `--to markdown`)
  - `comms/` submodule initialised
"""
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
# HERE = .../crates/lex-core/benches/md_corpus
# repo root is four levels up
REPO_ROOT = HERE.parents[3]
COMMS_BENCH = REPO_ROOT / "comms/specs/benchmark"
OUT_DIR = HERE / "auto"

AUTO_CONVERTED = ["040-on-parsing", "080-gentle-introduction"]


def main() -> int:
    if not COMMS_BENCH.is_dir():
        print(
            f"error: {COMMS_BENCH} not found.\n"
            "Run `git submodule update --init` first.",
            file=sys.stderr,
        )
        return 1

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for name in AUTO_CONVERTED:
        src = COMMS_BENCH / f"{name}.lex"
        dst = OUT_DIR / f"{name}.md"
        if not src.is_file():
            print(f"error: missing source {src}", file=sys.stderr)
            return 1
        result = subprocess.run(
            ["lexd", str(src), "--to", "markdown"],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            print(f"lexd failed on {src}:\n{result.stderr}", file=sys.stderr)
            return 1
        dst.write_text(result.stdout)
        print(f"wrote {dst} ({len(result.stdout)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
