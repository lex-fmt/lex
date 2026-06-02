// Command-line interface for lex
//
// This binary provides commands for inspecting and converting lex files.
//
// The inspect command is an internal tool for aid in the development of the lex ecosystem, and is bound to be be extracted to it's own crate in the future.
//
// The main role for the lexd program is to interface with lex content. Be it converting to and fro, linting or formatting it.
// The core capabilities use the lex-babel crate. This crate being a interface for the lex-babel library, which is a collection of formats and transformers.
//
// Converting:
//
// The conversion needs a to and from pair. The to can be auto-detected from the file extension, while being overwrittable by an explicit --from flag.
// Usage:
//  lexd <input> --to <format> [--from <format>] [--output <file>]  - Convert between formats (default)
//  lexd convert <input> --to <format> [--from <format>] [--output <file>]  - Same as above (explicit)
//  lexd inspect <path> [<transform>]      - Execute a transform (defaults to "ast-treeviz")
//  lexd --list-transforms                 - List available transforms
//  lexd config [list|gen|get|set|unset]   - Manage configuration
//
// Configuration:
//
// Settings are loaded from .lex.toml files (CWD, project root, platform config dir),
// environment variables (LEX__*), and CLI flags. Use `lex config` to manage settings.

use lexd::transforms;

use clap::{Arg, ArgAction, ArgMatches, Command, ValueHint};
use clapfig::{Boundary, Clapfig, ConfigCommand, SchemaConfigBuilder, SearchPath};
use lex_analysis::semantic_tokens::collect_semantic_tokens;
use lex_babel::{
    formats::lex::formatting_rules::FormattingRules, transforms::serialize_to_lex_with_rules,
    FormatRegistry, SerializedDocument,
};
use lex_config::{LexConfig, PdfPageSize, CONFIG_FILE_NAME};
use lex_core::lex::ast::{find_node_path_at_position, Position};
use lex_core::lex::builtins;
use lex_core::lex::includes::{
    resolve_from_source, FsLoader, LoadError, LoadedFile, Loader, ResolveConfig,
};
use lex_core::lex::mojibake::detect_mojibake;
use lex_extension_host::registry::Registry;
use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn build_cli() -> Command {
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
            Command::new("check-labels")
                .about("Check a .lex file for label-policy violations (CI-friendly)")
                .long_about(
                    "Parse a .lex file in permissive mode and report any label-policy \
                     violations that strict-mode parsing would have rejected:\n\n  \
                     - `forbidden-label-prefix` — labels using the reserved `doc.*` \
                     prefix (see general.lex §4.1)\n  \
                     - `unknown-lex-canonical` — `lex.*` literals that aren't \
                     registered canonicals\n\n\
                     Permissive parse means the rest of the file still produces \
                     diagnostics — useful for batch CI runs where you want to see all \
                     violations at once rather than failing on the first.\n\n\
                     Exit codes:\n  \
                     0: clean (no label-policy violations)\n  \
                     1: at least one violation found\n  \
                     2: I/O failure (file not found) or fatal parse error",
                )
                .arg(
                    Arg::new("path")
                        .help("Path to the .lex document")
                        .value_hint(ValueHint::FilePath)
                        .required(true),
                ),
        )
}

