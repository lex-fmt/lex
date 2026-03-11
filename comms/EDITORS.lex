# Lex Editor Feature Support Matrix

This document tracks the implementation status of Lex features across different editor clients.

| Feature | LSP (Rust) | VSCode | Lexed (Electron) | Neovim | Notes |
| :--- | :---: | :---: | :---: | :---: | :--- |
| **Diagnostics** |
| Syntax Errors | ✅ | ✅ | ✅ | ✅ | From `lex-parser` |
| Spellcheck | ✅ | ✅ | ✅ | ❌ | Includes "Add to dictionary" |
| Footnote Matching | ✅ | ✅ | ✅ | ❓ | Missing defs / unused defs |
| **Navigation** |
| Go to Definition | ✅ | ✅ | ✅ | ✅ | Headers, footnotes |
| Find References | ✅ | ✅ | ✅ | ✅ | Headers, footnotes |
| Annotation Nav | ✅ | ❌ | ✅ | ❌ | `next/previous_annotation` cmds |
| **Formatting** |
| Document Format | ✅ | ✅ | ✅ | ✅ | |
| Range Format | ✅ | ✅ | ✅ | ❓ | |
| **Completion** |
| Snippets | ⚠️ | ✅ | ❌ | ❌ | `@note`, `@image`, etc. |
| Wikilinks | ❌ | ❌ | ❌ | ❌ | Not implemented |
| Footnotes | ❌ | ❌ | ❌ | ❌ | Auto-complete refs? |
| **Code Actions** |
| Spellcheck Fixes | ✅ | ✅ | ✅ | ❌ | Suggestions + Dictionary |
| Footnote Fixes | ✅ | ⚠️ | ⚠️ | ❌ | "Add missing def" (placeholder) |
| Reorder Footnotes | ✅ | ✅ | ✅ | ❌ | `source.organizeImports` / specialized |
| **Commands** |
| Insert Asset | ✅ | ✅ | ❌ | ❌ | File picker integration needed |
| Insert Verbatim | ✅ | ✅ | ❌ | ❌ | File picker integration needed |
| Toggle Annotations| ✅ | ❌ | ❌ | ❌ | |

**Legend:**
- ✅ : Fully Implemented
- ⚠️ : Partially Implemented / WIP
- ❌ : Not Implemented
- ❓ : Unknown / Needs Verification

## Client-Specific Notes

### VSCode
- Uses standard VSCode APIs.
- Some commands rely on client-side logic (e.g., file pickers) bridging to LSP.

### Lexed
- Custom Electron-based editor using Monaco.
- Features implemented via `monaco-languageclient` or custom providers.
- "Insert Asset" needs Electron IPC for file dialogs.

### Neovim
- Standard built-in LSP client.
- Requires setup in `nvim-lspconfig`.
