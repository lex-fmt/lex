---
layout: default
title: Editors
---

# Editor Support

lex has first-class editor support through the `lex-lsp` language server. All editors share the same language intelligence backend.

## Features

| Feature | Description |
|---------|-------------|
| **Syntax Highlighting** | Semantic tokens for sessions, lists, definitions, annotations, inline formatting |
| **Document Outline** | Hierarchical symbol view of document structure |
| **Hover Previews** | Preview footnote and citation content |
| **Go to Definition** | Jump to footnote definitions, section targets |
| **Find References** | Find all uses of footnotes, citations |
| **Formatting** | Document and range formatting |
| **Folding** | Collapse sessions, lists, annotations, verbatim blocks |
| **Spellcheck** | With "Add to dictionary" support |
| **Code Actions** | Spelling suggestions, footnote fixes, reorder footnotes |

---

## Lexed (Desktop Editor)

Standalone, distraction-free editor built specifically for lex.

**Stack**: Electron + React + Monaco Editor

[Download Latest Release](https://github.com/lex-fmt/lexed/releases/latest) | [View on GitHub](https://github.com/lex-fmt/lexed)

### Features

- Monochrome theme (typography-focused, uses bold/italic instead of colors)
- Multi-pane editing with flexible splits
- Vim mode support
- Full LSP integration

### Keyboard Shortcuts (macOS)

| Shortcut | Action |
|----------|--------|
| `Cmd+Shift+]` / `[` | Cycle through tabs |
| `Cmd+1` ... `Cmd+9` | Focus pane by number |
| `Cmd+K` | Command palette |
| `Cmd+Shift+H` | Split horizontal |
| `Cmd+Shift+V` | Split vertical |
| `Cmd+Shift+/` | Show shortcuts reference |

On Windows/Linux, `Cmd` maps to `Ctrl` and `Option` maps to `Alt`.

---

## VS Code

Full VS Code integration with export/import commands and live preview.

[View on GitHub](https://github.com/lex-fmt/vscode)

### Installation

Build from source:
```bash
cd vscode
npm ci
npm run build
npx vsce package
# Install the generated .vsix file
```

### Commands

| Command | Description |
|---------|-------------|
| **Lex: Export to Markdown** | Convert active .lex to Markdown |
| **Lex: Export to HTML** | Convert active .lex to HTML |
| **Lex: Export to PDF** | Convert active .lex to PDF (prompts for save location) |
| **Lex: Convert to Lex** | Convert active .md to Lex format (prompts for save location) |
| **Lex: Open Preview** | Live HTML preview in current column |
| **Lex: Open Preview to the Side** | Live HTML preview side-by-side |

Commands appear in the command palette and context menus when editing .lex or .md files.

### Theming

The extension applies a monochrome theme to .lex files using typography (bold, italic) and grayscale intensity. Adapts to VS Code's light/dark mode.

---

## Neovim

Native Neovim plugin with LSP integration.

[View on GitHub](https://github.com/lex-fmt/nvim)

### Installation

With lazy.nvim:
```lua
{
    "lex-fmt/nvim",
    ft = "lex",
    dependencies = { "neovim/nvim-lspconfig" },
    config = function()
        require("lex").setup()
    end,
}
```

With packer.nvim:
```lua
use {
    "lex-fmt/nvim",
    requires = { "neovim/nvim-lspconfig" },
    config = function()
        require("lex").setup()
    end,
}
```

The plugin auto-downloads the `lex-lsp` binary on first use.

### Configuration

```lua
require("lex").setup({
    -- Theme: "monochrome" (default) or "native"
    theme = "monochrome",

    -- Custom binary path (optional)
    cmd = { "/path/to/lex-lsp" },

    -- Additional lspconfig options
    lsp_config = {
        on_attach = function(client, bufnr)
            -- Your custom on_attach
        end,
    },
})
```

### Theming

The default monochrome theme uses grayscale intensity levels:

| Group | Usage |
|-------|-------|
| `@lex.normal` | Full contrast content text |
| `@lex.muted` | Medium gray structural elements |
| `@lex.faint` | Light gray meta-information |
| `@lex.faintest` | Barely visible syntax markers |

Set `theme = "native"` to use your colorscheme's standard markup highlights instead.

---

## Feature Matrix

| Feature | LSP | VS Code | Lexed | Neovim |
|---------|:---:|:-------:|:-----:|:------:|
| Syntax Errors | ✓ | ✓ | ✓ | ✓ |
| Spellcheck | ✓ | ✓ | ✓ | - |
| Go to Definition | ✓ | ✓ | ✓ | ✓ |
| Find References | ✓ | ✓ | ✓ | ✓ |
| Document Format | ✓ | ✓ | ✓ | ✓ |
| Range Format | ✓ | ✓ | ✓ | ? |
| Annotation Navigation | ✓ | - | ✓ | - |
| Spellcheck Fixes | ✓ | ✓ | ✓ | - |
| Reorder Footnotes | ✓ | ✓ | ✓ | - |
| Insert Asset | ✓ | ✓ | - | - |
| Insert Verbatim | ✓ | ✓ | - | - |

✓ = Implemented, - = Not implemented, ? = Needs verification
