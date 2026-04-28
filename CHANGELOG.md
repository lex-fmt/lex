# Changelog

## Unreleased

### Changed

- **Releases now run end-to-end in CI via `scripts/release`.** Triggering a release with `scripts/release <version|major|minor|patch>` queues a `workflow_dispatch` run that performs the version bump (via `cargo set-version --workspace` — handles all 7 crate versions + 4 `[workspace.dependencies]` pins + `Cargo.lock` in one call), `## Unreleased` roll, commit, tag, GitHub Release, multi-platform build for both `lexd` and `lexd-lsp` (mac arm64+x86_64 signed+notarized, linux x86_64+arm64 gnu, linux x86_64 musl, windows x86_64), `.deb` attach for `lexd` on linux-gnu, crates.io publish for the 6 publishable crates in dep order, and Homebrew formula push for `lexd` to `arthur-debert/homebrew-tools` — all in CI. Replaces the previous local `cargo release` + tag-push trigger model. The legacy `scripts/release.sh` remains in the tree but is no longer the supported release path.
- **macOS arm64 + x86_64 binaries are now Developer ID signed and Apple-notarized** for both `lexd` and `lexd-lsp`. Previously adhoc/linker-signed only — meaning editor extensions (`lexed`, `vscode`, `nvim`) bundling `lexd-lsp` had to handle the unsigned inner binary themselves. Now both binaries can be re-bundled cleanly.

### Added

- **Homebrew installation via `arthur-debert/homebrew-tools` tap.** Install with `brew install arthur-debert/tools/lexd`. Installs `lexd` only — `lexd-lsp` continues to ship as tarball-only artifacts on the GitHub Release for editor extensions to consume.
- **`.deb` packages for Debian/Ubuntu (amd64 + arm64).** `apt install ./lexd_<version>_<arch>.deb` after downloading from the GitHub Release. Built by `cargo deb` in CI using the new `[package.metadata.deb]` block in `crates/lex-cli/Cargo.toml`.

### Fixed

- `lex-analysis`: the `missing-footnote` diagnostic no longer false-positives on numbered references inside a table cell when the resolving footnote list is the table's own positional list. The resolver now extends its in-scope footnote definitions with `table.footnotes` while walking a table's subject and cells, and restores the outer scope on exit. References outside a table still cannot reach table-local footnotes. The diagnostic message has been reworded from "no matching item in a `:: notes ::` list" to the scope-agnostic "no matching footnote definition in scope" to reflect that table-local definitions are also a valid resolution target.
- `lex-analysis`: the `table-inconsistent-columns` diagnostic no longer false-positives on rows whose column count is reduced by `^^` rowspan markers. Effective row width now accounts for cells carried over from previous rows via rowspan, not just colspans of the row's own cells.

### Added

- `lexd` now accepts piped input on stdin for `inspect`, `convert`, and `format` when the file path is omitted. Examples:
  - `cat foo.lex | lexd inspect ast-tag`
  - `cat foo.lex | lexd --from lex --to markdown`
  - `cat foo.lex | lexd format`

  `convert` requires `--from` when reading from stdin (there is no filename to auto-detect the source format from).

- `lex-core`: `Lexplore::footnotes(n)` loads footnote samples from `comms/specs/elements/footnotes.docs/`, mirroring the other per-element loaders.

### Tests

- Migrated all footnote-related tests off ad-hoc inline `.lex` strings: `lex-analysis` diagnostics and `collect_footnote_definitions`, `lexd-lsp` footnote reordering, and `lex-babel` HTML/IR table-footnote round-trips now load canonical samples from `footnotes.docs/` and `table.docs/` via `Lexplore`.
- Migrated `lex-analysis` table diagnostic tests (inconsistent columns, colspan/rowspan interactions) to the existing `table.docs/` samples — no new fixtures needed.

## 0.8.0

### Breaking

- Rename CLI binary and package: `lex-cli` -> `lexd` (binary: `lex` -> `lexd`)
- Rename LSP binary and package: `lex-lsp` -> `lexd-lsp` (binary: `lex-lsp` -> `lexd-lsp`)
- Installation is now `cargo install lexd` and `cargo install lexd-lsp`
- Release artifacts renamed: `lexd-{target}.tar.gz`, `lexd-lsp-{target}.tar.gz`
- `lexd` is now published to crates.io (previously `lex-cli` was not published)

The rename avoids conflicts with the Unix `lex` lexical analyzer generator, which shadows the binary on many systems. Internal library crates (`lex-core`, `lex-babel`, `lex-config`, `lex-analysis`) are unchanged.
