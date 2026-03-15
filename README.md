# Lex

A plain text format for structured documents — more expressive than Markdown, readable without tooling.

Structure comes from indentation and numbering, not markup. Ideas grow from free-form notes to technical documents without switching formats.

**[lex.ing](https://lex.ing)** — project site, specs, and documentation.

## Ecosystem

This repo is the unified Rust workspace containing all backend crates:

| Crate | Description |
|-------|-------------|
| `lex-core` | Parser and AST |
| `lex-babel` | Format conversion (Markdown, HTML, PDF, PNG, Pandoc JSON, RFC XML) |
| `lex-analysis` | Semantic analysis |
| `lex-lsp` | LSP server (semantic highlighting, symbols, formatting, completion, diagnostics, hover, go-to-definition, references, folding, document links) |
| `lex-config` | Configuration (clapfig) |
| `lex-cli` | Command-line interface |
| `lex-wasm` | WebAssembly bindings |

Specs and docs live in [`lex-fmt/comms`](https://github.com/lex-fmt/comms) (submoduled as `comms/`).

### Editor Plugins

- [VS Code](https://github.com/lex-fmt/vscode)
- [Neovim](https://github.com/lex-fmt/nvim)
- [LexEd](https://github.com/lex-fmt/lexed) — standalone desktop editor (Electron)

All editors use `lex-lsp` for language features and ship a monochrome theme optimized for prose.

## Install

```sh
cargo install lex-cli
cargo install lex-lsp
```

Editor plugins have their own installation instructions.

## Development

```sh
cargo build --workspace
cargo nextest run --workspace    # or cargo test --workspace
```

Pre-commit hook: `scripts/rust-pre-commit` (fmt, clippy, build, test).

Tree-sitter grammar lives in `tree-sitter/` and is released as an artifact alongside binaries.

## Release

Tag with `vX.Y.Z` and push. CI publishes crates in dependency order and builds binaries for 6 platforms.

## Contributing

Contributions welcome — code, docs, bug reports — via GitHub issues and PRs.

## License

MIT