fn main() {
    let config_cmd = ConfigCommand::new();
    let cli = build_cli().subcommand(config_cmd.as_command("config"));

    let args: Vec<String> = std::env::args().collect();

    // First, try normal parsing
    let matches = match cli.clone().try_get_matches_from(&args) {
        Ok(m) => m,
        Err(e) => {
            const KNOWN_SUBCOMMANDS: &[&str] = &[
                "inspect",
                "convert",
                "config",
                "format",
                "element-at",
                "token-at",
                "generate-lex-css",
                "labels",
                "check-labels",
                "help",
            ];
            let first_arg = args.get(1).map(String::as_str);
            let has_subcommand = first_arg
                .filter(|arg| !arg.starts_with('-'))
                .is_some_and(|arg| KNOWN_SUBCOMMANDS.contains(&arg));
            let has_to_flag = args.iter().any(|a| a == "--to" || a.starts_with("--to="));
            let first_is_file = first_arg.is_some_and(|arg| !arg.starts_with('-'));

            // Inject "convert" when the invocation looks like a conversion but
            // the subcommand was omitted (e.g. `lexd file.lex --to md` or
            // `cat file.lex | lexd --to md`).
            if !has_subcommand && (first_is_file || has_to_flag) {
                let mut new_args = vec![args[0].clone(), "convert".to_string()];
                new_args.extend_from_slice(&args[1..]);

                match cli.try_get_matches_from(&new_args) {
                    Ok(m) => m,
                    Err(e2) => e2.exit(),
                }
            } else {
                e.exit();
            }
        }
    };

    if matches.get_flag("list-transforms") {
        handle_list_transforms_command();
        return;
    }

    let builder = make_builder(&matches);

    match matches.subcommand() {
        Some(("config", sub)) => {
            let action = config_cmd.parse(sub).unwrap_or_else(|e| {
                eprintln!("Config error: {e}");
                std::process::exit(1);
            });
            builder.handle_and_print(&action).unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1);
            });
        }
        // `check-labels` doesn't need workspace config — only the
        // built-in `lex.*` canonical set, which the analysis pass
        // consults through compile-in constants. Short-circuiting
        // before `builder.load()` keeps the documented exit-code
        // contract (0/1/2 only) — a config load failure inside this
        // subcommand would otherwise exit with code 1 instead of 2.
        Some(("check-labels", sub_matches)) => {
            let exit = handle_check_labels_command(sub_matches);
            if exit != 0 {
                std::process::exit(exit);
            }
        }
        _ => {
            let config = builder.load().unwrap_or_else(|e| {
                eprintln!("Failed to load configuration: {e}");
                std::process::exit(1);
            });

            match matches.subcommand() {
                Some(("inspect", sub_matches)) => {
                    let pos1 = sub_matches.get_one::<String>("path").map(|s| s.as_str());
                    let pos2 = sub_matches
                        .get_one::<String>("transform")
                        .map(|s| s.as_str());
                    // When only one positional is given and it matches a known
                    // transform name, treat it as the transform (stdin mode).
                    let (path, transform) = match (pos1, pos2) {
                        (Some(p), None) if transforms::AVAILABLE_TRANSFORMS.contains(&p) => {
                            (None, p)
                        }
                        (p, t) => (p, t.unwrap_or("ast-treeviz")),
                    };
                    let inc = IncludeOptions::for_expanding_command(&matches, &config);
                    handle_inspect_command(path, transform, &config, &inc);
                }
                Some(("convert", sub_matches)) => {
                    let input = sub_matches.get_one::<String>("input").map(|s| s.as_str());
                    let from_arg = sub_matches.get_one::<String>("from");
                    let to = sub_matches.get_one::<String>("to").expect("to is required");

                    // Auto-detect --from if not provided and we have a file path
                    let from = if let Some(f) = from_arg {
                        f.to_string()
                    } else if let Some(path) = input {
                        let registry = FormatRegistry::default();
                        match registry.detect_format_from_filename(path) {
                            Some(detected) => detected,
                            None => {
                                eprintln!("Error: Could not detect format from filename '{path}'");
                                eprintln!("Please specify --from explicitly");
                                std::process::exit(1);
                            }
                        }
                    } else {
                        eprintln!("Error: --from is required when reading from stdin");
                        std::process::exit(1);
                    };

                    let output = sub_matches.get_one::<String>("output").map(|s| s.as_str());
                    let inc = IncludeOptions::for_expanding_command(&matches, &config);
                    let warnings_on = warnings_enabled(&matches);
                    handle_convert_command(input, &from, to, output, &config, &inc, warnings_on);
                }
                Some(("format", sub_matches)) => {
                    let input = sub_matches.get_one::<String>("input").map(|s| s.as_str());
                    // Format command always outputs to stdout (no -o flag) and
                    // never expands includes (per proposal §11.4).
                    let inc = IncludeOptions::for_format_command();
                    let warnings_on = warnings_enabled(&matches);
                    handle_convert_command(input, "lex", "lex", None, &config, &inc, warnings_on);
                }
                Some(("element-at", sub_matches)) => {
                    let path = sub_matches
                        .get_one::<String>("path")
                        .expect("path is required");
                    let row = *sub_matches
                        .get_one::<usize>("row")
                        .expect("row is required");
                    let col = *sub_matches
                        .get_one::<usize>("col")
                        .expect("col is required");
                    let all = sub_matches.get_flag("all");
                    handle_element_at_command(path, row, col, all);
                }
                Some(("token-at", sub_matches)) => {
                    let path = sub_matches
                        .get_one::<String>("path")
                        .expect("path is required");
                    let row = *sub_matches
                        .get_one::<usize>("row")
                        .expect("row is required");
                    let col = *sub_matches
                        .get_one::<usize>("col")
                        .expect("col is required");
                    handle_token_at_command(path, row, col);
                }
                Some(("generate-lex-css", _)) => {
                    handle_generate_lex_css_command();
                }
                Some(("labels", sub_matches)) => {
                    let exit = handle_labels_command(&matches, sub_matches);
                    if exit != 0 {
                        std::process::exit(exit);
                    }
                }
                _ => {
                    eprintln!("Unknown subcommand. Use --help for usage information.");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Dispatch `lexd check-labels <path>`. Parses the file permissively
/// so `doc.*` and unknown `lex.*` labels survive into the AST,
/// then runs the analysis pass and filters to the label-policy
/// diagnostics (`ForbiddenLabelPrefix` + `UnknownLexCanonical`).
/// Reports each violation with line/column info; exits non-zero
/// when any are found.
///
/// Exit codes:
///
/// - `0`: clean (no label-policy violations).
/// - `1`: at least one violation found.
/// - `2`: I/O failure (file not found) or fatal parse error.
fn handle_check_labels_command(sub: &ArgMatches) -> i32 {
    use lex_analysis::diagnostics::{analyze, DiagnosticKind};
    use lex_core::lex::parsing::parse_document_permissive;

    let path: PathBuf = sub
        .get_one::<String>("path")
        .map(PathBuf::from)
        .expect("clap enforces required");

    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lexd check-labels: failed to read {}: {e}", path.display());
            return 2;
        }
    };

    let document = match parse_document_permissive(&source) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!(
                "lexd check-labels: {} could not be parsed: {e}",
                path.display()
            );
            return 2;
        }
    };

    // Only label-policy diagnostics are in scope for this subcommand.
    // Other analysis diagnostics (missing-footnote, table-column,
    // schema validation) keep firing through `lexd format` /
    // `lex-lsp`; this command is the focused pre-flight check.
    let label_diags: Vec<_> = analyze(&document)
        .into_iter()
        .filter(|d| {
            matches!(
                d.kind,
                DiagnosticKind::ForbiddenLabelPrefix | DiagnosticKind::UnknownLexCanonical
            )
        })
        .collect();

    if label_diags.is_empty() {
        return 0;
    }

    for diag in &label_diags {
        let code = match diag.kind {
            DiagnosticKind::ForbiddenLabelPrefix => "forbidden-label-prefix",
            DiagnosticKind::UnknownLexCanonical => "unknown-lex-canonical",
            _ => "label-policy",
        };
        // 1-based line/column for the human-readable report.
        let line = diag.range.start.line + 1;
        let col = diag.range.start.column + 1;
        eprintln!(
            "{}:{}:{}: error[{code}]: {}",
            path.display(),
            line,
            col,
            diag.message
        );
    }
    eprintln!();
    eprintln!(
        "{}: {} label-policy violation(s)",
        path.display(),
        label_diags.len()
    );
    1
}

