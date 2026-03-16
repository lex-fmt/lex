# Lex

Lex is a plain text format for structured documents — more expressive than Markdown, human-readable in raw form. Structure comes from indentation (4-space tabs), not markup.

## Repo Structure

This is the unified Rust workspace containing all Lex crates and specifications.

```
crates/
  lex-core/       Parser crate (crates.io)
  lex-analysis/   Stateless semantic analysis (crates.io)
  lex-lsp/        LSP server, tower-lsp (crates.io)
  lex-babel/      Format conversion via IR (crates.io)
  lex-config/     Configuration loader (crates.io)
  lex-cli/        Command-line interface (crates.io)
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
- Pre-commit hook: `scripts/rust-pre-commit` (runs fmt, clippy, build, test)
- Exclude lex-wasm from most commands: `--exclude lex-wasm`

## Releasing

Tag with `vX.Y.Z` and push. CI publishes crates in dependency order:
lex-core → lex-config → lex-babel → lex-analysis → lex-lsp

CI also builds lex-cli and lex-lsp binaries for 6 platforms.

## Related repos

- [tree-sitter-lex](https://github.com/lex-fmt/tree-sitter-lex) — Tree-sitter grammar (syntax highlighting, injection, textobjects)
- [lexed](https://github.com/lex-fmt/lexed) — Electron desktop editor
- [nvim](https://github.com/lex-fmt/nvim) — Neovim plugin
- [vscode](https://github.com/lex-fmt/vscode) — VSCode extension

Editor UIs download pre-built lex-lsp binaries from this repo's releases,
and tree-sitter artifacts from lex-fmt/tree-sitter-lex releases.

For local development, set `LEX_LSP_PATH` to point editors at a local build:
```sh
cargo build -p lex-lsp
LEX_LSP_PATH=./target/debug/lex-lsp
```

## CLI Quick Reference

```sh
lex inspect file.lex                    # AST tree visualization (default)
lex inspect file.lex ast-tag            # XML-like AST
lex inspect file.lex ast-json           # JSON AST
lex inspect file.lex --ast-full         # Full AST with all properties
lex inspect file.lex token-line-simple  # Token stream (line-classified)
lex inspect file.lex ir-json            # Intermediate representation
lex file.lex --to markdown              # Convert formats
lex format file.lex                     # Auto-format
```
