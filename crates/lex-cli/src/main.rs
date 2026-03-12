// Command-line interface for lex
//
// This binary provides commands for inspecting and converting lex files.
//
// The inspect command is an internal tool for aid in the development of the lex ecosystem, and is bound to be be extracted to it's own crate in the future.
//
// The main role for the lex program is to interface with lex content. Be it converting to and fro, linting or formatting it.
// The core capabilities use the lex-babel crate. This crate being a interface for the lex-babel library, which is a collection of formats and transformers.
//
// Converting:
//
// The conversion needs a to and from pair. The to can be auto-detected from the file extension, while being overwrittable by an explicit --from flag.
// Usage:
//  lex <input> --to <format> [--from <format>] [--output <file>]  - Convert between formats (default)
//  lex convert <input> --to <format> [--from <format>] [--output <file>]  - Same as above (explicit)
//  lex inspect <path> [<transform>]      - Execute a transform (defaults to "ast-treeviz")
//  lex --list-transforms                 - List available transforms
//  lex config [list|gen|get|set|unset]   - Manage configuration
//
// Configuration:
//
// Settings are loaded from .lex.toml files (CWD, project root, platform config dir),
// environment variables (LEX__*), and CLI flags. Use `lex config` to manage settings.

use lex_cli::transforms;

use clap::{Arg, ArgAction, ArgMatches, Command, ValueHint};
use clapfig::{Boundary, Clapfig, ClapfigBuilder, ConfigCommand, SearchPath};
use lex_analysis::semantic_tokens::collect_semantic_tokens;
use lex_babel::{
    formats::lex::formatting_rules::FormattingRules, transforms::serialize_to_lex_with_rules,
    FormatRegistry, SerializedDocument,
};
use lex_config::{LexConfig, PdfPageSize, CONFIG_FILE_NAME};
use lex_core::lex::ast::{find_node_path_at_position, Position};
use std::collections::HashMap;
use std::fs;