/// Dispatch `lexd labels {list,validate}`. Returns the exit code
/// to propagate.
fn handle_labels_command(top: &ArgMatches, sub: &ArgMatches) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // Walk ancestors looking for `.lex.toml`, matching the behaviour
    // of every other lexd subcommand (config, convert, inspect…).
    // Without this, running `lexd labels list` from a subdirectory
    // would miss the workspace's `[labels]` block and report only
    // the `--ext-schema` flags.
    let workspace = find_nearest_lex_toml_dir(&cwd).unwrap_or_else(|| cwd.clone());
    let labels_path = workspace.join(CONFIG_FILE_NAME);
    let labels_config = match lex_config::load_labels_from_toml(&labels_path) {
        Ok(c) => c,
        Err(lex_config::LabelsConfigError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            lex_config::LabelsConfig::default()
        }
        Err(e) => {
            eprintln!("lexd labels: {e}");
            return 2;
        }
    };
    let ext_schemas: Vec<PathBuf> = top
        .get_many::<PathBuf>("ext-schema")
        .map(|values| values.cloned().collect())
        .unwrap_or_default();
    let enable_handlers = top.get_flag("enable-handlers");
    let outcome = lexd::extension_setup::boot_registry(lexd::extension_setup::ExtensionSetup {
        workspace_root: &workspace,
        labels_config: &labels_config,
        ext_schemas: &ext_schemas,
        enable_handlers,
        surface_override: None,
    });
    match sub.subcommand() {
        Some(("list", _)) => lexd::labels_subcommand::list(&outcome),
        Some(("validate", v)) => {
            let path = v
                .get_one::<String>("path")
                .map(PathBuf::from)
                .expect("path is required");
            lexd::labels_subcommand::validate(&path, &outcome)
        }
        Some(("emit", v)) => {
            // emit doesn't need the boot registry — `to_wire_node`
            // builds the wire form without schema lookup. The boot
            // we already paid for above is wasted in this branch
            // but keeping a single boot call site is simpler than
            // branching the dispatch tree to skip boot for emit.
            let _ = outcome;
            let path = v
                .get_one::<String>("path")
                .map(PathBuf::from)
                .expect("path is required");
            let labels: Vec<String> = v
                .get_many::<String>("label")
                .map(|vals| vals.cloned().collect())
                .unwrap_or_default();
            let namespaces: Vec<String> = v
                .get_many::<String>("namespace")
                .map(|vals| vals.cloned().collect())
                .unwrap_or_default();
            lexd::labels_subcommand::emit(&path, &labels, &namespaces)
        }
        _ => {
            eprintln!("lexd labels: subcommand required (list, validate, emit)");
            2
        }
    }
}

