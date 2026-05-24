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
