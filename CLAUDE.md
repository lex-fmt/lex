<!-- BEGIN release-managed orientation — managed by release-core; do not edit -->
This repo's quality gate, build, release, and PR/dev flow are provided by
`release-core` (installed at session start; not stored in this repo).

- **Start here:** run `release-core how-to` — the task playbook for *this* repo
  (its dev cycle, incl. coordinating a complex / multi-PR feature with subagents).
- Reference: `release-core --help`, `release-core <cmd> --help`, `release-core detect-kind`.
- Quality gate (run every loop, after `git add`): `release-core gate`.
<!-- END release-managed orientation -->

# Lex

Lex is a plain text format for structured documents — more expressive than Markdown, human-readable in raw form. Structure comes from indentation (4-space tabs), not markup.

## Repo Structure

This is the unified Rust workspace containing all Lex crates and specifications.

```text
crates/
  lex-core/       Parser crate (crates.io)
  lex-analysis/   Stateless semantic analysis (crates.io)
  lex-lsp/        LSP server, tower-lsp — package: lexd-lsp (crates.io)
  lex-babel/      Format conversion via IR (crates.io)
  lex-config/     Configuration loader (crates.io)
  lex-cli/        Command-line interface — package: lexd (crates.io)
  lex-wasm/       WebAssembly bindings (not published)
comms/
  specs/          Grammar specs and test fixtures
  docs/           Website content (lex.ing)
  assets/         Images and resources
```

## Key Files

| What | Where |
|------|-------|
| AST nodes | `crates/lex-core/src/lex/ast.rs` |
| Parser | `crates/lex-core/src/lex/parsing.rs` |
| Lexer | `crates/lex-core/src/lex/lexing.rs` |
| Grammar specs | `comms/specs/grammar-{core,line,inline}.lex` |
| Test fixtures | `comms/specs/elements/`, `comms/specs/trifecta/`, `comms/specs/benchmark/` |
| LSP server | `crates/lex-lsp/src/server.rs` |
| Analysis modules | `crates/lex-analysis/src/{semantic_tokens,document_symbols,hover,go_to_definition,completion,diagnostics}.rs` |
| Format adapters | `crates/lex-babel/src/formats/` |
| CLI entry | `crates/lex-cli/src/main.rs` |

## Development

- All crates build together: `cargo build --workspace`
- Tests: `cargo nextest run --workspace` or `cargo test --workspace`
- Pre-commit hook: `lefthook install` (lefthook composes `lefthook.yml` from arthur-debert/release Components — md/yaml/sh lint + cargo fmt/clippy check)
- Exclude lex-wasm from most commands: `--exclude lex-wasm`

## Releasing

This repo participates in the lex release cascade. Cutting a release here is triggered automatically when comms releases (via the `on-upstream-released` handler workflow). Once cut, it cascades further: lex's `notify-downstreams` step fires `repository_dispatch upstream-released` to vscode + nvim + lexed.

For a manual cut: `gh workflow run release.yml --repo lex-fmt/lex -f version=X.Y.Z` (lex uses workflow_dispatch, not tag-push — the canonical rust-cli@v1 workflow drives the bump+commit+tag itself).

Design + ops + gotchas: [arthur-debert/release/docs/lex-release-cascade.md](https://github.com/arthur-debert/release/blob/main/docs/lex-release-cascade.md).

## Related repos

- [tree-sitter-lex](https://github.com/lex-fmt/tree-sitter-lex) — Tree-sitter grammar (syntax highlighting, injection, textobjects)
- [lexed](https://github.com/lex-fmt/lexed) — Electron desktop editor
- [nvim](https://github.com/lex-fmt/nvim) — Neovim plugin
- [vscode](https://github.com/lex-fmt/vscode) — VSCode extension

Editor UIs download pre-built lexd-lsp binaries from this repo's releases,
and tree-sitter artifacts from lex-fmt/tree-sitter-lex releases.

For local development, set `LEX_LSP_PATH` to point editors at a local build:

```sh
cargo build -p lexd-lsp
LEX_LSP_PATH=./target/debug/lexd-lsp
```

## CLI Quick Reference

```sh
lexd inspect file.lex                    # AST tree visualization (default)
lexd inspect file.lex ast-tag            # XML-like AST
lexd inspect file.lex ast-json           # JSON AST
lexd inspect file.lex --ast-full         # Full AST with all properties
lexd inspect file.lex token-line-simple  # Token stream (line-classified)
lexd inspect file.lex ir-json            # Intermediate representation
lexd file.lex --to markdown              # Convert formats
lexd format file.lex                     # Auto-format
```