/// Build a clapfig builder with search paths and CLI overrides from
/// parsed args. `accept_dotted_extension_keys_in` lets extension-
/// emitted diagnostic codes under `[diagnostics.rules]` pass strict-
/// mode validation without errors. The CLI doesn't consume the
/// collected entries — only the LSP runs `apply_rules` — so they're
/// ignored at load time.
fn make_builder(matches: &ArgMatches) -> SchemaConfigBuilder<LexConfig> {
    let mut builder = Clapfig::schema_builder::<LexConfig>()
        .app_name("lex")
        .file_name(CONFIG_FILE_NAME)
        .search_paths(vec![
            SearchPath::Platform,
            SearchPath::Ancestors(Boundary::Marker(".git")),
            SearchPath::Cwd,
        ])
        .persist_scope("local", SearchPath::Cwd)
        .persist_scope("user", SearchPath::Platform)
        .accept_dotted_extension_keys_in(
            lex_config::DIAGNOSTICS_RULES_PATH,
            clapfig::UnknownKeyDecision::Collect,
        );

    // Explicit config file path
    if let Some(path) = matches.get_one::<String>("config-path") {
        let p = std::path::Path::new(path);
        if !p.exists() {
            eprintln!("Configuration file not found: {path}");
            std::process::exit(1);
        }
        let dir = if p.is_file() { p.parent().unwrap() } else { p };
        builder = builder.add_search_path(SearchPath::Path(dir.to_path_buf()));
    }

    // Apply CLI flag overrides for inspect subcommand
    if let Some(("inspect", sub)) = matches.subcommand() {
        if sub.get_flag("ast-full") {
            builder = builder.cli_override("inspect.ast.include_all_properties", Some(true));
        }
        if sub.get_flag("no-linum") {
            builder = builder.cli_override("inspect.ast.show_line_numbers", Some(false));
        }
        if sub.get_flag("color") {
            builder = builder.cli_override("inspect.nodemap.color_blocks", Some(true));
        }
        if sub.get_flag("color-char") {
            builder = builder.cli_override("inspect.nodemap.color_characters", Some(true));
        }
        if sub.get_flag("node-summary") {
            builder = builder.cli_override("inspect.nodemap.show_summary", Some(true));
        }
    }

    // Apply CLI flag overrides for convert subcommand
    if let Some(("convert", sub)) = matches.subcommand() {
        if let Some(theme) = sub.get_one::<String>("theme") {
            builder = builder.cli_override("convert.html.theme", Some(theme.clone()));
        }
        if let Some(css) = sub.get_one::<String>("css-path") {
            builder = builder.cli_override("convert.html.custom_css", Some(css.clone()));
        }
        if let Some(size) = sub.get_one::<String>("pdf-size") {
            builder = builder.cli_override("convert.pdf.size", Some(size.clone()));
        }
    }

    builder
}

/// Per-invocation include resolution settings derived from CLI flags +
/// `[includes]` config + the entry-file's location.
#[derive(Debug, Clone)]
struct IncludeOptions {
    /// `true` to expand `lex.include` annotations during conversion/inspect.
    /// Always `false` for `lex format` (per spec §11.4) and when
    /// `--no-includes` is passed.
    enabled: bool,
    /// Explicit root override (`--includes-root` flag or `[includes].root`
    /// in `.lex.toml`). When `None`, the resolver picks the nearest
    /// `.lex.toml` walking up from the entry file, falling back to the
    /// entry file's own directory.
    root_override: Option<PathBuf>,
    /// Maximum include depth, taken from `[includes].max_depth`
    /// (default 8).
    max_depth: usize,
    /// Maximum total include count, taken from
    /// `[includes].max_total_includes` (default 1000).
    max_total_includes: usize,
    /// Maximum size of any single included file in bytes, taken from
    /// `[includes].max_file_size` (default 10 MiB).
    max_file_size: u64,
}

impl IncludeOptions {
    /// Build options for an "expand by default" command (convert / inspect).
    fn for_expanding_command(matches: &ArgMatches, config: &LexConfig) -> Self {
        Self {
            enabled: !matches.get_flag("no-includes"),
            root_override: matches
                .get_one::<String>("includes-root")
                .map(PathBuf::from)
                .or_else(|| config.includes.root.as_ref().map(PathBuf::from)),
            max_depth: config.includes.max_depth,
            max_total_includes: config.includes.max_total_includes,
            max_file_size: config.includes.max_file_size,
        }
    }

    /// Disabled options for `lex format` (formatter never expands per spec §11.4).
    fn for_format_command() -> Self {
        Self {
            enabled: false,
            root_override: None,
            max_depth: 8,
            max_total_includes: 1000,
            max_file_size: 10 * 1024 * 1024,
        }
    }

    /// Resolution root for an entry file at `entry_path`, applying:
    /// 1. `root_override` if present.
    /// 2. Directory of the nearest `.lex.toml` walking up from the entry file.
    /// 3. The entry file's own directory.
    ///
    /// In all three cases the returned path is run through
    /// [`absolutize_path`] so it is absolute and lexically normalized —
    /// `ResolveConfig::root` requires an absolute path or the
    /// root-escape prefix check is weakened.
    fn resolved_root(&self, entry_path: &Path) -> PathBuf {
        let raw = if let Some(r) = &self.root_override {
            r.clone()
        } else {
            let start_dir = entry_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            find_nearest_lex_toml_dir(&start_dir).unwrap_or(start_dir)
        };
        absolutize_path(&raw)
    }
}

