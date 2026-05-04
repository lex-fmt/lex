# Changelog

## [Unreleased]

### Added

- `lex-core`: `Range.origin_path: Option<Arc<PathBuf>>` field with `with_origin` builder and `origin()` accessor. Currently always `None` — pure additive scaffolding for the upcoming includes feature (PR 1 of 10). The field is `#[serde(skip)]` so existing AST JSON output is byte-identical. `Range` is now `#[non_exhaustive]`; equality and hashing ignore `origin_path` (positional only). See `comms/specs/proposals/includes.lex` for the full design.
- `lex-core`: `Annotation::is_include()` and `Annotation::include_src()` accessors plus `RESERVED_NAMESPACE_PREFIX` (`"lex."`) and `INCLUDE_LABEL` (`"lex.include"`) constants. The `lex.*` annotation label namespace is now reserved for core-defined semantics; the accessors hide the string-match on the reserved label and serve as a migration boundary if includes are later modeled as a distinct AST node type. Pure additive scaffolding for the includes feature (PR 2 of 10).
- `lex-core`: new `lex_core::lex::includes` module skeleton — `Loader` trait, `ResolveConfig`, `LoadError`, `IncludeError`, and a stub `resolve_includes` that returns its input unchanged. The trait/config/error surface is stable from this PR; splice logic, container-policy validation, recursion, cycle detection, and depth limiting land in PRs 4–6. lex-core's own code does not reference `std::fs`; loaders are injected. New `test-support` cargo feature exposes `MemoryLoader` so downstream crates' tests can exercise APIs that take a `Loader`. Pure additive scaffolding (PR 3 of 10).
- `lex-core`: include resolver now actually splices. `resolve_from_source(source, source_path, config, loader)` parses the entry-point file (without annotation attachment so includes are visible in container children), recursively walks every container looking for `lex.include` annotations, loads each target through the injected `Loader`, parses it independently, stamps `Range.origin_path` on every node from the loaded file, validates the splice list against the host container's policy (Sessions are rejected inside `Definition` / `Annotation` body / `ListItem`), and replaces the include annotation with the resolved content in-place. The included file's `DocumentTitle` becomes a leading `Paragraph` and document-level annotations become regular annotations — matching what a textual paste with indent-shift would produce. After all splices, annotation attachment runs once on the merged tree so the include annotation lands on the first spliced sibling per standard rules. Adds `IncludeError::MissingSrc`. **Not yet recursive**: `lex.include` annotations inside *included* files survive into the merged tree (PR 5 adds recursion + cycle/depth/root-escape safety; PR 6 adds per-file footnote resolution and file-ref `origin_path` consultation). PR 4 of 10.
- `lex-core`: `Annotation::include_src()` now returns `Option<String>` (was `Option<&str>`) and unquotes the parameter value. The previous return type left raw quotes on parsed sources, which broke any downstream that used the value as a path.

## [0.9.2] - 2026-05-02


### Changed

- Bumped `comms` submodule to v0.15.0, which adds the canonical Lex monochrome theme at `comms/shared/theming/lex-theme.json` (the cross-editor source of truth consumed by editor packages via `gen-theme.py`).
- Dependabot config aligned with the canonical `arthur-debert/release` portfolio policy: cargo freshness updates dropped (security coverage continues via Dependabot security updates); github-actions group retained. (#485)

### Fixed

- Release pipeline: corrected crate publish order so `lex-babel` is published before `lex-config` (matches the actual dependency graph). (#484)
## [0.9.1] - 2026-05-01


### Changed

- **Release pipeline migrated to canonical reusable workflow at
  `arthur-debert/release/.github/workflows/rust-cli.yml@v1`.** lex's
  `.github/workflows/release.yml` is now a thin caller. Sixth and final
  consumer of the new pipeline (after dodot v2.0.0, padz v1.8.2,
  simple-gal v0.20.4, rustloc v0.14.2, burgertocow v0.3.1 — all verified
  end-to-end). Bug fixes propagate via a single bump of the action's
  `@v1` ref instead of hand-edits across 6 rust-CLIs.
- **Tarball naming + layout changed to canonical** (full target triples +
  subdir layout). Brew formula handles both layouts.
- **Intel-mac dropped from release artifacts** (`x86_64-apple-darwin`)
  for both `lexd` and `lexd-lsp`. arm64-only macOS by canonical convention.
  v0.9.0 and earlier remain available for Intel users via direct GH
  release download.
- **CHANGELOG section headers migrated to Keep-a-Changelog canonical
  bracketed form** (`## [Unreleased]`, `## [0.9.0] - DATE` instead of
  `## Unreleased`, `## 0.9.0 - DATE`). The action's prepare-release
  expects the bracketed form. Existing version section bodies are
  untouched.
## [0.9.0] - 2026-04-28

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

## [0.8.0]

### Breaking

- Rename CLI binary and package: `lex-cli` -> `lexd` (binary: `lex` -> `lexd`)
- Rename LSP binary and package: `lex-lsp` -> `lexd-lsp` (binary: `lex-lsp` -> `lexd-lsp`)
- Installation is now `cargo install lexd` and `cargo install lexd-lsp`
- Release artifacts renamed: `lexd-{target}.tar.gz`, `lexd-lsp-{target}.tar.gz`
- `lexd` is now published to crates.io (previously `lex-cli` was not published)

The rename avoids conflicts with the Unix `lex` lexical analyzer generator, which shadows the binary on many systems. Internal library crates (`lex-core`, `lex-babel`, `lex-config`, `lex-analysis`) are unchanged.
