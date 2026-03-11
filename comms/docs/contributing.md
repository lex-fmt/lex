---
layout: default
title: Contributing
---

# Contributing

lex is open source and contributions are welcome.

## Repository Structure

The project spans multiple repositories under the [lex-fmt](https://github.com/lex-fmt) organization:

| Repository | Description | Language |
|------------|-------------|----------|
| [core](https://github.com/lex-fmt/core) | lex-core parser | Rust |
| [tools](https://github.com/lex-fmt/tools) | CLI, lex-babel, lex-config | Rust |
| [editors](https://github.com/lex-fmt/editors) | lex-lsp, lex-analysis | Rust |
| [lexed](https://github.com/lex-fmt/lexed) | Desktop editor | TypeScript/Electron |
| [vscode](https://github.com/lex-fmt/vscode) | VS Code extension | TypeScript |
| [nvim](https://github.com/lex-fmt/nvim) | Neovim plugin | Lua |
| [comms](https://github.com/lex-fmt/comms) | Specs, docs, website | Markdown/lex |

## Dependency Flow

```
                comms/specs
                     |
                     v
  core (lex-parser) <----+
         |
         +----------------------+
         |                      |
         v                      v
  editors/lex-analysis    tools/lex-babel
         |                      |
         v                      v
  editors/lex-lsp         tools/lex-cli
         |                      |
         +---------+------------+
                   |
    +--------------+--------------+
    |              |              |
    v              v              v
  lexed          nvim          vscode
```

## How to Contribute

1. **Find an issue** or propose a change in the relevant repository
2. **Fork** the repository
3. **Create a branch** for your changes
4. **Submit a PR** with passing tests

### Requirements

- All PRs must have passing tests
- Follow existing code style
- Update relevant documentation

## Development Setup

### Prerequisites

- Git
- Rust toolchain ([rustup.rs](https://rustup.rs))
- Node.js 20+ (for lexed, vscode)

### Workspace Setup

Clone the workspace (aggregates all repositories):

```bash
git clone https://github.com/lex-fmt/lex-workspace.git lex
cd lex
./scripts/setup.sh          # Clone all repositories (HTTPS)
./scripts/setup.sh --ssh    # Use SSH URLs instead
```

### Building

```bash
# Build core parser
cd core && cargo build

# Build tools (CLI, lex-babel)
cd tools && cargo build

# Build LSP server
cd editors && cargo build

# Build Lexed
cd lexed && npm ci && npm run build

# Build VS Code extension
cd vscode && npm ci && npm run build
```

### Testing

```bash
# Rust crates
cargo test

# VS Code extension
cd vscode && npm test

# Lexed e2e tests
cd lexed && npm run test:e2e
```

## Local Development (Cross-Component Changes)

When making changes that span multiple components (e.g., lex-babel changes that affect lex-lsp):

```bash
./scripts/build-local.sh
```

This temporarily patches editors/ to use local lex-babel and lex-core, builds lex-lsp, and places it in `target/local/lex-lsp`.

To use the local binary with editors:

```bash
# lexed
LEX_LSP_PATH="$(pwd)/target/local/lex-lsp" npm run dev --prefix lexed

# vscode (set before launching Extension Development Host)
export LEX_LSP_PATH="$(pwd)/target/local/lex-lsp"

# nvim
vim.g.lex_lsp_path = "/path/to/lex/target/local/lex-lsp"
```

## Testing Conventions

All crates use official sample files from `comms/specs/` for tests:

- **kitchensink**: Comprehensive document with all features
- **trifecta**: Three focused test files covering edge cases
- **elements/**: Isolated tests for individual lex elements

Tests load fixtures via the testing module in lex-core.

## Releasing

Releases follow the dependency order. Each repo has GitHub Actions that trigger on version tags (`v*`):

1. **Release lex-core** (if changed):
   ```bash
   cd core && git tag v0.X.Y && git push --tags
   ```

2. **Release tools** (lex-babel, lex-config, lex-cli):
   ```bash
   cd tools && git tag v0.X.Y && git push --tags
   ```

3. **Release editors** (lex-analysis, lex-lsp):
   ```bash
   cd editors && git tag v0.X.Y && git push --tags
   ```

4. **Update editor clients**:
   - lexed: Update `shared/src/lex-version.json`
   - vscode: Update `LEX_LSP_VERSION` in `scripts/download-lex-lsp.sh`
   - nvim: Users auto-download on first use

## Questions?

Open an issue in the relevant repository or reach out on GitHub Discussions.