/// Walk upward from `start` looking for a directory that contains
/// `.lex.toml` (the canonical config name in this repo). Returns that
/// directory, or `None` if we hit the filesystem root without finding one.
fn find_nearest_lex_toml_dir(start: &Path) -> Option<PathBuf> {
    let mut cur: PathBuf = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    loop {
        if cur.join(CONFIG_FILE_NAME).is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Best-effort absolutize: try `Path::canonicalize` (handles symlinks
/// and resolves `..` against the real filesystem), and fall back to
/// `current_dir().join(path)` if the path doesn't exist on disk yet
/// (rare but possible — e.g., a CLI flag pointing at a not-yet-created
/// directory). Always returns an absolute path; the resolver requires
/// one for the root-escape prefix check to be sound.
fn absolutize_path(p: &Path) -> PathBuf {
    if let Ok(canon) = p.canonicalize() {
        return canon;
    }
    if p.is_absolute() {
        return p.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(p))
        .unwrap_or_else(|_| p.to_path_buf())
}

/// Read source content from a file path, or from stdin when the path is
/// omitted. Exits with an error if no path is given and stdin is a terminal
/// (i.e. the user forgot to pipe input).
fn read_source(path: Option<&str>) -> String {
    match path {
        Some(p) => fs::read_to_string(p).unwrap_or_else(|e| {
            eprintln!("Error reading file '{p}': {e}");
            std::process::exit(1);
        }),
        None => {
            if io::stdin().is_terminal() {
                eprintln!(
                    "Error: no input file provided and stdin is a terminal. \
                     Pass a file path or pipe content via stdin."
                );
                std::process::exit(1);
            }
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
                eprintln!("Error reading from stdin: {e}");
                std::process::exit(1);
            });
            buf
        }
    }
}

/// Handle the inspect command
fn handle_inspect_command(
    path: Option<&str>,
    transform: &str,
    config: &LexConfig,
    inc: &IncludeOptions,
) {
    let mut source = read_source(path);

    // When includes are enabled and we have a real file path, resolve
    // them and re-serialize the merged tree as lex source. Inspect
    // transforms then see the post-include AST. For stdin input or
    // when --no-includes is set, we leave the source unchanged.
    if inc.enabled {
        if let Some(p) = path {
            source = expand_includes_to_source(&source, p, inc);
        }
    }

    let params = build_inspect_params(config);

    let output = transforms::execute_transform(&source, transform, &params).unwrap_or_else(|e| {
        eprintln!("Execution error: {e}");
        std::process::exit(1);
    });

    print!("{output}");
}

/// Resolve `lex.include` annotations and re-serialize the result as lex
/// source so downstream transforms (which take a string) see the merged
/// content. The two-step "resolve to AST → serialize back to lex" is
/// the simplest way to wire the resolver into a string-in/string-out
/// pipeline without restructuring the transform layer.
///
/// Fast path: if the source contains no `lex.include` literal, return
/// it unchanged. The resolver's round trip would be a parse + serialize
/// no-op semantically, but the lex serializer normalizes whitespace and
/// indentation in ways that surprise downstream tools that assert on
/// exact source layout (notably `inspect ast-nodemap`). Skipping the
/// round trip when there's nothing to resolve preserves byte-identical
/// behavior for documents that don't use the feature.
///
/// Errors are printed to stderr and the process exits non-zero; they
/// surface include problems early instead of letting them propagate as
/// confusing parser/serializer errors downstream.
fn expand_includes_to_source(source: &str, entry_path: &str, inc: &IncludeOptions) -> String {
    if !source.contains("lex.include") {
        return source.to_string();
    }
    // Canonicalize entry_path so the resolver sees an absolute path
    // for cycle-detection identity and so relative includes are
    // resolved from the real on-disk parent (not a CLI-relative one).
    let entry = absolutize_path(&PathBuf::from(entry_path));
    let root = inc.resolved_root(&entry);
    let resolve_config = ResolveConfig {
        root: root.clone(),
        max_depth: inc.max_depth,
        max_total_includes: inc.max_total_includes,
    };
    let loader = FsLoader::new(root).with_max_file_size(inc.max_file_size);
    let registry = Registry::new();
    builtins::register_into(&registry, Arc::new(loader), resolve_config.clone()).unwrap_or_else(
        |e| {
            eprintln!("Failed to register lex.* built-ins: {e}");
            std::process::exit(1);
        },
    );
    let doc =
        resolve_from_source(source, Some(entry), &resolve_config, &registry).unwrap_or_else(|e| {
            eprintln!("Include resolution error: {e}");
            std::process::exit(1);
        });

    // Re-serialize with default formatting rules; the goal is just
    // to feed downstream transforms the merged source, not to
    // produce author-grade output.
    let rules = FormattingRules::default();
    serialize_to_lex_with_rules(&doc, rules).unwrap_or_else(|e| {
        eprintln!("Failed to re-serialize merged document: {e}");
        std::process::exit(1);
    })
}

/// Handle the convert command
/// Loader decorator that records the canonical path of any file whose
/// source text trips the mojibake detector, then delegates to the
/// inner loader unchanged. The CLI uses this to surface a per-file
/// warning for content pulled in by `:: lex.include ::` — content the
/// entry-source mojibake scan can't see on its own.
struct MojibakeScanningLoader<L: Loader> {
    inner: L,
    scan_enabled: bool,
    findings: Arc<std::sync::Mutex<Vec<PathBuf>>>,
}

impl<L: Loader> MojibakeScanningLoader<L> {
    fn new(inner: L, scan_enabled: bool) -> Self {
        Self {
            inner,
            scan_enabled,
            findings: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn findings(&self) -> Arc<std::sync::Mutex<Vec<PathBuf>>> {
        Arc::clone(&self.findings)
    }
}

impl<L: Loader> Loader for MojibakeScanningLoader<L> {
    fn load(&self, path: &Path) -> Result<LoadedFile, LoadError> {
        let loaded = self.inner.load(path)?;
        if self.scan_enabled && detect_mojibake(&loaded.source).is_some() {
            let mut findings = self.findings.lock().expect("findings mutex");
            findings.push(loaded.canonical_path.clone());
        }
        Ok(loaded)
    }
}

/// Returns true when CLI warnings should be printed to stderr. Off when
/// either `--no-warnings` was passed or `LEX_QUIET` is set to a
/// non-empty, non-zero value.
fn warnings_enabled(matches: &ArgMatches) -> bool {
    if matches.get_flag("no-warnings") {
        return false;
    }
    !matches!(std::env::var("LEX_QUIET"), Ok(v) if !v.is_empty() && v != "0")
}

fn handle_convert_command(
    input: Option<&str>,
    from: &str,
    to: &str,
    output: Option<&str>,
    config: &LexConfig,
    inc: &IncludeOptions,
    warnings_on: bool,
) {
    let registry = FormatRegistry::default();

    // Validate formats exist
    if let Err(e) = registry.get(from) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
    if let Err(e) = registry.get(to) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    let source = read_source(input);

    if warnings_on && detect_mojibake(&source).is_some() {
        // Detection-only — never blocks the conversion. The user fixes
        // it at the source by re-saving from a clean UTF-8 editor.
        let label = input.unwrap_or("<stdin>");
        eprintln!(
            "warning: {label} appears to be UTF-8-double-encoded.\n  \
             em-dashes, accented letters, and curly quotes may be corrupted.\n  \
             consider re-saving the file in UTF-8 from a clean source."
        );
    }

    // Parse — for lex input with includes enabled and a real input
    // path, route through the include resolver so the merged tree is
    // what we serialize. Other input formats (markdown, html) have no
    // include concept; stdin can't anchor relative include paths.
    let doc = if from == "lex" && inc.enabled && input.is_some() {
        // Canonicalize so resolver-side path comparisons (cycle stack,
        // root-escape prefix check) work in absolute-path space.
        let entry = absolutize_path(&PathBuf::from(input.expect("input is Some by guard")));
        let root = inc.resolved_root(&entry);
        let resolve_config = ResolveConfig {
            root: root.clone(),
            max_depth: inc.max_depth,
            max_total_includes: inc.max_total_includes,
        };
        let inner_loader = FsLoader::new(root).with_max_file_size(inc.max_file_size);
        // Wrap the loader so each included file is scanned for mojibake
        // too — the entry-source scan above doesn't see content pulled
        // in by `:: lex.include ::`, so without this an included file
        // could silently propagate corrupted bytes through the
        // converter.
        let scanning_loader = MojibakeScanningLoader::new(inner_loader, warnings_on);
        let mojibake_paths = scanning_loader.findings();
        let registry = Registry::new();
        builtins::register_into(&registry, Arc::new(scanning_loader), resolve_config.clone())
            .unwrap_or_else(|e| {
                eprintln!("Failed to register lex.* built-ins: {e}");
                std::process::exit(1);
            });
        let resolved = resolve_from_source(&source, Some(entry), &resolve_config, &registry)
            .unwrap_or_else(|e| {
                eprintln!("Include resolution error: {e}");
                std::process::exit(1);
            });
        if warnings_on {
            let paths = mojibake_paths.lock().expect("loader findings mutex");
            for path in paths.iter() {
                eprintln!(
                    "warning: {} appears to be UTF-8-double-encoded.\n  \
                     em-dashes, accented letters, and curly quotes may be corrupted.\n  \
                     consider re-saving the file in UTF-8 from a clean source.",
                    path.display()
                );
            }
        }
        resolved
    } else {
        registry.parse(&source, from).unwrap_or_else(|e| {
            eprintln!("Parse error: {e}");
            std::process::exit(1);
        })
    };

    let mut format_options = HashMap::new();

    // Serialize (format-specific parameters from config)
    let result = if to == "lex" {
        let rules = formatting_rules_from_config(config);
        match serialize_to_lex_with_rules(&doc, rules) {
            Ok(text) => SerializedDocument::Text(text),
            Err(err) => {
                eprintln!("Serialization error: {err}");
                std::process::exit(1);
            }
        }
    } else {
        if to == "pdf" {
            format_options = pdf_params_from_config(config);
        } else if to == "html" {
            format_options.insert("theme".to_string(), config.convert.html.theme.clone());
            if let Some(css_path) = &config.convert.html.custom_css {
                format_options.insert("css-path".to_string(), css_path.clone());
            }
        }
        registry
            .serialize_with_options(&doc, to, &format_options)
            .unwrap_or_else(|e| {
                eprintln!("Serialization error: {e}");
                std::process::exit(1);
            })
    };

    // Output
    match (output, result) {
        (Some(path), data) => {
            fs::write(path, data.into_bytes()).unwrap_or_else(|e| {
                eprintln!("Error writing file '{path}': {e}");
                std::process::exit(1);
            });
        }
        (None, SerializedDocument::Text(text)) => {
            print!("{text}");
        }
        (None, SerializedDocument::Binary(_)) => {
            eprintln!("Binary formats (like PDF) require an output file. Use -o <path>.");
            std::process::exit(1);
        }
    }
}

/// Handle the element-at command
fn handle_element_at_command(path: &str, row: usize, col: usize, all: bool) {
    let source = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading file '{path}': {e}");
        std::process::exit(1);
    });

    let registry = FormatRegistry::default();
    let doc = registry.parse(&source, "lex").unwrap_or_else(|e| {
        eprintln!("Parse error: {e}");
        std::process::exit(1);
    });

    // Convert 1-based to 0-based
    let pos = Position::new(row.saturating_sub(1), col.saturating_sub(1));

    let path_nodes = find_node_path_at_position(&doc, pos);

    if path_nodes.is_empty() {
        eprintln!("No element found at {row}:{col}");
        return;
    }

    if all {
        for node in path_nodes {
            println!("{}: {}", node.node_type(), node.display_label());
        }
    } else if let Some(node) = path_nodes.last() {
        println!("{}: {}", node.node_type(), node.display_label());
    }
}

/// Handle the token-at command
fn handle_token_at_command(path: &str, row: usize, col: usize) {
    let source = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading file '{path}': {e}");
        std::process::exit(1);
    });

    let registry = FormatRegistry::default();
    let doc = registry.parse(&source, "lex").unwrap_or_else(|e| {
        eprintln!("Parse error: {e}");
        std::process::exit(1);
    });

    // Convert 1-based row/col to 0-based
    let target_line = row.saturating_sub(1);
    let target_col = col.saturating_sub(1);
    let tokens = collect_semantic_tokens(&doc);
    let lines: Vec<&str> = source.lines().collect();

    let matching: Vec<_> = tokens
        .iter()
        .filter(|t| {
            let s = &t.range.start;
            let e = &t.range.end;
            if s.line == e.line {
                // Single-line token
                s.line == target_line && target_col >= s.column && target_col < e.column
            } else {
                // Multi-line token
                if target_line == s.line {
                    target_col >= s.column
                } else if target_line == e.line {
                    target_col < e.column
                } else {
                    target_line > s.line && target_line < e.line
                }
            }
        })
        .collect();

    if matching.is_empty() {
        println!("No semantic token at {row}:{col}");
    } else {
        for token in &matching {
            let start = &token.range.start;
            let end = &token.range.end;
            let excerpt = if start.line == end.line {
                lines
                    .get(start.line)
                    .map(|l| {
                        let s = start.column.min(l.len());
                        let e = end.column.min(l.len());
                        &l[s..e]
                    })
                    .unwrap_or("")
            } else {
                lines
                    .get(start.line)
                    .map(|l| {
                        let s = start.column.min(l.len());
                        &l[s..]
                    })
                    .unwrap_or("")
            };
            println!(
                "{}:{}-{}:{}  {}  \"{}\"",
                start.line + 1,
                start.column + 1,
                end.line + 1,
                end.column + 1,
                token.kind.as_str(),
                excerpt,
            );
        }
    }
}

