# @lex-fmt/lex-wasm

WebAssembly bindings for the [Lex](https://github.com/lex-fmt/lex) language —
parser, semantic analysis, formatting, and HTML export running entirely in the
browser. No server round-trip, no native dependency.

These bindings expose the same sync feature surface as `lexd-lsp` (the stdio
LSP server), routed through the shared `lex-lsp-core` crate. Editors that
already speak the Language Server Protocol can drive the WASM module via a
WebWorker transport instead of stdio.

## Install

```sh
npm install @lex-fmt/lex-wasm
```

## Usage

```js
import init, { LexDocument } from "@lex-fmt/lex-wasm";

await init();
const doc = LexDocument.new("Section:\n\n    - item one\n");
console.log(doc.format());
```

The module ships as ESM with TypeScript declarations. Use the `web` target if
you load it directly in the browser without a bundler; this package is built
with `--target bundler`, intended for Vite, webpack, esbuild, etc.

## Features

- Parsing and AST inspection (`LexDocument.new`, `toHtml`)
- LSP-equivalent providers: semantic tokens, document symbols, hover,
  goto-definition, references, folding, completion, diagnostics
- Spellcheck against an embedded en_US dictionary (`EmbeddedSpellchecker`)
- Document formatting (`format`)

See the [main repository](https://github.com/lex-fmt/lex) for the Lex language
reference and editor integrations (VS Code, Neovim, Lexed).

## License

MIT — see [LICENSE](https://github.com/lex-fmt/lex/blob/main/LICENSE).
