//! The `lexd` clap command tree.
//!
//! [`build_cli`] assembles the top-level `Command` with every subcommand and
//! global flag. The `config` subcommand is layered on by `main` via clapfig's
//! `ConfigCommand`, so it is not built here.

use clap::{Arg, ArgAction, Command, ValueHint};
use lexd::transforms;
use std::path::PathBuf;

pub(crate) fn build_cli() -> Command {
    Command::new("lexd")
        .version(env!("CARGO_PKG_VERSION"))
        .about("A tool for inspecting and converting lex files")
        .long_about(
            "lexd is a command-line tool for working with lex document files.\n\n\
            Commands:\n  \
            - inspect: View internal representations (tokens, AST, etc.)\n  \
            - convert: Transform between document formats (lex, markdown, HTML, etc.)\n  \
            - config:  Manage configuration (list, get, set, gen)\n\n\
            Configuration:\n  \
            Settings are loaded from .lex.toml files, LEX__* env vars, and CLI flags.\n  \
            Use `lexd config list` to see resolved settings.\n\n\
            Includes:\n  \
            `lexd convert` and `lexd inspect` resolve `:: lex.include src=\"...\" ::`\n  \
            annotations by default, splicing the included file's content into the\n  \
            host tree. Use --no-includes to disable. `lexd format` never expands\n  \
            includes. See comms/specs/proposals/includes.lex for the design.\n\n\
            Examples:\n  \
            lexd inspect file.lex                    # View AST tree visualization\n  \
            lexd inspect file.lex ast-tag            # View AST as XML tags\n  \
            lexd inspect file.lex --ast-full         # Show complete AST (all node properties)\n  \
            lexd file.lex --to markdown              # Convert to markdown (outputs to stdout)\n  \
            lexd file.lex --to html -o output.html   # Convert to HTML file\n  \
            lexd file.lex --to lex --no-includes     # Convert without expanding includes\n  \
            lexd config list                         # Show all resolved settings\n  \
            lexd config set convert.html.theme fancy-serif  # Persist a setting"
        )
        .arg_required_else_help(true)
        .subcommand_required(false)
        .arg(
            Arg::new("list-transforms")
                .long("list-transforms")
                .help("List available transforms")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("config-path")
                .long("config")
                .value_name("PATH")
                .help("Path to a .lex.toml configuration file")
                .value_hint(ValueHint::FilePath)
                .global(true),
        )
        .arg(
            Arg::new("no-includes")
                .long("no-includes")
                .help("Disable lex.include resolution (operate on the unresolved tree)")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("includes-root")
                .long("includes-root")
                .value_name("PATH")
                .help("Resolution root for lex.include (default: nearest .lex.toml or entry-file directory)")
                .value_hint(ValueHint::DirPath)
                .global(true),
        )
        .arg(
            Arg::new("no-warnings")
                .long("no-warnings")
                .help("Suppress non-fatal warnings on stderr (also: LEX_QUIET=1)")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .subcommand(
            Command::new("inspect")
                .about("Inspect internal representations of lex files")
                .long_about(
                    "View the internal structure of lex files at different processing stages.\n\n\
                    Transforms (stage-format):\n  \
                    - ast-tag:      AST as XML-like tags\n  \
                    - ast-treeviz:  AST as tree visualization (default)\n  \
                    - ast-nodemap:  AST as character/color map\n  \
                    - ast-json:     AST as JSON\n  \
                    - parity:       Block skeleton for tree-sitter parity checking\n  \
                    - token-*:      Token stream representations\n  \
                    - ir-json:      Intermediate representation\n\n\
                    Examples:\n  \
                    lexd inspect file.lex                     # Tree visualization (default)\n  \
                    lexd inspect file.lex ast-tag             # XML-like output\n  \
                    lexd inspect file.lex --ast-full          # Complete AST with all properties\n  \
                    lexd inspect file.lex token-core-json     # View token stream"
                )
                .arg(
                    Arg::new("path")
                        .help("Path to the lex file (reads from stdin if omitted)")
                        .required(false)
                        .index(1)
                        .value_hint(ValueHint::FilePath),
                )
                .arg(
                    Arg::new("transform")
                        .help(
                            "Transform to apply (stage-format). Defaults to 'ast-treeviz'",
                        )
                        .long_help(
                            "Transform to apply in the format stage-format.\n\n\
                            Available transforms:\n  \
                            ast-treeviz, ast-tag, ast-json, ast-nodemap,\n  \
                            token-core-json, token-line-json,\n  \
                            ir-json, and more.\n\n\
                            Use --list-transforms to see all options."
                        )
                        .required(false)
                        .value_parser(clap::builder::PossibleValuesParser::new(
                            transforms::AVAILABLE_TRANSFORMS,
                        ))
                        .index(2)
                        .value_hint(ValueHint::Other),
                )
                .arg(
                    Arg::new("ast-full")
                        .long("ast-full")
                        .help("Show complete AST including all node properties")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("no-linum")
                        .long("no-linum")
                        .help("Hide line numbers in AST output")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("color")
                        .long("color")
                        .help("Use ANSI-colored blocks in nodemap output")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("color-char")
                        .long("color-char")
                        .help("Color Base2048 glyphs with ANSI codes in nodemap output")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("node-summary")
                        .long("node-summary")
                        .help("Show summary statistics under nodemap output")
                        .action(ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("convert")
                .about("Convert between document formats (default command)")
                .long_about(
                    "Convert documents between different formats.\n\n\
                    Supported formats (see comms/docs/interop-scope.lex for tiering):\n  \
                    - lex:      Lex format (.lex)                       [core]\n  \
                    - markdown: Markdown (.md)                          [core, both directions]\n  \
                    - html:     HTML with optional themes (.html)       [core, export only]\n  \
                    - pdf:      PDF via headless Chrome                 [core, export only]\n  \
                    - png:      PNG via headless Chrome screenshot      [core, export only]\n  \
                    - rfc_xml:  IETF RFC XML v3                         [experimental, import only]\n  \
                    - tag:      XML-like tag format (diagnostic)\n\n\
                    The source format is auto-detected from the file extension.\n\
                    Text outputs go to stdout by default, or use -o to specify a file.\n\
                    Binary outputs (pdf, png) require -o <path>.\n\n\
                    Examples:\n  \
                    lexd convert input.lex --to markdown          # Convert to markdown (stdout)\n  \
                    lexd convert input.md --to lex -o output.lex  # Markdown to lex file\n  \
                    lexd convert doc.lex --to html -o out.html    # Generate HTML\n  \
                    lexd input.lex --to markdown                  # 'convert' is optional"
                )
                .arg(
                    Arg::new("input")
                        .help("Input file path (reads from stdin if omitted)")
                        .required(false)
                        .index(1)
                        .value_hint(ValueHint::FilePath),
                )
                .arg(
                    Arg::new("from")
                        .long("from")
                        .help("Source format (auto-detected from file extension if not specified)")
                        .long_help(
                            "Source format to convert from.\n\n\
                            If not specified, the format is auto-detected from the file extension.\n\
                            Use this option to override auto-detection."
                        )
                        .value_hint(ValueHint::Other),
                )
                .arg(
                    Arg::new("to")
                        .long("to")
                        .help("Target format (required)")
                        .long_help(
                            "Target format to convert to.\n\n\
                            Interop formats: lex, markdown, html, pdf, png (core);\n  \
                                             rfc_xml (experimental, parse-only).\n\
                            Diagnostic formats: tag, treeviz, linetreeviz.\n\
                            Use the format name, not the file extension.\n\
                            See comms/docs/interop-scope.lex for the v1 tiering."
                        )
                        .required(true)
                        .value_hint(ValueHint::Other),
                )
                .arg(
                    Arg::new("output")
                        .long("output")
                        .short('o')
                        .help("Output file path (defaults to stdout)")
                        .long_help(
                            "Path to write the converted output.\n\n\
                            If not specified, output is written to stdout.\n\
                            The file extension should match the target format."
                        )
                        .value_hint(ValueHint::FilePath),
                )
                .arg(
                    Arg::new("theme")
                        .long("theme")
                        .help("HTML theme (e.g. 'fancy-serif', 'modern')")
                        .value_hint(ValueHint::Other),
                )
                .arg(
                    Arg::new("css-path")
                        .long("css-path")
                        .help("Path to custom CSS file for HTML export")
                        .value_hint(ValueHint::FilePath),
                )
                .arg(
                    Arg::new("pdf-size")
                        .long("pdf-size")
                        .help("PDF page profile ('lexed' or 'mobile')")
                        .value_parser(["lexed", "mobile"])
                        .value_hint(ValueHint::Other),
                ),
        )
        .subcommand(
            Command::new("format")
                .about("Format a lex file")
                .long_about(
                    "Format a lex file using standard formatting rules.\n\n\
                    This command parses the input lex file and re-serializes it,\n\
                    applying standard indentation and spacing rules.\n\n\
                    Output is always written to stdout.\n\n\
                    Examples:\n  \
                    lexd format input.lex                  # Format to stdout\n  \
                    lexd format input.lex > formatted.lex  # Redirect to file"
                )
                .arg(
                    Arg::new("input")
                        .help("Input file path (reads from stdin if omitted)")
                        .required(false)
                        .index(1)
                        .value_hint(ValueHint::FilePath),
                ),
        )
        .subcommand(
            Command::new("element-at")
                .about("Get information about the element at a specific position")
                .arg(
                    Arg::new("path")
                        .help("Path to the lex file")
                        .required(true)
                        .index(1)
                        .value_hint(ValueHint::FilePath),
                )
                .arg(
                    Arg::new("row")
                        .help("Row number (1-based)")
                        .required(true)
                        .index(2)
                        .value_parser(clap::value_parser!(usize)),
                )
                .arg(
                    Arg::new("col")
                        .help("Column number (1-based)")
                        .required(true)
                        .index(3)
                        .value_parser(clap::value_parser!(usize)),
                )
                .arg(
                    Arg::new("all")
                        .long("all")
                        .help("Show all ancestors")
                        .action(ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("token-at")
                .about("Get the semantic token at a specific position")
                .arg(
                    Arg::new("path")
                        .help("Path to the lex file")
                        .required(true)
                        .index(1)
                        .value_hint(ValueHint::FilePath),
                )
                .arg(
                    Arg::new("row")
                        .help("Row number (1-based)")
                        .required(true)
                        .index(2)
                        .value_parser(clap::value_parser!(usize)),
                )
                .arg(
                    Arg::new("col")
                        .help("Column number (1-based)")
                        .required(true)
                        .index(3)
                        .value_parser(clap::value_parser!(usize)),
                ),
        )
        .subcommand(
            Command::new("generate-lex-css")
                .about("Output the default CSS used for HTML export")
                .long_about(
                    "Outputs the default baseline CSS used when converting to HTML.\n\n\
                    Use this as a starting point for custom styling. The output can be\n\
                    saved to a file and customized, then referenced via the\n\
                    convert.html.custom_css config setting.\n\n\
                    Examples:\n  \
                    lexd generate-lex-css                    # Print CSS to stdout\n  \
                    lexd generate-lex-css > custom.css       # Save to file for editing"
                ),
        )
        .arg(
            Arg::new("ext-schema")
                .long("ext-schema")
                .help("Register a local extension schema directory (repeatable)")
                .long_help(
                    "Add an extension namespace by pointing at a local directory of\n\
                     YAML schemas. Repeatable: --ext-schema ./acme --ext-schema ./other.\n\
                     The directory's basename becomes the namespace name.",
                )
                .value_name("DIR")
                .value_parser(clap::value_parser!(PathBuf))
                .action(ArgAction::Append)
                .global(true),
        )
        .arg(
            Arg::new("enable-handlers")
                .long("enable-handlers")
                .help("Allow subprocess extension handlers to run")
                .long_help(
                    "Subprocess extension handlers don't run by default in CLI mode\n\
                     (the trust gate denies them). Pass --enable-handlers to opt in for\n\
                     this run. The flag does not persist any trust decisions to the\n\
                     workspace's .lex/trust.json — it's a one-shot.",
                )
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .subcommand(
            Command::new("labels")
                .about("Manage and inspect extension namespaces")
                .long_about(
                    "Inspect the extension namespaces visible to lexd, validate documents\n\
                     against registered schemas, and (in future releases) emit and update\n\
                     namespace caches.\n\n\
                     Subcommands:\n  \
                     list             — print every registered namespace with its source\n  \
                     validate <doc>   — run analysis against a document, print diagnostics\n  \
                     emit <doc> [...] — walk a document's labelled annotations / verbatims,\n  \
                                        write one NDJSON record per match to stdout",
                )
                .subcommand_required(true)
                .subcommand(
                    Command::new("list").about("List registered extension namespaces"),
                )
                .subcommand(
                    Command::new("validate")
                        .about("Validate a document against registered schemas")
                        .arg(
                            Arg::new("path")
                                .help("Path to the .lex document")
                                .value_hint(ValueHint::FilePath)
                                .required(true),
                        ),
                )
                .subcommand(
                    Command::new("emit")
                        .about("Emit NDJSON records for the document's labelled nodes")
                        .long_about(
                            "Walk a .lex document's labelled annotations and verbatim blocks \
                             and write one newline-delimited JSON record per match to stdout. \
                             Pull-based export for downstream tools (static-site generators, \
                             indexers, pipelines). Filter with --label and --namespace; both \
                             repeatable, intersected when combined.\n\n\
                             Record shape uses the wire AST `Position`/`Range` types — same \
                             format LSP hover and extension hook payloads use. Body shapes (JSON): \
                             `{\"kind\":\"none\"}` for marker labels, `{\"kind\":\"text\",\"text\":\"…\"}` for \
                             text bodies (incl. verbatim) and `{\"kind\":\"lex\",\"wire\":[…]}` for \
                             parsed bodies.",
                        )
                        .arg(
                            Arg::new("path")
                                .help("Path to the .lex document")
                                .value_hint(ValueHint::FilePath)
                                .required(true),
                        )
                        .arg(
                            Arg::new("label")
                                .long("label")
                                .help("Only emit records for this label (repeatable)")
                                .action(clap::ArgAction::Append),
                        )
                        .arg(
                            Arg::new("namespace")
                                .long("namespace")
                                .help("Only emit records in this namespace (repeatable)")
                                .action(clap::ArgAction::Append),
                        ),
                ),
        )
        .subcommand(
            Command::new("check")
                .about("Lint .lex documents and report diagnostics (CI-friendly)")
                .long_about(
                    "Run lex-analysis diagnostics over one or more documents and report \
                     findings with a CI-friendly exit-code contract.\n\n\
                     Each file is parsed, its `lex.include` annotations are expanded by \
                     default (use --no-includes to skip), the extension registry is booted \
                     so schema/handler diagnostics fire, and the analysis pass runs with \
                     any `[diagnostics.rules]` severity overrides from `.lex.toml` applied. \
                     Include-assembly failures (missing/cyclic/oversize includes) surface \
                     here as diagnostics blamed on the include site. Findings originating \
                     inside an included file are reported against that file's path.\n\n\
                     Pass --references to also validate internal cross-references \
                     (session / definition / annotation / citation) over the merged \
                     document: a reference is flagged when its target is absent from the \
                     whole tree. Resolution is bidirectional across includes (a fragment \
                     may reference targets in its master and vice-versa). These findings \
                     default to warning severity, configurable per-rule via \
                     `[diagnostics.rules]`.\n\n\
                     Exit codes:\n  \
                     0: clean (no finding at/above the --fail-on threshold)\n  \
                     1: at least one finding met the threshold\n  \
                     2: operational error (unreadable file, bad arguments)",
                )
                .arg(
                    Arg::new("paths")
                        .help("Paths to the .lex documents (one or more)")
                        .value_hint(ValueHint::FilePath)
                        .num_args(1..)
                        .required(true),
                )
                .arg(
                    Arg::new("fail-on")
                        .long("fail-on")
                        .value_name("SEVERITY")
                        .help("Severity at/above which a finding fails the run")
                        .value_parser(["error", "warning", "info", "hint"])
                        .default_value("warning"),
                )
                .arg(
                    Arg::new("format")
                        .long("format")
                        .value_name("FORMAT")
                        .help("Output format")
                        .value_parser(["human", "json"])
                        .default_value("human"),
                )
                .arg(
                    Arg::new("references")
                        .long("references")
                        .help(
                            "Also validate internal cross-references (session / \
                             definition / annotation / citation) over the merged \
                             document",
                        )
                        .action(ArgAction::SetTrue),
                ),
        )
}