/// Handle the generate-lex-css command
fn handle_generate_lex_css_command() {
    print!("{}", lex_babel::formats::get_default_css());
}

/// Handle the list-transforms command
fn handle_list_transforms_command() {
    println!("Available transforms:\n");
    println!("Stages:");
    println!("  token-core  - Core tokenization (no semantic indentation)");
    println!("  token-line  - Full lexing with semantic indentation");
    println!("  ir          - Intermediate representation (parse tree)");
    println!("  ast         - Abstract syntax tree (final parsed document)\n");

    println!("Formats:");
    println!("  json        - JSON output (all stages)");
    println!("  tag         - XML-like tag format (AST only)");
    println!("  treeviz     - Tree visualization (AST only)");
    println!("  nodemap     - Character/color map (AST only)");
    println!("  simple      - Plain text token names");
    println!("  pprint      - Pretty-printed token names\n");

    println!("Available transform combinations:");
    for transform_name in transforms::AVAILABLE_TRANSFORMS {
        println!("  {transform_name}");
    }

    println!("\nConversion formats (v1 tiering — see comms/docs/interop-scope.lex):");
    let registry = FormatRegistry::default();
    for format_name in registry.list_formats() {
        let tier = format_tier(&format_name);
        println!("  {format_name:<12} {tier}");
    }
}

