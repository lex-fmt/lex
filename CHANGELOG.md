<!-- generated - do not edit. See CHANGELOG/README.txt -->

# Changelog

## Unreleased

## 0.17.0 - 2026-06-03

### Added — smart paste: `lex/preparePaste` re-anchors pasted text to the caret's structural level ([#708](https://github.com/lex-fmt/lex/issues/708))

The language server now implements a custom `lex/preparePaste` request. On paste, lex-lsp classifies the clipboard (verbatim / table / single-line / re-anchor) and, for the re-anchor case, shifts the pasted block's indentation to match the structural container enclosing the caret — so copy-paste between and into Lex documents lands at the right structural level instead of the source's original indentation. Advertised via a capability flag so editors enable interception only against a server that implements it. Editor glue that calls the request ships separately, one PR per editor.

## 0.16.0 - 2026-06-02

### Fixed — whole-element reference-line anchors render on the default `lexd <file> --to <fmt>` path ([#722](https://github.com/lex-fmt/lex/issues/722))

Whole-element reference-line anchors (the `[#id]`-on-its-own-line form) were silently dropped on the default file-convert path, which routes through the include resolver even when a file has no includes. The resolver used a hand-rolled copy of the source→AST front-end that never ran the reference-line pre-pass, so the list collapsed into a self-linking paragraph. The two front-ends are now unified into a single `parse_to_attached_root` stage that both the standard pipeline and the include resolver call, so the pre-pass (and any future front-end stage) lives in exactly one place. Entry-file reference lines now anchor correctly; reference lines authored inside included files are not yet propagated through the wire codec (dropped rather than emitted with a wrong range — a documented follow-up).
### Removed — pre-1.0 legacy backward-compatibility scaffolding ([#727](https://github.com/lex-fmt/lex/issues/727))

Lex is pre-1.0 with no install base, so the compatibility shims that existed only to ease migration from older formats are removed wholesale. Four user- and API-facing surfaces go away:

- **`lexd migrate-labels` subcommand removed** ([#728](https://github.com/lex-fmt/lex/issues/728)). The standalone label-migration command and its `lex_core::lex::migrate` module (including `blessed_for_legacy`) are deleted. The LSP `forbidden-label-prefix` quickfix that previously rewrote a legacy label through the curated "blessed" table now simply strips the reserved `doc.` prefix, which covers the only case that arises in practice.
- **`lexd format` no longer migrates legacy session-based footnotes** ([#729](https://github.com/lex-fmt/lex/issues/729)). The dual-format path that detected child sessions titled `1. Note` and converted them to list-based footnotes during formatting is gone (`normalize_footnotes`, `convert_session_notes_to_list`, `split_numbered_title`). List-based footnote formatting is unchanged; the legacy session form was documented as "should not occur in practice".
- **Legacy detokenizer re-export shim removed** ([#730](https://github.com/lex-fmt/lex/issues/730)). The canonical detokenizer lives in `lex_core::lex::token::formatting`; the old `lex::formats::detokenizer` re-export that preserved historical import paths is deleted. Direct consumers import `{detokenize, ToLexString}` from `token::formatting`.
- **Parser entry points consolidated** ([#731](https://github.com/lex-fmt/lex/issues/731)). Internal naming around the parser front-end is unified; no behavior change.
### Changed — `process_full` collapsed into `parse_document`; `process_full_permissive` renamed to `parse_document_permissive` ([#737](https://github.com/lex-fmt/lex/pull/737))

`lex-core`'s parsing entry points carried two public names for the same pipeline call — `parse_document` (documented as a backward-compatibility alias) and `process_full` — plus a matching `process_full_permissive`. Pre-1.0 there is no compatibility to keep, so the public surface is now a single canonical pair: `parse_document` (the standard strict parse) and `parse_document_permissive` (the LSP/diagnostics variant that lets policy-violating `doc.*` / unknown `lex.*` labels flow into the AST instead of failing the parse). `process_full` and `process_full_permissive` are removed. Direct `lex-core` consumers calling the old names must rename; `parse_document` itself is unchanged, so the overwhelming majority of call sites are unaffected.

## 0.15.0 - 2026-06-01

### Added — extensions declare diagnostic codes; the host validates `[diagnostics.rules]` against them ([#659](https://github.com/lex-fmt/lex/issues/659))

Extension-emitted `[diagnostics.rules]` entries (`<namespace>.<code>`) are now schema-validated against the resolved registry, so a misspelled or undeclared code no longer silently retunes nothing.

- **Schema declares codes.** `lex_extension::schema::Schema` gains an optional `diagnostics` list — each entry carries `code`, an optional `description`, and a `default_severity` (defaulting to `warning`). The field is additive: schemas that omit it load with no declared codes, and built-in `lex.*` schemas declare none (they surface diagnostics through `lex-analysis`, not the extension code path).
- **Registry accessor.** `Registry::declared_diagnostic_codes(namespace)` aggregates and de-dupes the codes a namespace's schemas declare, returning `None` for an unregistered namespace so callers can distinguish "unknown namespace" from "known namespace, undeclared code".
- **Host validation.** `lex_fmt::validate_extension_diagnostic_rules` classifies each rule key: an unregistered namespace passes (rules may be staged ahead of installing the extension), a declared code passes, and an undeclared code under a registered namespace is reported as a dead letter with a closest-match "did you mean … ?" suggestion and the list of declared codes. The LSP runs this after registry boot and surfaces findings via `window/showMessage`.
### Fixed — paragraph lines split only by indentation no longer merge on format ([#699](https://github.com/lex-fmt/lex/issues/699))

A paragraph whose continuation lines were merely more-indented (alignment / hanging indent) was split into separate sibling paragraphs by the parser, then re-merged into one paragraph after formatting normalized the indent — a silent semantic change across a round-trip. Such hanging-indent continuations now fold back into the paragraph at parse time (real blank-line breaks are preserved).
### Changed — removed the open-form data marker; unrecognized `:: label` lines are kept as text ([#700](https://github.com/lex-fmt/lex/issues/700))

There was never a real "open form" of a data marker. A `:: label` with no closing `::` was classified as a distinct token that no grammar rule consumed, so the parser silently dropped such lines (and a definition whose sole body was one collapsed into a paragraph). Following Lex's rule that anything unrecognized becomes a paragraph — be forgiving, never lose content — these lines now classify as paragraph text.

- **New diagnostic.** An `unclosed-annotation` Warning flags a paragraph line shaped like `:: label …` with no closing `::`, so authors know it looks like metadata but is treated as content. Configurable via `[diagnostics.rules]`.
- `LineType::DataLine` and its classifier are removed. Closed-form `:: label ::` (annotations, verbatim closings) is unchanged.
### Fixed — table column alignment is read from the markdown separator row ([#702](https://github.com/lex-fmt/lex/issues/702))

The separator row's colon hints (`:---` left, `---:` right, `:---:` center) were detected only to be discarded; alignment was sourced solely from the `:: table align=… ::` parameter. Markdown-style aligned tables now keep their alignment across a format round-trip. The explicit `align=` parameter still overrides the separator row.
### Fixed — annotation parameters keep their comma separator on format ([#703](https://github.com/lex-fmt/lex/issues/703))

The Lex serializer joined annotation parameters with a space instead of a comma, so `:: warning type=critical, id=123 ::` re-serialized as `:: warning type=critical id=123 ::` and re-parsed to a single parameter. Parameters now re-emit comma-separated.
### Changed — extension diagnostic codes carry their namespace on the wire ([#657](https://github.com/lex-fmt/lex/issues/657))

Follow-up to #636: `[diagnostics.rules]` now accepts extension-emitted codes (`acme.task-due-date-missing`, `mit.plasma-specs.invalid-version`) alongside built-ins, configured identically.

- **Wire-format change (breaking).** `DiagnosticKind::Handler::code()` returns the namespace-prefixed form: a handler emitting `code: Some("foo")` under namespace `acme` produces wire `code = "acme.foo"` (previously bare `"foo"`). When the handler omits a code, the fallback is per-namespace `"acme.diagnostic"` rather than the old global literal `"handler.diagnostic"`. Any consumer matching on the bare wire `code` for an extension diagnostic must update to the dotted form.
- **`[diagnostics.rules]` extension keys.** `DiagnosticsRulesConfig` gains an `extra: BTreeMap<String, RuleConfig>` map. Any key under `[diagnostics.rules]` that doesn't match a built-in field flows into `extra` via `#[serde(flatten)]`; clapfig's strict-mode validator sees them as consumed (the same `#[serde(flatten)]` is pushed onto the confique-generated `Layer` field via `#[config(layer_attr(...))]`). Tradeoff: typo detection for *built-in* field names is sacrificed at this attribute level — a misspelled `missing_footote` now lands in `extra` instead of erroring. Schema-based validation (deferred, follow-up issue) will restore typo detection once extension schemas declare their codes.
- **`lookup_by_code` semantics.** Resolution order is named built-in field → `extra` map → `None`. Built-ins always win — a stray `extra` entry with a built-in code does not override the typed surface. `apply_rules` semantics are unchanged: `allow` drops the diagnostic, `warn` keeps intrinsic severity, `deny` upgrades to `Error`. Identical code path to built-ins; extension support is purely a lookup-table extension.
- **Lenient.** Any string key is accepted into `extra` without further validation. Entries that never match anything are harmless (ESLint / Clippy convention).

### Added — configurable diagnostic rules via `[diagnostics.rules]` ([#636](https://github.com/lex-fmt/lex/issues/636))

Closes the v1 loop on the diagnostic-configuration system. `.lex.toml` gains a `[diagnostics.rules]` block with one field per built-in diagnostic code, and the LSP server honours those rules when publishing diagnostics.

- **Configuration surface.** `DiagnosticsConfig` gains a `rules: DiagnosticsRulesConfig` nested struct. Each field carries its description as a doc comment and its intrinsic severity as the `#[config(default)]`. Schema-validation codes nest under `[diagnostics.rules.schema]`. `lexd config gen` emits the full annotated catalog automatically.
- **Severity verbs.** Each code accepts `"allow"` (suppress emission), `"warn"` (keep intrinsic LSP severity), or `"deny"` (upgrade to `Error`).
- **Code centralisation.** `DiagnosticKind::code()` and `SchemaValidationKind::code()` return the on-the-wire diagnostic code (formerly hard-coded inside `lex-lsp::to_lsp_diagnostic`). `DiagnosticsRulesConfig::lookup_by_code` resolves codes → rule entries.
- **Runtime wiring.** A new `apply_rules` function in `lex-analysis` filters and remaps diagnostics. The LSP server reads `[diagnostics.rules]` from `.lex.toml` and applies the registry to every analysis pass before publishing — editor squiggles honour the configuration immediately.
- **Drift test.** A test in `lex-analysis` iterates every built-in `DiagnosticKind` variant and asserts `lookup_by_code(kind.code())` returns `Some`, so adding a new diagnostic without a matching config field fails CI.
- **Breaking.** The previous `diagnostics.spellcheck = bool` knob is replaced by `[diagnostics.rules].spellcheck = "warn" | "allow" | "deny"`. Existing `.lex.toml` files using the boolean form fail strict-key validation and must migrate.
- **Out of scope for v1.** Extension-emitted codes (`<namespace>.<code>`) pass through untouched until the `extra` map surface lands. Per-document and per-region annotation overrides (v2 / v3 of #636) ship in follow-up work. CLI `lexd lint` rendering is its own work-stream.

### Added — `lex-extension-host::GitFetcher` real shell-out implementation ([#650](https://github.com/lex-fmt/lex/issues/650))

The git transport is no longer a stub. `GitFetcher::fetch` shells out to `git clone --depth=1` to populate the destination directory. Honors `uri.rev` as `--branch <ref>` (branch or tag) and `uri.subdir` to extract a subdirectory of the repo as the schema root. The `.git/` directory is stripped after clone — the cache only holds schema content.

Both registered schemes (`git:` and `git+ssh:`) route to this fetcher. URL forms accepted are whatever `git clone` accepts: `https://...git`, `git@host:owner/repo.git`, `file:///path/to/bare`, plus `git+ssh://...` (preserved verbatim — git treats it as a synonym for `ssh://`). Spec §3.3 / §6.3 cover the URL surface and the choice to shell out rather than embed libgit2.

No new dependencies — `std::process::Command` is the entire surface. Auth is whatever `git clone` would honor at the command line (SSH agent, OS keychain credential helpers, `gh auth setup-git`, `gitconfig`-declared SSO providers); there is no Lex-side credential knob. `GIT_TERMINAL_PROMPT=0` is set on the spawned process so a missing credential helper surfaces as a clean error rather than blocking the boot path. `git` must be in `PATH`; if it's missing the fetcher returns `FetchError::Other` with an actionable message pointing at the `path:` / `--ext-schema` escape hatches.

Git's stderr is classified into typed `FetchError` variants:

- `FetchError::Network` — connectivity failures (DNS, connection refused/timeout, unreachable).
- `FetchError::UpstreamStatus` — auth-shaped failures (permission denied, authentication failed, repository not found — the github/gitlab APIs use the last as a private-repo not-authorised signal too).
- `FetchError::Other` — everything else, carrying git's raw stderr verbatim (unknown ref, corrupted upstream, "not a git repository", etc.).

`is_immutable_rev` returns true for SHA-shaped refs (`^[0-9a-f]{7,40}$`) and tag-shaped refs (optional `v` prefix + `<digits>.<digits>` + optional suffix). The cache treats these as cacheable indefinitely; branch names and `None` are mutable and expire after the 24-hour TTL.

What this enables in `lex.toml`:

- `[labels.X] git = "git@internal.example.com:docs/lex-labels.git"` — private repos work end-to-end, inheriting the user's git credential setup.
- The `via = "git"` knob on `github:` / `gitlab:` URL templates — private-repo path for the forge shorthands.
- Self-hosted git over any transport git understands (HTTPS, SSH, git://, file://).

Spec §11.2 mirror/fallback URLs are intentionally out of scope.

### Added — `lex-extension-host::HttpsFetcher` real network implementation ([#649](https://github.com/lex-fmt/lex/issues/649))

The HTTPS transport is no longer a stub. `HttpsFetcher::fetch` performs a single HTTPS GET (sync, via `ureq` with rustls + webpki-roots), detects the archive format from `Content-Type` with URL-extension fallback, and extracts `tar.gz` or `zip` archives into the destination directory. Honors `uri.subdir` for archives that wrap content in a top-level directory (the GitHub tarball API does this).

Path-traversal defence at the extraction layer: archive members with `..` components or absolute paths are rejected; tarball symlink/hardlink entries and zip symlinks (detected via `S_IFLNK` in `unix_mode`) are skipped on both archive paths (schema directories are pure data, allowing archive-shipped symlinks would expand the trust surface). Response size capped at 256 MiB to defend against pathological servers, with a separate 64 KiB cap on 4xx/5xx error-response bodies so a hostile server can't OOM us on the diagnostic path either. Connect (30s) and read (120s) timeouts on the `ureq` agent so a stalled upstream can't hang the resolver indefinitely. Successful response bodies are streamed to a temp file rather than buffered into memory, so peak resident memory stays bounded even at the 256 MiB cap. `subdir` matching uses a component-windowed search, so nested paths (`subdir = "src/labels"`) work as well as single-component ones.

New deps: `ureq` (sync HTTP client, tokio-free), `flate2` + `tar` (gzipped tarballs), `zip` (zip archives). All gated behind the new `https-fetcher` cargo feature on `lex-extension-host` (default-on for `lex-cli`, `lex-lsp`, and `lex-fmt`, off for wasm builds where the underlying `ring`/`getrandom 0.2` chain doesn't compile to `wasm32-unknown-unknown`). With the feature off, `HttpsFetcher::fetch` returns `FetchError::Unimplemented`. All deps sit on the resolver path only, so consumers that don't use remote namespaces don't pay the cost at boot.

The `header` knob for `Authorization` / custom header pass-through (spec §6.2) is plumbing-ready in the fetcher but not yet exposed via `lex-config`; that's the follow-up tracked in #651.

### Changed — `lex-extension-host` resolver factored into transports + URL templates ([#648](https://github.com/lex-fmt/lex/pull/648))

Restructures the namespace resolver to match the new `extending-lex-stores.lex` companion spec. The previous model registered four peer fetchers (`GithubFetcher`, `GitlabFetcher`, `HttpsFetcher`, `GitSshFetcher`); the new model registers two transport fetchers (`HttpsFetcher`, `GitFetcher`) covering three schemes (`https`, `git`, `git+ssh`) and adds a URL-template layer (`github:`, `gitlab:`) that expands forge shorthands into transport URIs before dispatch.

Public-API changes (visible to direct `lex-extension-host` consumers):

- **Removed:** `resolve::fetcher::GithubFetcher`, `resolve::fetcher::GitlabFetcher`.
- **Renamed:** `resolve::fetcher::GitSshFetcher` → `resolve::fetcher::GitFetcher`. The renamed fetcher claims both `git:` and `git+ssh:` schemes.
- **Added:** `resolve::ResolveError::UnknownScheme` gains a `scheme: String` field that names the actual missing transport (after template expansion, if any) for clearer diagnostics.

No behaviour change for end users: every stub still returns `FetchError::Unimplemented`; per-transport network implementations are tracked at [#562](https://github.com/lex-fmt/lex/issues/562) (now rescoped from "implement four fetchers" to "implement two fetchers + two templates").
### Added — reference anchoring in HTML / Markdown serializers (references-general.lex §2.3)

The babel HTML and Markdown serializers now honour Lex's implicit reference anchors instead of always linking a bracketed reference to itself.

- **Inline word anchor (§2.3.1).** A link-like inline reference (`Url` / `File` / `Session` / `General`) wraps its anchored word — the preceding word by default, or the following word when the reference is first on the line — and the bracketed reference no longer renders as literal `[...]` text. `the project website [https://lex.ing] today` → HTML `the project <a href="https://lex.ing">website</a> today`, Markdown `the project [website](https://lex.ing) today`.
- **Whole-element anchor (§2.3.2).** A reference line targeting an element's head line wraps that head line in the link: session title (`<h2><a …>Title</a></h2>`), list item (`<li><a …>Water</a></li>`), definition term and verbatim subject (trailing colon excluded), and a plain paragraph line. The reference line itself emits no separate output.
- **Self-link (§2.3.2).** A reference line with no element directly above renders as a standalone link of its own text, spliced into the document at its source position.
- **Marker-style references unchanged (§2.3.4).** Footnotes `[1]`, citations `[@key]`, and annotation references `[::label]` keep their existing marker rendering and are never given a word or whole-element anchor.

Anchors are read from lex-core's authoritative resolution (`ReferenceInline.word_anchor` and `Document::reference_lines()`); the previous in-babel anchor heuristic (`common/links.rs`) is removed. IR `Verbatim` gains a `subject_href` field carrying the verbatim-subject link through to the serializers.


## [0.14.1] - 2026-05-17

### Added — `lex-config` diagnostic rule types ([#636](https://github.com/lex-fmt/lex/issues/636))

Foundation types for the diagnostic-configuration tracking issue. `lex-config` now exports `Severity` (`Allow` / `Warn` / `Deny`), `RuleConfig` (an untagged enum accepting either `"warn"` or `["warn", { … }]` on disk), and the `RuleOptions` alias (`BTreeMap<String, toml::Value>`). No consumers yet — the runtime consumption surface (`DiagnosticsRulesConfig` struct, registry, emission-site wiring) lands in follow-up PRs on the same issue. Public crate API addition, hence the Unreleased note.

## [0.14.0] - 2026-05-16

### Changed — four babel/CLI interop fixes ([#607](https://github.com/lex-fmt/lex/issues/607), [#608](https://github.com/lex-fmt/lex/issues/608), [#610](https://github.com/lex-fmt/lex/issues/610), [#611](https://github.com/lex-fmt/lex/issues/611))

Four more user-visible converter fixes, continuing the interop work-stream.

- **KaTeX CDN tags carry SRI hashes (#611).** The KaTeX 0.16.11 CSS, core JS, and auto-render JS tags emitted by the HTML serializer now include `integrity="sha384-…"` attributes; browsers reject any CDN payload whose hash doesn't match. Hashes were computed from the official release tarball at github.com/KaTeX/KaTeX/releases/tag/v0.16.11.
- **`.lex-verbatim` paged-media break-rule modernized (#607).** Every other paged-media element in `@media print` already pairs the legacy `page-break-*` property with its modern `break-*` companion; `.lex-verbatim` only had the legacy form. Chromium's headless paged-media now treats `page-break-inside` as deprecated, so without the modern companion code blocks could split across PDF pages.
- **Mojibake detection for `lexd convert` / `lexd format` (#608).** A new `lex-core::lex::mojibake::detect_mojibake` helper scans for the two-character signatures of a UTF-8 → cp1252 → UTF-8 round-trip (`Ã©`, `Ã¶`, `â€…`, etc.) and the CLI prints a one-shot stderr warning when ≥3 distinct patterns appear in the entry file or any `:: lex.include ::`-pulled file. The conversion still runs to completion — detection is informational. Suppress with the new `--no-warnings` global flag or `LEX_QUIET=1` env. Auto-correction is out of scope (re-encoding mojibake is lossy in edge cases).
- **Redundant `<div class="lex-content">` wrapper dropped inside `<section>` (#610, option 1).** Extends the `<dd>`-specific narrow fix (#604) to its broader sibling case: the wrapper is also pure DOM bloat when the immediate parent is a `<section>` (the section IS the content area). Baseline CSS gains a direct-child `margin-left` rule on the section's content children (`p, .lex-list, .lex-verbatim, .lex-verbatim-subject, .lex-definition, .lex-table, blockquote, figure, section.lex-session`) that replaces the wrapper's old padding. `margin-left` (not padding-left) so boxed children like `.lex-verbatim` keep moving as a whole instead of just nudging their inner text. The further targets the issue lists (`<li>`, `<blockquote>` parents) are deferred for a follow-up; each needs its own CSS-compensation pass and snapshot review. Snapshots updated: kitchensink + three trifecta fixtures. No theme changes required (neither `theme-fancy-serif.css` nor `theme-modern.css` references `.lex-content` or `section.lex-session`).

### Changed — three babel interop fixes ([#604](https://github.com/lex-fmt/lex/issues/604), [#605](https://github.com/lex-fmt/lex/issues/605), [#606](https://github.com/lex-fmt/lex/issues/606))

Three user-visible changes to the markdown / HTML converters, surfaced during the glossary-doc review (deferred from PR #609).

- **HTML `<dd>` body shape (#604).** `<dd>` no longer wraps its content in `<div class="lex-content"><p class="lex-paragraph">…</p></div>` — the dd is already the content container and the inner div added bytes without semantic value. Simple bodies render as `<dd><p>…</p></dd>` now. Baseline CSS gains compensating `.lex-definition dd > p` / `.lex-definition dd > .lex-list` rules so the rendered vertical rhythm and nested-content indentation are preserved.
- **Pandoc-flavored markdown definition lists (#605).** Definitions now serialize as `Term\n\n: details` (via Comrak's native `DescriptionList` nodes; `description_lists` extension enabled in both serializer and parser). The legacy `**Term**:` fallback is replaced. The markdown importer also recognizes Pandoc-style definition lists, so a `lex → markdown → lex` round-trip preserves the `<dl>` structure (regression test ships).
- **Numeric heading escape (#606).** Comrak's `## 1\. Glossary` (escape on a digit-dot prefix to disambiguate from an ordered-list marker) is post-processed away on heading lines — a `#`-prefixed line can't open a list, so the escape is just visual noise. Paragraph-leading `1\.` keeps Comrak's protection.

`regex` and `once_cell` move from dev-deps to main deps (the post-process in #606 uses them).

### Changed — symmetric IR for `document_annotations` ([#614](https://github.com/lex-fmt/lex/issues/614), Phase 3b of #570)

`Document::document_annotations` is now the single source of truth for document-scope annotations on the IR → Lex path. `to_lex_document` emits every entry into `lex_doc.annotations` via the `to_lex_annotation_raw` helper (shipped in Phase 3a, previously dead code), so a `lex → IR → lex` roundtrip preserves document metadata structurally.

The legacy `frontmatter` annotation event synthesis in `crates/lex-babel/src/common/nested_to_flat.rs` is retired. `tree_to_events` no longer inserts a packed `frontmatter` annotation event from `document_annotations` — format-specific serializers that need a YAML preamble read the IR slot directly. The markdown serializer (`crates/lex-babel/src/formats/markdown/serializer.rs`) was updated to synthesize the YAML block from `document_annotations` at output time, matching the previous flatten-keys-by-`lex.metadata.*`-prefix shape. The Markdown import path still produces a `frontmatter` annotation in `children[0]` and continues to round-trip through the existing markdown-side handling — final unification with the metadata-label whitelist happens in Sub D (#617).

This unblocks the rest of the interop architecture work-stream (umbrella #613): #615 / #616 / #617 build on the now-symmetric IR.
## [0.13.0] - 2026-05-14


### Added — `lexd check-labels` pre-flight validator ([#584](https://github.com/lex-fmt/lex/issues/584) PR 5 of 5)

Final PR of the bare-as-blessed label namespace model. PRs 1–4 added the form-tagging infrastructure, strict resolution, form-preserving emit, and the LSP-side label-policy surface. This PR ships the CLI equivalent for batch / CI use.

- **New subcommand `lexd check-labels <path>`** in `crates/lex-cli/src/main.rs`. Parses the file permissively (so `doc.*` and unknown `lex.*` labels flow through into the AST), runs the analysis pass, and reports any `forbidden-label-prefix` / `unknown-lex-canonical` diagnostics with `path:line:col: error[<code>]: <message>` formatting. Exits `0` on a clean file, `1` if any violations are found, `2` on I/O or fatal parse error.
- **Designed for CI use** — permissive parse means every violation surfaces in one invocation; you don't have to fix one and re-run to see the next.
- **Out of scope** — the originally-planned `lexd migrate-labels` UX rework was dropped (nothing in the wild needs migrating; the existing command already covers the rare cases). Per #584's reduced PR 5 scope: just the label-check pre-flight.

### Fixed — PR 589 review fixups ([#584](https://github.com/lex-fmt/lex/issues/584) follow-up)

Five issues caught in review of [#589](https://github.com/lex-fmt/lex/pull/589) — fixes shipped as a follow-up since PR 589 merged before the fixups landed.

- **Hover form-classification was a no-op.** `label_form_hover_line` re-classified `annotation.data.label.value`, but `NormalizeLabels` had already rewritten it to canonical — `classify_label` always returned `Canonical` form. As a result, the "Shortcut for `lex.metadata.author`" / "Prefix-stripped form" / "Community label" hover lines PR 4 advertised never actually fired. Fixed to consult `Label.form` directly (the parser-recorded source classification) and pair it with `label.value` as the canonical.
- **Walker double-walked attached annotations + child content.** `check_labels` had per-type walkers (`walk_annotation`/`walk_verbatim`/`walk_table`) that descended into a node's children, then `walk_item` *also* descended via `attached_annotations` + `item.children()` on the same node — duplicate diagnostics for any forbidden label nested inside another label-bearing site. Restructured into a single `walk_item` dispatcher that emits type-specific labels and defers to a uniform attached/children walk. New regression test `check_labels_emits_each_offending_site_exactly_once`.
- **`process_full_permissive` duplicated the standard pipeline.** Hand-rolled lexing → parsing → assembling so any future stage addition / reordering would have to be mirrored. Introduced `lex_core::lex::transforms::standard::run_string_to_ast(s, mode)` so strict (`STRING_TO_AST`) and permissive (`process_full_permissive`) share a single pipeline definition.
- **`doc.*` quickfix only fired for the 4 curated mappings.** `doc.foo` / `doc.random` produced a `forbidden-label-prefix` diagnostic with no code action. Added a generic "strip `doc.` prefix" fallback so every diagnostic has an attached quickfix.
- **Diagnostic message text duplicated `RejectReason::message()`.** Strict-mode parser errors and permissive-mode analysis diagnostics carried near-identical wording in two places that had to stay in sync. `check_labels` now delegates message construction to `RejectReason::message()` so the wording is literally identical across both surfaces.

### Added — label-policy diagnostics, hover info, quickfix ([#584](https://github.com/lex-fmt/lex/issues/584) PR 4 of 5)

Fourth of five PRs for the bare-as-blessed label namespace model. PRs 1–3 added the form-tagging infrastructure, strict resolution, and form-preserving emit. This PR wires the **user-facing surface** so editors can show what's going wrong and fix it.

- **New `process_full_permissive` parse entry point** in `crates/lex-core/src/lex/parsing.rs`. Mirrors `process_full` but runs `NormalizeLabels` in permissive mode so `doc.*` and unknown `lex.*` labels flow through into the AST instead of failing the parse. Intended for hosts (the LSP) that want to surface label-policy violations as in-place diagnostics rather than as wholesale parse failures.

- **LSP parse switched to permissive mode** (`crates/lex-lsp/src/server.rs::DocumentStore::upsert`). A `:: doc.table ::` in a file no longer blanks out every LSP feature (semantic tokens, hover, completion, goto-def) — the rest of the file keeps working and the offending label gets a diagnostic.

- **New `check_labels` analysis pass** (`crates/lex-analysis/src/diagnostics.rs`). Walks every label site (annotation, verbatim closer, table closer, nested annotation) and re-classifies via `classify_label`. Emits:
  - `DiagnosticKind::ForbiddenLabelPrefix` (Error, code `forbidden-label-prefix`) for `doc.*` labels.
  - `DiagnosticKind::UnknownLexCanonical` (Error, code `unknown-lex-canonical`) for `lex.*` literals not in the registered canonical set.

- **Hover shows form classification** (`crates/lex-analysis/src/hover.rs::annotation_hover_result`). For `Shortcut`-form labels: "Shortcut for `lex.metadata.author`". For `Stripped`: "Prefix-stripped form of `lex.metadata.author`". For `Community`: "Community label". `Canonical` form gets no extra line (already the natural reading).

- **Quickfix for `doc.*` labels** (`crates/lex-lsp-core/src/available_actions.rs`). When a `forbidden-label-prefix` diagnostic is on a known legacy spelling (`doc.table`, `doc.image`, `doc.video`, `doc.audio`), offers "Rewrite `doc.table` to `table`" as a `QUICKFIX`-kind code action. Reuses the legacy→blessed map newly exposed as `lex_core::lex::migrate::blessed_for_legacy`.

### Wire `on_resolve` for tabular + media ([#583](https://github.com/lex-fmt/lex/issues/583))

Closing follow-up to #570. The `lex.tabular.table` and `lex.media.{image,video,audio}` labels now go through `Registry::dispatch_resolve` at IR construction time rather than being pattern-matched on `DocNode` variants by the legacy `VerbatimRegistry` lookup. This is the load-bearing piece of the refactor: third-party namespaces can now intercept verbatim labels through the same code path the built-ins use.

**Wire format bumped to `wire_version: 2`.** Two shape changes; both are breaking for handlers that decoded the wire format directly.

- `WireNode::Table` carries per-column alignment via `column_aligns: Vec<String>` (was a single `align: String` summary in v1). Mixed-alignment tables — e.g. left-aligned first column, right-aligned numeric second column — now round-trip losslessly. `column_aligns.length` equals the widest row in the table.
- New typed `WireNode::{Image, Video, Audio}` variants for media. Resolve dispatch for `lex.media.*` produces these directly instead of a generic `Verbatim` node with reconstructed params.

`lex-extension v0.13.0`'s `WIRE_VERSION` const ticks to 2. The reusable subprocess fixture handler picks up the bump automatically (it forwards `lex_extension::WIRE_VERSION`).

**Registry parameter on `from_lex_document`.** `lex-babel::ir::from_lex::from_lex_document` and the recursive helpers it dispatches through (`convert_children`, `from_lex_session`, `from_lex_list`, `from_lex_definition`, `from_lex_annotation`, `from_lex_table`, `from_lex_verbatim`, …) now take `registry: &Registry`. The public `lex_babel::to_ir(doc)` still works — it delegates to the new `to_ir_with_registry(doc, registry)` using a process-wide `default_registry()` populated with the built-in `lex.*` schemas. Callers that boot a custom registry (`lex-cli`, `lex-lsp`, embedders) plumb theirs through `to_ir_with_registry`. Same `default_registry()` is reused by `to_lex_document` so verbatim labels round-trip through `Registry::dispatch_format` regardless of which direction kicks the conversion off.

`Schema::hooks.resolve` is now `true` on `lex.tabular.table` and the three `lex.media.*` schemas. The legacy `parse_pipe_table` helper in `lex-babel/src/common/verbatim/table.rs` is gone — the canonical parser is `lex_core::lex::builtins::tabular::parse_pipe_table_to_wire`, which emits a typed `WireNode::Table`. The reverse path (`from_wire_node`) decodes the typed media wire kinds back into `ContentItem::Verbatim` with reconstructed params so lex-core's untyped AST shape is preserved.

### Added — form-preserving emit ([#584](https://github.com/lex-fmt/lex/issues/584) PR 3 of 5)

Third of five PRs for the bare-as-blessed label namespace model. PRs 1 + 2 added `LabelForm` and tagged every label site at parse time; this PR wires that tag through `LexSerializer` so emit preserves the user's source spelling.

- New `source_spelling(&Label) -> String` and `shortcut_for_canonical(&str) -> Option<&'static str>` in `crates/lex-core/src/lex/assembling/stages/normalize_labels.rs`. Pure functions; reverse-lookup `SHORTCUT_TABLE` for Shortcut-tagged labels, strip the `lex.` prefix for Stripped-tagged labels, return `value` verbatim for Canonical / Community.
- `LexSerializer::visit_annotation` and `LexSerializer::leave_verbatim_block` now call `source_spelling` instead of emitting `label.value` directly. Tables inherit the same behavior because the `:: table ::` closer is itself an annotation walked through `visit_annotation`.
- Roundtrip contract from `comms/specs/general.lex` §4.3 is now live: `:: author ::` → AST canonical `lex.metadata.author` (form=Shortcut) → emit `:: author ::`. Same for `metadata.author` (Stripped), `lex.metadata.author` (Canonical), `acme.task` (Community).
- New round-trip tests in `formats/lex/serializer.rs` for all four forms, plus a verbatim closer test (`image src=…`). `test_verbatim_04_user_repro` was updated to expect the shortcut closer (`:: table ::`) instead of the canonical (`:: lex.tabular.table ::`).
- New unit tests in `normalize_labels.rs` covering `source_spelling` for each form variant and `shortcut_for_canonical` for both hit + miss.

### Changed — strict `NormalizeLabels` + structural-Table emit ([#584](https://github.com/lex-fmt/lex/issues/584) PR 2 of 5)

Second of five PRs for the bare-as-blessed label namespace model. PR 1 added the form-tagging infrastructure; this PR replaces `NormalizeLabels`'s legacy whitelist with the resolution rules from `comms/specs/general.lex` §4.2 and rejects forbidden forms at parse time.

**Resolution rules:**

- Shortcut table → `LabelForm::Shortcut`. The 10 normative shortcuts are `table`, `image`, `video`, `audio`, `author`, `title`, `tags`, `date`, `include`, `notes`.
- `lex.*` literal (registered canonical) → `LabelForm::Canonical`.
- Prefix-strip (`metadata.author` → `lex.metadata.author`) when `lex.<input>` exists in the canonical set → `LabelForm::Stripped`.
- Dotted non-reserved (`acme.task`) → `LabelForm::Community`; registry validation deferred to analysis.
- Bare unknown (`foobar`, `42`, `^name`, `spec2025`) → `LabelForm::Community`; PR 4 of #584 adds a typo-prevention lint in analysis. The parser is deliberately permissive here so document-scoped reference identifiers (footnote IDs, citation keys, language hints on verbatim closers) parse without each needing a carve-out.

**Hard rejections (TransformError at parse time):**

- `doc.*` — reserved-forbidden under §4.1.
- `lex.*` literals that aren't in the registered canonical set.

**New strict / permissive modes** — `NormalizeLabels::new()` is strict (used by `STRING_TO_AST`); `NormalizeLabels::permissive()` skips rejection (used by `lexd migrate-labels`'s `parse_permissive` so legacy `doc.*` source can be parsed and rewritten).

**New canonical: `lex.notes`** — registered alongside `lex.include` / `lex.metadata.*` / `lex.tabular.*` / `lex.media.*`. The label is the canonical footnote-definition-list marker. Promotion to a core canonical (rather than `metadata.notes` via prefix-strip) lets the source-level form stay `:: notes ::` and aligns with the spec's blessed-shortcut tier.

**New `CANONICAL_LABELS` slice in `crates/lex-core/src/lex/builtins/mod.rs`** — single source of truth for which `lex.*` labels exist. `register_into` and `NormalizeLabels`'s `classify_label` both consume it; a parity test in `builtins::tests` enforces that the slice stays in sync with `register_into`'s schema set.

**Structural-Table emit in `LexSerializer`** — `visit_table` / `leave_table` now emit a markdown-style pipe table with per-column alignment, padded for width. Previously `LexSerializer` had no Table visitor and tables resulting from the bare `:: table ::` closer serialized to an empty `:: lex.tabular.table ::` block.

**Migration tool refactor** — `lex-core::migrate` now uses `parse_permissive` instead of `STRING_TO_AST` so legacy `doc.*` source can still be parsed for rewriting. The legacy-label table moved into `migrate.rs` (now scoped to migration use only; `NormalizeLabels` doesn't carry a "legacy" concept). `doc.table` → `table`, `doc.image` → `image`, `category` → `metadata.category`, etc.

**Production callers flipped off legacy forms:**

- `crates/lex-babel/src/templates/asset.rs` — `AssetKind::label()` emits `image`/`video`/`audio` (blessed shortcuts); `Data` falls back to `asset.data` (community-shape; no canonical for generic data assets today).
- `crates/lex-analysis/src/completion.rs` — `STANDARD_VERBATIM_LABELS` list shrunk to blessed shortcuts only.
- `crates/lex-analysis/src/utils.rs::is_notes_list` — accepts both `notes` (shortcut) and `lex.notes` (canonical) since callers may hand-build ASTs.
- `crates/lex-core/src/lex/assembling/stages/apply_table_config.rs` — the `:: table ::` config annotation lookup accepts both spellings.

**Test fixture migrations:**

- Bare `note` / `info` / `warning` (test-only stand-ins) replaced with `test.note` / `test.info` / `test.warning` (community-shape) in ~20 files where the test was exercising parser/LSP plumbing, not label semantics.
- `doc.note` / `doc.data` in test fixtures replaced with `test.note` / `test.data`.
- Tests asserting specific label spellings (`closing_label("image")`) updated to expect the canonical (`closing_label("lex.media.image")`) since `NormalizeLabels` resolves at parse time.
- The `verbatim_03_table_formatting` and `verbatim_04_user_repro` tests in `formats/lex/serializer.rs` now exercise the structural-Table emit path (no longer the legacy verbatim-with-markdown reformatter).
- Snapshot fixtures (kitchensink markdown + html, detokenizer outputs) regenerated to match the new output.

**Comms-side sibling**: `lex-fmt/comms` PR 43 adds `notes` to §4.2's normative shortcut table and flips three benchmark fixtures off `doc.*`.

### Deferred follow-up

The legacy verbatim-markdown reformatter code (`common/verbatim/table.rs::TableHandler` + `parse_pipe_table`, `VerbatimRegistry`'s `format_content` path, `LexSerializer::verbatim_registry` field) remains in the tree but is unreachable from user input now that:

1. `doc.table` is hard-rejected, so no source-level path triggers the reformatter through NormalizeLabels.
2. `:: table ::` parses as a structural Table and is serialized by `visit_table`.
3. `lex.tabular.table` and `tabular.table` source also parse as Verbatim today, but no tests exercise that path; PR 3 of #584 retires it.

A tidy-up PR after PR 3 can delete the dead modules and the `verbatim_registry` field on `LexSerializer`.

### Added — `LabelForm` infrastructure ([#584](https://github.com/lex-fmt/lex/issues/584) PR 1 of 5)

First of five PRs implementing the bare-as-blessed label namespace model spelled out in `comms/specs/general.lex` §4. This PR is the lex-core foundation: it adds the `LabelForm` enum (`Canonical | Stripped | Shortcut | Community`) and a `form: LabelForm` field on `Label`. `NormalizeLabels` now tags every rewrite with the matching form so downstream formatters can preserve the user's choice of spelling on roundtrip. No emission behavior changes yet — formatters still emit `label.value` verbatim; PR 3 wires `form` through.

- New `comms` submodule pin: includes `specs/general.lex` §4 (Label Namespaces) and the bare-form flip in `specs/elements/lex.include.lex`.
- `LEGACY_TO_CANONICAL` table grew a third tuple element: `(legacy, canonical, form)`. The four new-shortcut entries (`title`, `author`, `date`, `tags`) tag as `Shortcut`; the four non-shortcut metadata entries (`category`, `template`, `publishing-date`, `front-matter`) and the four `doc.*` entries tag as `Stripped`. Today these classifications are forward-looking; PR 2 of #584 hard-rejects `doc.*` and bare non-shortcut names, and PR 3 wires the form into the formatter.

### Refactored — label semantics ([#570](https://github.com/lex-fmt/lex/issues/570))

Multi-phase refactor moving label-semantic decisions out of the IR layer and through the extension registry. Eight PRs (#575–#581) landed via the `refac/label` integration branch, followed by a legacy-code cleanup pass.

**New built-in `lex.*` schemas** registered alongside `lex.include`:

- `lex.metadata.{title, author, date, tags, category, template, publishing-date, front-matter}`
- `lex.tabular.table`
- `lex.media.{image, video, audio}`

**New parse-time pass** (`NormalizeLabels`) rewrites bare labels to their canonical `lex.*` form during `STRING_TO_AST`. Source `:: title ::` becomes `:: lex.metadata.title ::` in the AST; source `:: doc.table ::` (verbatim) becomes `:: lex.tabular.table ::`.

**New `on_format` reverse hook** in the extension wire spec (`lex-extension-wire.lex` §4.8) and the `LexHandler` trait. Given a typed AST subtree, a handler returns a `LexAnnotationOut` describing the label, parameters, body, and verbatim flag the host emits as Lex source. `Registry::dispatch_format` is the entry point. The four built-in verbatim handlers implement it.

**`to_lex.rs` now dispatches through `Registry::dispatch_format`** for `lex.tabular.table` and `lex.media.{image,video,audio}` instead of pattern-matching on `DocNode` variants through the local `VerbatimRegistry`. Output now carries canonical labels (`:: lex.tabular.table ::`, not `:: doc.table ::`).

**`document_annotations` field on `DocNode::Document`** is the source of truth for document-scope metadata. `nested_to_flat` synthesizes the `frontmatter` event from it at emission time — the IR no longer carries a synthetic `frontmatter` annotation in children.

**New `lexd migrate-labels <path>` subcommand** for source-level migration. Default mode prints the rewritten source to stdout; `--in-place` overwrites; `--check` exits non-zero if any migrations are pending.

#### Breaking changes (pre-release, documented)

- **Bare labels in source are silently rewritten at parse time.** This is observable in `lexd format` output: a file containing `:: title :: My Doc` will format to `:: lex.metadata.title :: My Doc`. Use `lexd migrate-labels` for explicit batch migration.
- **`doc.table`, `doc.image`, `doc.video`, `doc.audio` verbatim labels in `to_lex` output are now canonical:** `lex.tabular.table` / `lex.media.{image,video,audio}`.
- **The `VerbatimRegistry::default_with_standard()` registry no longer carries `doc.*` legacy aliases.** Embedders hand-building IR `Verbatim` nodes with the legacy labels and feeding them through `from_lex_verbatim` will hit a `None` lookup. Use the canonical names.
- **The `VerbatimHandler` trait shrunk:** `to_ir` and `convert_from_ir` methods removed. `label()` and `format_content()` remain. The IR-construction path now goes through `from_lex_verbatim` calling free helpers directly (`table::parse_pipe_table`, `media::image_from_params`, etc.); the IR→Lex path goes through `Registry::dispatch_format`.
- **Inline `:: lex.metadata.title :: ...` in the document body is no longer promoted to document metadata.** Inline annotations stay inline. Document-scope metadata must be attached at the document level (the lex-core `Document.annotations` slot, i.e. annotations at the very top of the source before any content). This was a behavioural quirk of the legacy whitelist.
- **`lex-babel::ir::nodes::Document` gained a `document_annotations: Vec<Annotation>` field.** Code constructing `Document` via struct-literal syntax must add `document_annotations: vec![]` (or populate as needed).
- **`lex-babel::common::nested_to_flat`'s legacy bare-label metadata whitelist (`["author", "note", "title", ...]`) is gone.** It synthesized `lex-metadata:<label>` verbatim events; after the refactor, document metadata flows through the `frontmatter` annotation event synthesized at the document boundary instead.
- **`lex-extension` trait `LexHandler` gained a default-impl `on_format` method.** Existing impls compile unchanged; new method is non-breaking per the wire-spec versioning policy.
## [0.12.0] - 2026-05-12


### Added

- **HTML render splice** ([#563](https://github.com/lex-fmt/lex/issues/563)).
  `lexd convert --to html` (via
  `lex_babel::serialize_to_html_with_registry`) now actually splices
  handler-rendered HTML into the output for annotations whose
  registered handler returns `RenderOut::String`. The default
  `<!-- lex:label -->` ... body ... `<!-- /lex:label -->` rendering
  is replaced by the handler's raw HTML; the annotation's body
  events are suppressed (the handler owns the full rendering of its
  labelled node). Mechanism: AST-walk and event-walk visit
  annotations in matching document order; the HTML builder
  maintains a counter, looks up the matching plan entry on
  `Event::StartAnnotation`, and emits a sentinel comment that
  string-replaces with the handler's HTML after DOM serialization.
  Handler diagnostics from `on_render` continue to surface via
  `HtmlExportOutcome::diagnostics`.
  - `RenderOut::WireAst` outputs fall through to default rendering
    with the existing format-shape-mismatch diagnostic;
    `WireAst → HTML` conversion is a follow-up.
  - Document-level annotations (extracted into the IR's synthetic
    `frontmatter` block before events are emitted) are not splice
    targets — the existing frontmatter path handles them as
    metadata. Per-element annotations (the actual extension use
    case) splice correctly.
  - Multi-annotation documents with trailing annotations may hit
    an IR-ordering quirk that breaks the counter-based alignment.
    Single-annotation documents and clearly-separated annotations
    work; see test
    `multi_annotation_splice_is_a_known_limitation` for context.

- `lexd labels emit <doc> [--label X]... [--namespace N]...`
  ([#564](https://github.com/lex-fmt/lex/issues/564)). Pull-based
  NDJSON export — one record per labelled annotation / verbatim in
  the document. Output uses the wire `Position` / `Range` types so
  the same parser that consumes LSP hover and extension hook
  payloads consumes emit output unchanged. Body shapes are tagged:
  `{kind: "none"}`, `{kind: "text", text: "…"}`, or `{kind: "lex",
  wire: [...]}`. `--label` and `--namespace` are repeatable and
  intersect. No registry boot required — `to_wire_node` produces
  the wire form without schema lookup, so this command runs against
  documents whose namespaces aren't registered. Exit 0 on success
  (including zero matches), 2 on parse failure.

- **Resolver machinery** (#546 item A, partial). `lex-extension-host`
  gains a pluggable [`Fetcher`] trait, a `FetcherRegistry`, and a
  content-keyed `ResolverCache` (24-hour TTL for mutable refs,
  indefinite for immutable). The four remote schemes
  (`github:`/`gitlab:`/`https:`/`git+ssh:`) ship as stub `Fetcher`
  impls that return `FetchError::Unimplemented` — same observable
  behaviour as before, but plugged into the new dispatch so an
  implementer's PR per [lex#562](https://github.com/lex-fmt/lex/issues/562)
  only needs to swap the stub for a real fetcher. `path:` stays
  built-in (no cache, no fetcher) — it's special-cased in
  `resolve_namespace_with` before the registry is consulted.
- `resolve_namespace_with(uri, workspace_root, &registry, &cache)`
  for callers that want explicit control over the registry + cache
  (hosts constructing one cache + registry at boot time; tests
  using tempdir caches and custom fetchers). The existing
  `resolve_namespace(uri, workspace_root)` is now a convenience
  wrapper using `default_fetcher_registry()` and
  `ResolverCache::user_default()` (XDG-cache-home aware).
- End-to-end validation: `tests/resolver_http_e2e.rs` exercises the
  full pipeline (URI parse → registry dispatch → cache miss →
  fetch → cache hit on second resolve) with a hand-rolled HTTP
  `Fetcher` against a `std::net::TcpListener` mock server. Proves
  the machinery works for a non-stub fetcher without depending on
  any external network.
- `sha2` workspace dependency, used by `ResolverCache` for the
  content-key hash function (`hash(scheme + body + rev + subdir)`
  → 64-char hex directory name under the cache root).

- **Extract-to-include LSP command**
  ([#497](https://github.com/lex-fmt/lex/issues/497)).
  `lexd-lsp` registers a new workspace command `lex.extractToInclude`
  that splits a selected slice of a Lex document out into a new file
  referenced via `:: lex.include src="…" ::`. The command takes
  positional arguments `[uri, range, src]`, validates the target
  path against the configured includes root (returns distinct
  `ExtractError` variants for empty / URL / absolute / root-escape /
  existing-target / missing-parent-dir cases), indent-shifts the
  selection so its shallowest non-blank line lands at column 0,
  parses the shifted text as a Lex fragment, and returns an atomic
  `WorkspaceEdit` containing `CreateFile` + target-content
  `TextEdit` + host-replace `TextEdit`. All logic lives in
  `lex_lsp::features::extract` so per-editor shims stay thin —
  editor wiring is tracked at
  [#498](https://github.com/lex-fmt/lex/issues/498). The deeper
  `GeneralContainer` host-policy check (e.g. rejecting a Session
  selected for extraction into a Definition body) is reserved for
  a follow-up; the `ExtractError::ContainerPolicy` variant stays in
  the enum for it.

### Fixed

- **`lexd inspect` silently dropped verbatim blocks when source
  mentioned `lex.include` in prose**
  ([#505](https://github.com/lex-fmt/lex/issues/505)). The CLI's
  `expand_includes_to_source` round-trips source through
  `resolve_from_source` → `serialize_to_lex_with_rules` → re-parse
  whenever the literal string `lex.include` appears anywhere — even
  in backticked prose or proposal docs describing the feature. The
  Lex serializer wrote a verbatim block's subject line directly
  after the preceding paragraph (the parser consumes the
  separating blank line as part of the verbatim's preamble, so it
  isn't represented as a `BlankLineGroup` in the AST and no other
  visitor emits it). On the re-parse the paragraph absorbed the
  subject and the verbatim — plus everything it bracketed — silently
  disappeared from the resolved tree. `visit_verbatim_block` now
  emits the leading blank line, suppressed when the verbatim is the
  first child of a `Subject:`-style container opener (Definition,
  verbatim group) where a blank at column 0 would terminate the
  container.

### Changed

- **Trust-matrix flip (lex#528 PR 12d).** `lex_fmt::boot_registry`
  now installs the OS-appropriate `Sandbox` on the trust gate (via
  the new `lex_extension_host::sandbox::os_default()` factory) and
  passes the same `Arc<dyn Sandbox>` to every
  `SubprocessHandler::spawn_with_sandbox` call. The effect is
  per-OS:
  - **Linux**: declared-pure subprocess handlers (`capabilities: {
    fs: false, net: false }`) **auto-trust** under the default
    engine — no prompt, no `--enable-handlers` flag. The kernel
    enforces the declared capability set via `LinuxSandbox`
    (seccomp + landlock). This is the user-visible matrix flip
    promised by §8 of the proposal.
  - **macOS**: pure handlers continue to **prompt** because
    `MacosSandbox::supports` is pinned to `false` pending a
    hardened `(deny default)` SBPL profile. The same prompt UX
    Windows uses.
  - **Windows + other**: pure handlers continue to **prompt** —
    `NullSandbox` is the fallback. `lex#528` PR 12c (Windows Job
    Objects + restricted tokens) is unscheduled; the trust gate
    routes those subprocesses to the prompt path until it ships.
  Full subprocess handlers (any `capabilities` declaring `fs` or
  `net`) always prompt regardless of OS — `supports` returns
  `false` for non-pure shapes on every impl.

### Added

- `lex_extension_host::sandbox::os_default() -> Arc<dyn Sandbox>`:
  factory selecting the per-target default sandbox. Used by the
  δ-phase trust-matrix flip (above); also available to embedders
  building a custom `Engine` who want the same per-OS dispatch.
- `lex-extension-host::sandbox::MacosSandbox`: macOS implementation of
  the `Sandbox` trait (lex#528 PR 12b). Installs a Sandbox Profile
  Language (SBPL) policy via the libSystem `sandbox_init` API inside
  a `pre_exec` hook so it applies to the child after `fork()` and
  survives `execve()`. The v1 profile denies `network*` and reads of
  `/etc` (covering both probe-fixture targets) while keeping other
  operations permissive enough for the system loader to bring up a
  Rust binary. **`supports()` returns `false` for every capability
  shape on macOS** until a hardened `(deny default)` profile lands —
  the current `(allow default)` profile still permits writes
  anywhere on disk and reads outside `/etc`, so auto-trusting on it
  would let a `pure`-declared handler silently exfiltrate or modify
  user data. The trust gate routes pure handlers to the prompt path
  on macOS, same as Windows or no-landlock Linux, until the
  hardened profile work ships. `sandbox_init` is deprecated since
  macOS 10.8 but still resolvable in libSystem through Sequoia —
  same dependency Apple's `sandbox-exec` utility relies on.
- `lex-extension-host::sandbox::LinuxSandbox`: Linux implementation of
  the `Sandbox` trait (lex#528 PR 12a). Combines `landlock` (filesystem
  allowlist — dynamic-loader paths + the handler binary itself) and
  `seccompiler` (network-stack syscall deny) installed via a `pre_exec`
  hook so the policy applies to the child after `fork()` and survives
  `execve()`. `supports()` returns `true` only for the
  `Capabilities::is_pure()` shape; finer capability shapes report
  unsupported until the schema grows the corresponding fields. Build
  surface remains MIT/Apache (`landlock` MIT/Apache, `seccompiler`
  Apache/BSD) — `lex-extension-host` and downstream consumers stay MIT.
- `lex-extension-host::sandbox`: new module hosting the `Sandbox`
  trait (the OS-level sandbox facade), a `SandboxError` error type,
  and a `NullSandbox` no-op default that reports
  `supports(_) == false` for every capability set on every platform.
  Foundation for the δ-phase trust matrix flip (lex#528): per-OS
  sandbox implementations (12a Linux via seccomp+landlock, 12b macOS
  via sandbox-exec, 12c Windows via Job Objects + restricted tokens)
  plug in behind this trait; the trust gate consults
  `Sandbox::supports(caps)` to decide whether a declared-pure handler
  can auto-trust without a prompt.
- `SubprocessHandler::spawn_with_sandbox` companion to the existing
  `spawn`. Takes `Arc<dyn Sandbox>` so the host can install one
  instance and share it with `TrustGate`. The worker thread calls
  `apply_to(&mut cmd, capabilities)` before `cmd.spawn()` so the
  kernel enforces declared capabilities from the child's first
  instruction. `spawn` (the existing entry point) now delegates to
  `spawn_with_sandbox` with `NullSandbox` — no behaviour change for
  current callers.
- `SpawnError::Sandbox(String)` variant for policy-install failures
  on the host side. Same handling as other spawn errors today;
  retry-with-prompt path is future work alongside PR 12d.
- `TrustGate::set_sandbox(Arc<dyn Sandbox>)` and
  `TrustGate::sandbox()` accessor (returns the Arc by clone). The
  default install is `NullSandbox`, so β/γ behaviour is preserved
  (every subprocess prompts). When PR 12a/b/c land, `lex-fmt`
  swaps in the OS-appropriate impl and shares the same Arc across
  the gate and the transport — guaranteeing the auto-trust decision
  is anchored on the sandbox that actually enforces policy.

### Changed

- Renamed the `lex-engine` crate to `lex-fmt` and positioned it as the
  canonical Rust embedder API for the lex document format. The
  `boot_registry` / `ExtensionSetup` / `BootOutcome` types move
  unchanged; only the crate name (and the `use` paths in `lexd` /
  `lexd-lsp`) change. Sets up PR 11's `Engine::builder()` to land in
  a discoverable place (`use lex_fmt::Engine`) rather than under a
  name that says "boot helper". `lex-engine` 0.11.0 stays published
  on crates.io for historical reference; future releases publish as
  `lex-fmt`.
- `TrustGate::evaluate` now short-circuits to `Trusted` for the
  `(transport=Subprocess, capability=Pure)` pair when
  `sandbox.supports(Capabilities::default())` returns `true`. With
  `NullSandbox` (the default), this branch is inactive —
  observable behaviour is unchanged. Tests cover both the
  supported and unsupported paths via a mock sandbox.
## [0.11.0] - 2026-05-10


### Added

- `lexd-lsp` trust prompt: subprocess handlers that haven't been
  pinned in `<workspace>/.lex/trust.json` now forward a
  `lex/trustRequest` LSP custom request to the editor. The editor
  (vscode / nvim / lexed in the coordinated γ release) renders an
  editor-native prompt and replies; the response feeds the trust gate
  and pins the decision for subsequent sessions. Sync→async bridge
  runs on the boot's `spawn_blocking` thread via
  `tokio::runtime::Handle::block_on` — the runtime keeps serving
  other LSP requests while the prompt is open. Boot is serialized
  with a `tokio::sync::Mutex` so a burst of LSP requests on file
  open (semantic tokens + hover + folding + …) produces exactly one
  boot, not N parallel boots with N parallel trust prompts. Boot
  diagnostics (resolver failures, denied namespaces, schema errors)
  are surfaced to the editor as `window/showMessage` notifications
  so users can see why a configured extension isn't working. Replaces
  10a's `LspDeferTrustPrompt` stub. Part of the γ phase of the
  extension system (lex-fmt/lex#516).
- New `lex-engine` crate: lifts the extension boot helper out of `lex-cli`
  so both `lexd` and `lexd-lsp` can share a single
  `boot_registry(ExtensionSetup { ... }) -> BootOutcome` entry point.
  `ExtensionSetup` now takes a `Box<dyn TrustPromptHandler>` so each host
  installs a prompt that fits its UX (CLI denies with a
  `--enable-handlers` rationale; LSP forwards a `lex/trustRequest` to
  the editor). Future home of the public `Engine::builder()` facade
  for embedders (PR 11). Part of the γ phase of the extension system
  (lex-fmt/lex#516).
- `lexd-lsp` extension dispatch: `textDocument/hover`,
  `textDocument/completion`, and `textDocument/codeAction` requests now
  consult the registered extension namespaces' handlers in addition to
  the existing built-in providers. Hover takes precedence over the
  built-in when a registered handler returns content; completion + code
  actions are additive. Combined with the trust-prompt forwarding
  (above), this is the full LSP surface for third-party namespaces:
  trust untrusted handlers via `lex/trustRequest`, dispatch hooks via
  the registry, and fall through to built-ins when no namespace
  matches. The per-editor UI for `lex/trustRequest` lands in
  coordinated vscode/nvim/lexed releases.
- `lex-analysis::utils::find_verbatim_at_position`: locates a verbatim
  block whose source range contains the cursor position. Mirror of
  the existing `find_annotation_at_position`; used by extension
  dispatch to identify labelled verbatim blocks under the cursor.
- `lex-babel`: new `serialize_to_html_with_registry(doc, options, &Registry)`
  entry point and `HtmlExportOutcome { html, diagnostics }` result type.
  Walks the AST, dispatches `on_render` for every labelled annotation /
  verbatim whose schema declares `hooks.render: ["html"]`, and surfaces
  handler diagnostics. Splice integration of handler-rendered output
  into the HTML stream is a separate follow-up — today's entry point
  produces the same default HTML as the registry-less path while
  collecting handler diagnostics. Part of the extension-system β phase
  (lex-fmt/lex#516).
- `lex-babel::render_dispatch`: format-independent render-hook walker
  that builds a `RenderPlan` of `(label, output, diagnostic)` triples
  for the format-specific serializer to splice. Sister module to
  `lex-analysis::label_dispatch` (validate hooks).
## [0.10.6] - 2026-05-07


### Fixed

- Inline-walker columns are now UTF-16 code units, matching what LSP
  clients expect by default. Two cursor walkers —
  `lex_core::lex::ast::inline_positions::position_at` (semantic tokens
  + document links) and `lex_analysis::inline::ReferenceWalker::position_at`
  (`find_references` + `goto_definition`) — were accumulating
  `column += ch.len_utf8()` for each char as they walked through a
  line. For any char wider in UTF-8 than in UTF-16 (notably `→` at 3
  bytes / 1 unit, `§` at 2 bytes / 1 unit, non-BMP emoji at 4 bytes /
  2 units), every subsequent inline token's column was shifted right
  by `len_utf8 - len_utf16`. In VSCode this surfaced as the
  open-backtick of an inline code span landing on the *next* glyph
  (the `e` of `Setup` instead of the `` ` ``), and as
  `find_references` / `goto_definition` jumping to the wrong column on
  any line that contained a `→` before the reference. Switched both
  walkers to `len_utf16`. Byte-level `Range::span` values are
  unchanged — they remain UTF-8 byte offsets, which is correct.

  Caveat for follow-up: the rest of the AST still records
  `Position.column` as a UTF-8 byte offset (see
  `SourceLocation::byte_to_position`, and the deliberate slicing-by-bytes
  in `lex_lsp_core::available_actions::label_from_diagnostic_range`).
  Block-level ranges sent to LSP that span content with non-ASCII
  characters (e.g., a session title containing a `→`) therefore still
  have a similar shift in their *end* columns. The deeper fix is to
  convert UTF-8 byte columns to UTF-16 at the LSP wire boundary; this
  PR is scoped to the inline-walker bug that was visible in editor
  rendering of paragraph text.
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
