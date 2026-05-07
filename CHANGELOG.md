# Changelog

## [Unreleased]

### Fixed

- LSP `Position.column` is now reported in UTF-16 code units to match
  the LSP spec's default `positionEncoding`, instead of UTF-8 bytes.
  Two cursor walkers — `lex_core::lex::ast::inline_positions::position_at`
  (semantic tokens + document links) and
  `lex_analysis::inline::ReferenceWalker::position_at`
  (`find_references` + `goto_definition`) — were accumulating
  `column += ch.len_utf8()` for each char in the line. For any
  character wider than its UTF-16 representation (notably the `→`
  arrow at 3 UTF-8 bytes / 1 UTF-16 unit, and any non-BMP emoji at 4
  bytes / 2 units), every subsequent token's column was offset by the
  delta. In VSCode this surfaced as the open-backtick of a code span
  landing on the *next* character, painting the wrong glyph in the
  marker style and shifting the InlineCode content range one character
  right of where it should be. `find_references` and `goto_definition`
  jumps were similarly off when the line contained any non-ASCII
  character before the reference. Switched both walkers to
  `column += ch.len_utf16()`. Byte-level `Range::span` values are
  unchanged — they are and remain UTF-8 byte offsets, which is correct.

## [0.10.5] - 2026-05-07


### Fixed