/// Returns a short tier label for a format name, used by
/// `lexd --list-transforms` to make the v1 scope visible at a glance.
/// See `comms/docs/interop-scope.lex` for the full tiering.
fn format_tier(name: &str) -> &'static str {
    match name {
        "lex" => "[core]",
        "markdown" => "[core, both directions]",
        "html" => "[core, export only]",
        "pdf" => "[core, export only]",
        "png" => "[core, export only]",
        "rfc_xml" => "[experimental, import only]",
        "tag" | "treeviz" | "linetreeviz" => "[diagnostic]",
        _ => "",
    }
}

fn formatting_rules_from_config(config: &LexConfig) -> FormattingRules {
    let cfg = &config.formatting.rules;
    FormattingRules {
        session_blank_lines_before: cfg.session_blank_lines_before,
        session_blank_lines_after: cfg.session_blank_lines_after,
        normalize_seq_markers: cfg.normalize_seq_markers,
        unordered_seq_marker: cfg.unordered_seq_marker,
        max_blank_lines: cfg.max_blank_lines,
        indent_string: cfg.indent_string.clone(),
        preserve_trailing_blanks: cfg.preserve_trailing_blanks,
        normalize_verbatim_markers: cfg.normalize_verbatim_markers,
    }
}

fn build_inspect_params(config: &LexConfig) -> HashMap<String, String> {
    let mut params = HashMap::new();

    if config.inspect.ast.include_all_properties {
        params.insert("ast-full".to_string(), "true".to_string());
    }

    params.insert(
        "show-linum".to_string(),
        config.inspect.ast.show_line_numbers.to_string(),
    );

    if config.inspect.nodemap.color_blocks {
        params.insert("color".to_string(), "true".to_string());
    }
    if config.inspect.nodemap.color_characters {
        params.insert("color-char".to_string(), "true".to_string());
    }
    if config.inspect.nodemap.show_summary {
        params.insert("nodesummary".to_string(), "true".to_string());
    }

    params
}