fn build_cli() -> Command {
    Command::new("lex")
        .version(env!("CARGO_PKG_VERSION"))
        .about("A tool for inspecting and converting lex files")
        .long_about(
            "lex is a command-line tool for working with lex document files.\n\n\
            Commands:\n  \
            - inspect: View internal representations (tokens, AST, etc.)\n  \
            - convert: Transform between document formats (lex, markdown, HTML, etc.)\n  \
            - config:  Manage configuration (list, get, set, gen)\n\n\
            Configuration:\n  \
            Settings are loaded from .lex.toml files, LEX__* env vars, and CLI flags.\n  \
            Use `lex config list` to see resolved settings.\n\n\
            Examples:\n  \
            lex inspect file.lex                    # View AST tree visualization\n  \
            lex inspect file.lex ast-tag            # View AST as XML tags\n  \
            lex inspect file.lex --ast-full         # Show complete AST (all node properties)\n  \
            lex file.lex --to markdown              # Convert to markdown (outputs to stdout)\n  \
            lex file.lex --to html -o output.html   # Convert to HTML file\n  \
            lex config list                         # Show all resolved settings\n  \
            lex config set convert.html.theme fancy-serif  # Persist a setting"
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
                    - token-*:      Token stream representations\n  \
                    - ir-json:      Intermediate representation\n\n\
                    Examples:\n  \
                    lex inspect file.lex                     # Tree visualization (default)\n  \
                    lex inspect file.lex ast-tag             # XML-like output\n  \
                    lex inspect file.lex --ast-full          # Complete AST with all properties\n  \
                    lex inspect file.lex token-core-json     # View token stream"
                )
                .arg(
                    Arg::new("path")
                        .help("Path to the lex file")
                        .required(true)
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
                    Supported formats:\n  \
                    - lex:      Lex format (.lex)\n  \
                    - markdown: Markdown (.md)\n  \
                    - html:     HTML with optional themes (.html)\n  \
                    - tag:      XML-like tag format\n\n\
                    The source format is auto-detected from the file extension.\n\
                    Output goes to stdout by default, or use -o to specify a file.\n\n\
                    Examples:\n  \
                    lex convert input.lex --to markdown          # Convert to markdown (stdout)\n  \
                    lex convert input.md --to lex -o output.lex  # Markdown to lex file\n  \
                    lex convert doc.lex --to html -o out.html    # Generate HTML\n  \
                    lex input.lex --to markdown                  # 'convert' is optional"
                )
                .arg(
                    Arg::new("input")
                        .help("Input file path")
                        .required(true)
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
                            Available formats: lex, markdown, html, tag\n\
                            Use the format name, not the file extension."
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
                    lex format input.lex                  # Format to stdout\n  \
                    lex format input.lex > formatted.lex  # Redirect to file"
                )
                .arg(
                    Arg::new("input")
                        .help("Input file path")
                        .required(true)
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
                    lex generate-lex-css                    # Print CSS to stdout\n  \
                    lex generate-lex-css > custom.css       # Save to file for editing"
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
            // Check if this is a "missing subcommand" error by seeing if the first arg looks like a file
            if args.len() > 1
                && !args[1].starts_with('-')
                && ![
                    "inspect",
                    "convert",
                    "config",
                    "format",
                    "element-at",
                    "token-at",
                    "generate-lex-css",
                    "help",
                ]
                .contains(&args[1].as_str())
            {
                // Inject "convert" as the subcommand
                let mut new_args = vec![args[0].clone(), "convert".to_string()];
                new_args.extend_from_slice(&args[1..]);

                // Try parsing again with "convert" injected
                match cli.try_get_matches_from(&new_args) {
                    Ok(m) => m,
                    Err(e2) => e2.exit(),
                }
            } else {
                // Not a case where we should inject convert, show original error
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
        _ => {
            let config = builder.load().unwrap_or_else(|e| {
                eprintln!("Failed to load configuration: {e}");
                std::process::exit(1);
            });

            match matches.subcommand() {
                Some(("inspect", sub_matches)) => {
                    let path = sub_matches
                        .get_one::<String>("path")
                        .expect("path is required");
                    let transform = sub_matches
                        .get_one::<String>("transform")
                        .map(|s| s.as_str())
                        .unwrap_or("ast-treeviz");
                    handle_inspect_command(path, transform, &config);
                }
                Some(("convert", sub_matches)) => {
                    let input = sub_matches
                        .get_one::<String>("input")
                        .expect("input is required");
                    let from_arg = sub_matches.get_one::<String>("from");
                    let to = sub_matches.get_one::<String>("to").expect("to is required");

                    // Auto-detect --from if not provided
                    let from = if let Some(f) = from_arg {
                        f.to_string()
                    } else {
                        let registry = FormatRegistry::default();
                        match registry.detect_format_from_filename(input) {
                            Some(detected) => detected,
                            None => {
                                eprintln!("Error: Could not detect format from filename '{input}'");
                                eprintln!("Please specify --from explicitly");
                                std::process::exit(1);
                            }
                        }
                    };

                    let output = sub_matches.get_one::<String>("output").map(|s| s.as_str());
                    handle_convert_command(input, &from, to, output, &config);
                }
                Some(("format", sub_matches)) => {
                    let input = sub_matches
                        .get_one::<String>("input")
                        .expect("input is required");
                    // Format command always outputs to stdout (no -o flag)
                    handle_convert_command(input, "lex", "lex", None, &config);
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
                _ => {
                    eprintln!("Unknown subcommand. Use --help for usage information.");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Build a clapfig builder with search paths and CLI overrides from parsed args.
fn make_builder(matches: &ArgMatches) -> ClapfigBuilder<LexConfig> {
    let mut builder = Clapfig::builder::<LexConfig>()
        .app_name("lex")
        .file_name(CONFIG_FILE_NAME)
        .search_paths(vec![
            SearchPath::Platform,
            SearchPath::Ancestors(Boundary::Marker(".git")),
            SearchPath::Cwd,
        ])
        .persist_scope("local", SearchPath::Cwd)
        .persist_scope("user", SearchPath::Platform);

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

/// Handle the inspect command
fn handle_inspect_command(path: &str, transform: &str, config: &LexConfig) {
    let source = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading file '{path}': {e}");
        std::process::exit(1);
    });

    let params = build_inspect_params(config);

    let output = transforms::execute_transform(&source, transform, &params).unwrap_or_else(|e| {
        eprintln!("Execution error: {e}");
        std::process::exit(1);
    });

    print!("{output}");
}

/// Handle the convert command
fn handle_convert_command(
    input: &str,
    from: &str,
    to: &str,
    output: Option<&str>,
    config: &LexConfig,
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

    // Read input file
    let source = fs::read_to_string(input).unwrap_or_else(|e| {
        eprintln!("Error reading file '{input}': {e}");
        std::process::exit(1);
    });

    // Parse
    let doc = registry.parse(&source, from).unwrap_or_else(|e| {
        eprintln!("Parse error: {e}");
        std::process::exit(1);
    });

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

    println!("\nConversion formats:");
    let registry = FormatRegistry::default();
    for format_name in registry.list_formats() {
        println!("  {format_name}");
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
        Clapfig::builder::<LexConfig>()
            .app_name("lex")
            .no_env()
            .search_paths(vec![])
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