- `lex_core::ast::Document::find_all_links` /
  `lex_core::ast::Session::find_all_links` now return `DocumentLink`
  ranges that cover only the `[bracketed]` reference, not the
  containing paragraph or title line. Editors render LSP
  `textDocument/documentLink` ranges as clickable underlined link
  surfaces; the previous code used `paragraph.range()` (with a comment
  acknowledging the limitation: "we don't have inline-level ranges
  yet") for URL and File reference types, so any paragraph containing
  a `[https://…]` or `[./path]` reference was rendered end-to-end as
  one giant link in VSCode. A new internal `ReferenceLocator` walks
  the inline tree with the same cursor / escape logic that
  `lex-analysis::semantic_tokens::InlineWalker` uses, producing
  precise byte and `Position` ranges for each URL/File reference.
  Verbatim `src` parameters retain their verbatim-block range
  (they aren't bracketed inline references). Existing extraction
  tests only asserted `target` + `link_type`, never `range`; new
  tests lock in the bracket-bounded invariant.
- `lex_core::ast::Session::find_all_links` now scans nested-session
  titles, not just the session it is invoked on.
  `Document::find_all_links` calls into the implicit root session
  (whose title is empty) and paragraph traversal yields paragraphs
  only, so URL/File references that appear in a section heading like
  `1. See [./handlers.lex] for details` were silently dropped from
  the LSP `documentLink` response — editors had no clickable surface
  on the heading even though the same reference inside a body
  paragraph worked. The fix walks `Session::iter_sessions_recursive`
  after the session's own title, so every heading at every depth
  contributes bracket-bounded link ranges.

### Changed

- Release pipeline consolidation: WASM/npm publishing is now part of
  the canonical `arthur-debert/release@v1.2.0` rust-cli workflow
  (opt-in via `wasm-package` input). Replaces the separate
  `release-wasm.yml` workflow that re-installed Rust and recompiled the
  workspace dep tree. One tag, one workflow run, one operator dashboard
  view — `crates.io`, GH release tarballs (incl. `lex-wasm-wasm.tar.gz`
  for direct-download consumers), Homebrew, and `npm` all ship from a
  single pipeline. (#510)
- `lex-lsp-core` is now part of the cargo publish list so its version
  ships to crates.io in lockstep with the rest of the workspace.
## [0.10.3] - 2026-05-06


### Added

- New `lex-lsp-core` crate consolidating the sync LSP feature surface
  (formatting, table navigation, available actions, document links,
  footnotes) shared by `lexd-lsp` (stdio) and `lex-wasm`. Eliminates
  hand-port drift between the two surfaces. (#506)
- `wasm32-unknown-unknown` build + `wasm-pack` artifact verification job
  in CI. Concretely closes the "spellbook on wasm32" risk in #465. (#508)
- npm publish workflow for `@lex-fmt/lex-wasm`, fired on `vX.Y.Z` tag
  push. The wasm package version is taken from `crates/lex-wasm/Cargo.toml`,
  so it stays in lockstep with the Rust crates. (#509)
- `lex_lsp_core::formatting::apply_edits(source, edits)` helper and
  routed the WASM `format()` binding through it. WASM no longer
  pattern-matches on the edit shape and is decoupled from the
  full-document-edit invariant. (#506 follow-up)

### Changed

- `lex-babel` workspace dep now uses `default-features = false`; `lex-cli`
  opts in to `native-export` (PDF/PNG via Pandoc) explicitly. The previous
  default pulled `which` (broken on wasm32 — incomplete `Sys` impl) into
  every `lex-babel` consumer including `lex-wasm`. (#508)
- `rust-toolchain.toml` declares `wasm32-unknown-unknown` as a target so
  rustup auto-installs it for the project-pinned 1.88.0 toolchain. (#508)
## [0.10.2] - 2026-05-05


### Fixed

- `lex-core::includes`: platform-absolute include `src` (Windows `C:\foo`, `\\server\share`, `\foo`) is now rejected up front in `resolve_path` with the new `IncludeError::AbsolutePath` variant instead of relying on `PathBuf::join`'s "absolute replaces base" semantics + the downstream root-escape check. The spec forbids absolute filesystem paths from entering the resolution pipeline; this holds the line at the input boundary and surfaces a clear "use relative or root-absolute" message instead of a misleading "escapes root" error. The root-absolute form (leading `/` against the includes root) is unchanged. Addresses item #4 from the security review. (#TBD)

### Added

- `lex-core::includes`: resource limits to bound resolver work against adversarial input. `ResolveConfig::max_total_includes` (default 1000) caps the total number of `lex.include` annotations resolved across the entire document — `max_depth` alone bounds chain length but a doc with thousands of includes at depth 1 still blows past it. `FsLoader::with_max_file_size(bytes)` (default 10 MiB) caps per-include file size; oversize files are rejected before any bytes hit memory. Both are surfaced as their own `IncludeError` variants (`TotalIncludesExceeded`, `FileTooLarge`) with `include_site` for editor diagnostics. Configurable via new `[includes].max_total_includes` and `[includes].max_file_size` keys in `lex.toml`. Addresses item #3 from the security review. (#503)

### Security

- `lex-core`: `FsLoader` now defends against arbitrary-file-read via symlink path traversal. Previously the resolver's lexical `..`-normalization correctly blocked textual root escapes, but a symlink inside the repository pointing outside the resolution root (e.g., `repo/sneaky -> /etc`) bypassed the check — `lexical_normalize` doesn't touch the filesystem, so it can't see through symlinks. `FsLoader` now stores its allowed root and, on every load, calls `fs::canonicalize` on both the requested path and the root, then verifies the canonical target sits under the canonical root. Symlinks pointing outside root are rejected as `IncludeError::RootEscape` before any read happens. Editors and CI that process untrusted Lex repositories should pick up this fix immediately. Surfaces a new `LoadError::OutsideRoot` variant; `Loader` trait now returns `LoadedFile { source, canonical_path }` instead of bare `String` so the resolver can use the loader's authoritative identity for cycle detection. (#502)

### Changed

- `lex-core::includes`: `Loader::load` now returns `LoadedFile { source, canonical_path }` instead of `String`. Implementations decide what `canonical_path` means — `FsLoader` returns the post-`fs::canonicalize` path (symlinks resolved, case-folded on case-insensitive FS); `MemoryLoader` returns the lookup key unchanged. The resolver uses `canonical_path` for cycle detection and origin stamping, so symlink loops and case-folded re-includes are now caught as `IncludeError::Cycle` rather than slipping through to `IncludeError::DepthExceeded`.
- `lex-core::includes`: `FsLoader::new` now takes the resolution root: `FsLoader::new(root: PathBuf)`. `Default` impl removed (a loader without a root would be unsafe).
- `lex-core::includes`: `FsLoader` now rejects non-regular files (FIFOs, sockets, devices, directories) before reading. Previously a malicious symlink to `/dev/zero` could block or OOM the reader once the symlink check landed; this is the second layer of defense.
## [0.10.1] - 2026-05-05

### Fixed

- `lex-lsp`: `include-not-found` diagnostic now points at the offending `:: lex.include src=… ::` annotation instead of falling back to the document head. Without this fix, vscode rendered the diagnostic as a zero-length point at line 1 col 1, giving no signal which include in a multi-include doc was broken. `IncludeError::NotFound` now carries `include_site: Range`; the resolver wires `annotation.location` through. The other no-site error variants (`RootEscape`, `LoaderIo`, `ParseFailed`) still fall back to head_range for now — same fix pattern applies but kept out of scope here. (#500)

## [0.10.0] - 2026-05-04


### Added

- `lex-core`: `Range.origin_path: Option<Arc<PathBuf>>` field with `with_origin` builder and `origin()` accessor. Currently always `None` — pure additive scaffolding for the upcoming includes feature (PR 1 of 10). The field is `#[serde(skip)]` so existing AST JSON output is byte-identical. `Range` is now `#[non_exhaustive]`; equality and hashing ignore `origin_path` (positional only). See `comms/specs/proposals/includes.lex` for the full design.
- `lex-core`: `Annotation::is_include()` and `Annotation::include_src()` accessors plus `RESERVED_NAMESPACE_PREFIX` (`"lex."`) and `INCLUDE_LABEL` (`"lex.include"`) constants. The `lex.*` annotation label namespace is now reserved for core-defined semantics; the accessors hide the string-match on the reserved label and serve as a migration boundary if includes are later modeled as a distinct AST node type. Pure additive scaffolding for the includes feature (PR 2 of 10).
- `lex-core`: new `lex_core::lex::includes` module skeleton — `Loader` trait, `ResolveConfig`, `LoadError`, `IncludeError`, and a stub `resolve_includes` that returns its input unchanged. The trait/config/error surface is stable from this PR; splice logic, container-policy validation, recursion, cycle detection, and depth limiting land in PRs 4–6. lex-core's own code does not reference `std::fs`; loaders are injected. New `test-support` cargo feature exposes `MemoryLoader` so downstream crates' tests can exercise APIs that take a `Loader`. Pure additive scaffolding (PR 3 of 10).
- `lex-core`: include resolver now actually splices. `resolve_from_source(source, source_path, config, loader)` parses the entry-point file (without annotation attachment so includes are visible in container children), recursively walks every container looking for `lex.include` annotations, loads each target through the injected `Loader`, parses it independently, stamps `Range.origin_path` on every node from the loaded file, validates the splice list against the host container's policy (Sessions are rejected inside `Definition` / `Annotation` body / `ListItem`), and replaces the include annotation with the resolved content in-place. The included file's `DocumentTitle` becomes a leading `Paragraph` and document-level annotations become regular annotations — matching what a textual paste with indent-shift would produce. After all splices, annotation attachment runs once on the merged tree so the include annotation lands on the first spliced sibling per standard rules. Adds `IncludeError::MissingSrc`. PR 4 of 10.
- `lex-core`: include resolver is now **recursive**. Each loaded file is fully resolved (its own `lex.include` annotations replaced) before being spliced into the host. Each level of recursion walks with that file's *own* directory as the host, so a relative path inside an included file resolves from the file's location — not the entry's. Cycle detection via an active-chain stack of *lexically normalized* absolute paths (a path already on the stack is a cycle; the resolver does not touch the filesystem, so symlink canonicalization is up to the loader). Depth limit defaults to 8 (`ResolveConfig::max_depth`), configurable per project. `IncludeError::Cycle` and `IncludeError::DepthExceeded` both carry the offending include site (`Range`) and the resolution chain at the moment of failure for diagnostics. PR 5 of 10.
- `lex-core`: origin-aware reference helpers complete the includes machinery. `lex_core::lex::includes::resolve_file_reference(target, ref_origin, root)` resolves a `ReferenceType::File` target the same way the include resolver resolves include paths — relative paths from the reference's authoring directory (`Range.origin_path`'s parent), root-absolute under `root`, with the same lexical-normalization + root-escape protection. `Document::find_annotation_by_label_in_origin(label, origin)` scopes footnote-style lookups to the file the reference was authored in, so a `[1]` in `chapter.lex` finds `:: 1 ::` defined in `chapter.lex` and not some other file's `:: 1 ::`. Unlike the existing `find_annotation_by_label`, the new walker checks both standalone annotations and *attached* `.annotations` slots on every node type that carries them (Session, Definition, ListItem, Paragraph, List, Table, VerbatimBlock) — necessary post-attachment. Both helpers are pure additive utilities; downstream wiring (CLI in PR 7, LSP in PR 8) will consume them. PR 6 of 10.
- `lex-core`: `Annotation::include_src()` now returns `Option<String>` (was `Option<&str>`) and unquotes the parameter value. The previous return type left raw quotes on parsed sources, which broke any downstream that used the value as a path.
- **`lex` includes are now live for users.** `lexd convert` and `lexd inspect` expand `:: lex.include src="..." ::` annotations by default, splicing the included file's content into the host tree before serializing. Pass `--no-includes` to operate on the unresolved tree (useful for inspecting a single document atom). `lexd format` never expands includes (per spec §11.4). (Note: the lex serializer's visitor does not currently emit *attached* annotations on Session/Definition/etc., which means a `:: lex.include ::` line that gets attached during parsing may not appear in formatter output verbatim — that's a pre-existing serializer limitation, separate from the includes feature, that will be addressed in a follow-up.) PR 7 of 10.
- **`lex-lsp` adds goto-definition + hover for `lex.include` annotations.** Click on `:: lex.include src="chapter.lex" ::` to jump to chapter.lex; hover to see a small preview (resolved path + first non-blank lines from the target). Path resolution uses the same `[includes].root` precedence as the resolver. Both features short-circuit cleanly: cursor not on an include falls through to the existing in-document goto/hover; untitled buffers and broken paths return None gracefully. PR 9 of 10.
- **`lex-lsp` runs include resolution on `did_open` / `did_change`** and surfaces include errors as diagnostics. Each `IncludeError` variant maps to a distinct diagnostic code (`include-cycle`, `include-depth-exceeded`, `include-root-escape`, `include-not-found`, `include-parse-failed`, `include-container-policy`, `include-loader-io`, `include-missing-src`); diagnostics that carry an `include_site` (cycle, depth, container policy, missing src) point at the offending annotation, others fall back to the document head. The LSP continues to store the *unresolved* host parse in its document store so position-based features (semantic tokens, hover, goto-definition, document symbols) keep using ranges in the host buffer's coordinate space — the merged tree is computed for diagnostic purposes only. Origin-aware position mapping (so cross-file goto / hover can land in the right buffer) lands in a follow-up. Fast path skips the resolver entirely when source contains no `lex.include` literal (avoids per-keystroke work and spurious `include-parse-failed` diagnostics on ordinary parse errors). Untitled URIs skip resolution silently. Editor packages need no changes — they pick this up via the next `lexd-lsp` pin bump. PR 8 of 10.
  - New `--includes-root <PATH>` global flag explicitly sets the resolution root. Default discovery: nearest `.lex.toml` walking upward from the entry file, falling back to the entry's own directory.
  - New `[includes]` config section with `root` (path) and `max_depth` (integer, default 8). CLI flags override config, config overrides defaults.
  - New `lex_core::lex::includes::FsLoader` is the production loader (filesystem-backed). Only `FsLoader` references `std::fs` — the rest of `lex-core` stays sandbox-clean.
  - Editor packages (`vscode`, `nvim`, `lexed`) pick up nothing yet — LSP integration lands in PR 8.

### Changed

- Bumped `comms` submodule to v0.16.0, which adds the canonical `specs/elements/lex.include.lex` element doc, the `specs/elements/lex.include.docs/` fixture set, and formally reserves the `lex.*` annotation label namespace in `specs/general.lex` §3.1. Also archives the includes proposal to `specs/proposals/done/includes.lex` per the new "frozen-when-implemented" convention.
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