fn pdf_params_from_config(config: &LexConfig) -> HashMap<String, String> {
    let mut params = HashMap::new();
    match config.convert.pdf.size {
        PdfPageSize::LexEd => {
            params.insert("size-lexed".to_string(), "true".to_string());
        }
        PdfPageSize::Mobile => {
            params.insert("size-mobile".to_string(), "true".to_string());
        }
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> LexConfig {
        Clapfig::schema_builder::<LexConfig>()
            .app_name("lex")
            .no_env()
            .search_paths(vec![])
            .accept_dotted_extension_keys_in(
                lex_config::DIAGNOSTICS_RULES_PATH,
                clapfig::UnknownKeyDecision::Collect,
            )
            .load()
            .expect("defaults to load")
    }

    #[test]
    fn default_config_has_expected_values() {
        let config = test_config();
        assert_eq!(config.formatting.rules.session_blank_lines_before, 1);
        assert!(config.inspect.ast.show_line_numbers);
        assert!(!config.inspect.ast.include_all_properties);
        assert_eq!(config.convert.pdf.size, PdfPageSize::LexEd);
        assert_eq!(config.convert.html.theme, "default");
    }

    #[test]
    fn inspect_params_include_configured_defaults() {
        let config = test_config();
        let params = build_inspect_params(&config);
        assert_eq!(params.get("show-linum"), Some(&"true".to_string()));
        assert!(!params.contains_key("ast-full"));
        assert!(!params.contains_key("color"));
    }

    #[test]
    fn inspect_params_with_all_flags() {
        let mut config = test_config();
        config.inspect.ast.include_all_properties = true;
        config.inspect.nodemap.color_blocks = true;
        config.inspect.nodemap.color_characters = true;
        config.inspect.nodemap.show_summary = true;

        let params = build_inspect_params(&config);
        assert_eq!(params.get("ast-full"), Some(&"true".to_string()));
        assert_eq!(params.get("color"), Some(&"true".to_string()));
        assert_eq!(params.get("color-char"), Some(&"true".to_string()));
        assert_eq!(params.get("nodesummary"), Some(&"true".to_string()));
    }

    #[test]
    fn pdf_params_follow_configured_profile() {
        let mut config = test_config();
        config.convert.pdf.size = PdfPageSize::Mobile;
        let params = pdf_params_from_config(&config);
        assert_eq!(params.get("size-mobile"), Some(&"true".to_string()));
        assert!(!params.contains_key("size-lexed"));
    }

    #[test]
    fn pdf_params_default_lexed() {
        let config = test_config();
        let params = pdf_params_from_config(&config);
        assert_eq!(params.get("size-lexed"), Some(&"true".to_string()));
        assert!(!params.contains_key("size-mobile"));
    }
}
