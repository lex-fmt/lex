Tools

1. lex CLI

    lex is the command-line swiss army knife for converting, formatting, and inspecting lex documents. It keeps the whole lifecycle inside a single binary by delegating to the Rust crates described below.

2. Installation

    Clone the tools repository and build the binary with Cargo:

        git clone https://github.com/lex-fmt/tools
        cd tools
        cargo build --release
        # Binary at target/release/lex
    :: shell ::
3. Commands

    3.1. Convert

        Convert between formats (lex, markdown, html, pdf). The `convert` subcommand is optional because it’s the default behavior.

            # Convert lex to markdown (stdout)
            lex document.lex --to markdown
            
            # Convert lex to HTML with output file
            lex document.lex --to html -o output.html
            
            # Convert lex to PDF
            lex document.lex --to pdf -o output.pdf
            
            # Convert markdown back to lex
            lex document.md --to lex
            
            # Explicit convert subcommand
            lex convert document.lex --to html
        :: shell ::
        The source format is auto-detected from the extension. Override with `--from` when needed.

    3.2. Format

        Apply canonical spacing and indentation rules:

            # Format to stdout
            lex format document.lex
            
            # Redirect to a file
            lex format document.lex > formatted.lex
        :: shell ::
    3.3. Inspect

        Explore intermediate representations—ideal for debugging the language pipeline.

            # AST tree visualization (default)
            lex inspect document.lex
            
            # AST as XML-like tags
            lex inspect document.lex ast-tag
            
            # AST as JSON
            lex inspect document.lex ast-json
            
            # Token stream
            lex inspect document.lex token-core-json
            
            # Show all AST properties
            lex inspect document.lex --ast-full
        :: shell ::
        Available transforms:
            - ast-treeviz
            - ast-tag
            - ast-json
            - ast-nodemap
            - token-core-json
            - token-line-json
            - ir-json

    3.4. Element At

        Locate the element (or ancestor chain) covering a given cursor position.

            # Get element at row 10, column 5
            lex element-at document.lex 10 5
            
            # Show all ancestors
            lex element-at document.lex 10 5 --all
        :: shell ::
4. Supported Formats

    Definition:
        lex:
            Import ✓ · Export ✓ · Extension `.lex`
    Definition:
        Markdown:
            Import ✓ · Export ✓ · Extension `.md`
    Definition:
        HTML:
            Import – · Export ✓ · Extension `.html`
    Definition:
        PDF:
            Import – · Export ✓ · Extension `.pdf`

5. Configuration

    Settings are loaded from `lex.toml` files, `LEX__*` environment variables, and CLI flags. Use `lex config` to manage settings.

        # Show all resolved settings
        lex config list

        # Persist a setting
        lex config set convert.html.theme fancy-serif

        # Generate a template config file
        lex config gen -o lex.toml

        # Override the config file path
        lex document.lex --to html --config ./my-lex.toml
    :: shell ::
    Definition:
        Format-specific flags:
            Convert and inspect subcommands accept format-specific flags.

            # HTML with theme
            lex document.lex --to html --theme fancy-serif

            # PDF with mobile page size
            lex document.lex --to pdf --pdf-size mobile -o out.pdf

            # Inspect with full AST properties
            lex inspect document.lex --ast-full
        :: shell ::
    Example `lex.toml`:

        [formatting.rules]
        session_blank_lines_before = 2
        session_blank_lines_after = 1
        normalize_seq_markers = true
        indent_string = "    "
        
        [convert.html]
        theme = "default"
        
        [convert.pdf]
        size = "default"  # or "mobile" or "lexed"
        
        [inspect.ast]
        include_all_properties = false
        show_line_numbers = true
    :: text language=toml

6. lex-babel Library

    The conversion engine lives in the `lex-babel` crate. Use it directly when integrating lex with other systems.

        use lex_babel::FormatRegistry;
        
        let registry = FormatRegistry::default();
        
        // Parse from markdown
        let doc = registry.parse(&markdown_source, "markdown")?;
        
        // Serialize to HTML
        let html = registry.serialize(&doc, "html")?;
    :: rust ::
    Architecture summary:
        - IR layer: format-agnostic events
        - Common layer: flat↔nested transformations
        - Format layer: adapters per target format

7. Design Principles

    - Invisible syntax: indentation reveals hierarchy
    - Graceful degradation: fallback to paragraphs instead of errors
    - Complete lifecycle: from drafts to publishing
    - Tool-friendly: deterministic grammar
    - Future-proof: plain Unicode text
