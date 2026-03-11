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
//
// Extra Parameters:
//
// Format-specific parameters can be passed using --extra-<parameter-name> <value>.
// The CLI layer strips the "extra-" prefix and passes the parameters to the format/transform.
// Example:
//  lex inspect file.lex --extra-all-nodes true --extra-max-depth 5

use lex_cli::transforms;

use clap::{Arg, ArgAction, Command, ValueHint};
use lex_babel::{
    formats::lex::formatting_rules::FormattingRules, transforms::serialize_to_lex_with_rules,
    FormatRegistry, SerializedDocument,
};
use lex_config::{LexConfig, Loader, PdfPageSize};
use lex_core::lex::ast::{find_node_path_at_position, Position};
use std::collections::HashMap;
use std::fs;

/// Parse extra-* arguments from command line args
/// Returns (cleaned_args_without_extras, extra_params_map)
///
/// Supports both:
/// - `--extra-<key> <value>` (explicit value)
/// - `--extra-<key>` (boolean flag, defaults to "true")
/// - `--extras-<key>` (alias for `--extra-<key>`)
fn parse_extra_args(args: &[String]) -> (Vec<String>, HashMap<String, String>) {
    let mut cleaned_args = Vec::new();
    let mut extra_params = HashMap::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        let key_opt = if let Some(key) = arg.strip_prefix("--extra-") {
            Some(key)
        } else {
            arg.strip_prefix("--extras-")
        };

        if let Some(key) = key_opt {
            // Found an extra-* argument
            // Check if the next arg is a value or another flag/end
            let has_value = if i + 1 < args.len() {
                let next = &args[i + 1];
                !next.starts_with('-') && !next.starts_with("--")
            } else {
                false
            };

            if has_value {
                // Explicit value provided
                extra_params.insert(key.to_string(), args[i + 1].clone());
                i += 2; // Skip both the key and value
            } else {
                // No value, treat as boolean flag (default to "true")
                extra_params.insert(key.to_string(), "true".to_string());
                i += 1;
            }
            continue;
        }

        cleaned_args.push(arg.clone());
        i += 1;
    }

    (cleaned_args, extra_params)
}

fn build_cli() -> Command {
    Command::new("lex")
        .version(env!("CARGO_PKG_VERSION"))
        .about("A tool for inspecting and converting lex files")
        .long_about(
            "lex is a command-line tool for working with lex document files.\n\n\
            Commands:\n  \
            - inspect: View internal representations (tokens, AST, etc.)\n  \
            - convert: Transform between document formats (lex, markdown, HTML, etc.)\n\n\
            Extra Parameters:\n  \
            Use --extra-<name> [value] to pass format-specific options.\n  \
            Boolean flags can omit the value (defaults to 'true').\n\n\
            Examples:\n  \
            lex inspect file.lex                    # View AST tree visualization\n  \
            lex inspect file.lex ast-tag            # View AST as XML tags\n  \
            lex inspect file.lex --extra-ast-full   # Show complete AST (all node properties)\n  \
            lex file.lex --to markdown              # Convert to markdown (outputs to stdout)\n  \
            lex file.lex --to html -o output.html   # Convert to HTML file"
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
            Arg::new("config")
                .long("config")
                .value_name("PATH")
                .help("Path to a lex.toml configuration file")
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
                    Extra Parameters:\n  \
                    --extra-ast-full      Show complete AST including:\n                          \
                    * Document-level annotations\n                          \
                    * All node properties (labels, subjects, parameters)\n                          \
                    * Session titles, list markers, definition subjects\n\n\
                    Examples:\n  \
                    lex inspect file.lex                     # Tree visualization (default)\n  \
                    lex inspect file.lex ast-tag             # XML-like output\n  \
                    lex inspect file.lex --extra-ast-full    # Complete AST with all properties\n  \
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
            Command::new("generate-lex-css")
                .about("Output the default CSS used for HTML export")
                .long_about(
                    "Outputs the default baseline CSS used when converting to HTML.\n\n\
                    Use this as a starting point for custom styling. The output can be\n\
                    saved to a file and customized, then passed via --extra-css to the\n\
                    convert command to extend the default styles.\n\n\
                    Examples:\n  \
                    lex generate-lex-css                    # Print CSS to stdout\n  \
                    lex generate-lex-css > custom.css       # Save to file for editing"
                ),
        )
}

fn main() {
    // Try to parse args. If no subcommand is provided, inject "convert"
    let args: Vec<String> = std::env::args().collect();

    // Parse extra-* arguments before clap processing
    let (cleaned_args, mut extra_params) = parse_extra_args(&args);

    // First, try normal parsing with cleaned args
    let cli = build_cli();
    let matches = match cli.clone().try_get_matches_from(&cleaned_args) {
        Ok(m) => m,
        Err(e) => {
            // Check if this is a "missing subcommand" error by seeing if the first arg looks like a file
            if cleaned_args.len() > 1
                && !cleaned_args[1].starts_with('-')
                && cleaned_args[1] != "inspect"
                && cleaned_args[1] != "convert"
                && cleaned_args[1] != "generate-lex-css"
                && cleaned_args[1] != "help"
            {
                // Inject "convert" as the subcommand
                let mut new_args = vec![cleaned_args[0].clone(), "convert".to_string()];
                new_args.extend_from_slice(&cleaned_args[1..]);

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

    let mut config = load_cli_config(matches.get_one::<String>("config").map(|s| s.as_str()));
    apply_config_overrides(&mut config, &mut extra_params);

    match matches.subcommand() {
        Some(("inspect", sub_matches)) => {
            let path = sub_matches
                .get_one::<String>("path")
                .expect("path is required");
            let transform = sub_matches
                .get_one::<String>("transform")
                .map(|s| s.as_str())
                .unwrap_or("ast-treeviz");
            handle_inspect_command(path, transform, &extra_params, &config);
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
            handle_convert_command(input, &from, to, output, &extra_params, &config);
        }
        Some(("format", sub_matches)) => {
            let input = sub_matches
                .get_one::<String>("input")
                .expect("input is required");
            // Format command always outputs to stdout (no -o flag)
            handle_convert_command(input, "lex", "lex", None, &extra_params, &config);
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
        Some(("generate-lex-css", _)) => {
            handle_generate_lex_css_command();
        }
        _ => {
            eprintln!("Unknown subcommand. Use --help for usage information.");
            std::process::exit(1);
        }
    }
}

/// Handle the inspect command (old execute command)
fn handle_inspect_command(
    path: &str,
    transform: &str,
    extra_params: &HashMap<String, String>,
    config: &LexConfig,
) {
    let source = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading file '{path}': {e}");
        std::process::exit(1);
    });

    let params = build_inspect_params(config, extra_params);

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
    extra_params: &HashMap<String, String>,
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

    // Serialize (format-specific parameters allowed via --extra-*)
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
        for (key, value) in extra_params {
            format_options.insert(key.clone(), value.clone());
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
        // If no element found, we might want to print something or just exit
        // The requirement says "returns the element name..."
        // If nothing found, maybe print nothing or error?
        // I'll print a message for now.
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

fn load_cli_config(explicit_path: Option<&str>) -> LexConfig {
    let loader = Loader::new().with_optional_file("lex.toml");
    let loader = if let Some(path) = explicit_path {
        loader.with_file(path)
    } else {
        loader
    };

    loader.build().unwrap_or_else(|err| {
        eprintln!("Failed to load configuration: {err}");
        std::process::exit(1);
    })
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

fn apply_config_overrides(config: &mut LexConfig, extra_params: &mut HashMap<String, String>) {
    if let Some(raw) = extra_params.remove("ast-full") {
        config.inspect.ast.include_all_properties = parse_bool_arg("ast-full", &raw);
    }
    if let Some(raw) = extra_params.remove("show-linum") {
        config.inspect.ast.show_line_numbers = parse_bool_arg("show-linum", &raw);
    }

    if let Some(raw) = take_override(extra_params, &["color"]) {
        config.inspect.nodemap.color_blocks = parse_bool_arg("color", &raw);
    }
    if let Some(raw) = take_override(extra_params, &["colorchar", "color-char"]) {
        config.inspect.nodemap.color_characters = parse_bool_arg("color-char", &raw);
    }
    if let Some(raw) = take_override(extra_params, &["nodesummary", "node-summary"]) {
        config.inspect.nodemap.show_summary = parse_bool_arg("nodesummary", &raw);
    }

    let mut pdf_override = None;
    if let Some(raw) = extra_params.remove("size-mobile") {
        if parse_bool_arg("size-mobile", &raw) {
            pdf_override = Some(PdfPageSize::Mobile);
        }
    }
    if let Some(raw) = extra_params.remove("size-lexed") {
        if parse_bool_arg("size-lexed", &raw) {
            if let Some(existing) = pdf_override {
                eprintln!("Conflicting PDF profile overrides: {existing:?} and lexed");
                std::process::exit(1);
            }
            pdf_override = Some(PdfPageSize::LexEd);
        }
    }

    if let Some(size) = pdf_override {
        config.convert.pdf.size = size;
    }

    if let Some(raw) = take_override(extra_params, &["theme"]) {
        config.convert.html.theme = raw;
    }

    if let Some(path) = take_override(extra_params, &["css", "css-path"]) {
        config.convert.html.custom_css = Some(path);
    }
}

fn build_inspect_params(
    config: &LexConfig,
    overrides: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut params = HashMap::new();

    if config.inspect.ast.include_all_properties {
        params.insert("ast-full".to_string(), "true".to_string());
    }

    params.insert(
        "show-linum".to_string(),
        if config.inspect.ast.show_line_numbers {
            "true".to_string()
        } else {
            "false".to_string()
        },
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

    for (key, value) in overrides {
        params.insert(key.clone(), value.clone());
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

fn take_override(map: &mut HashMap<String, String>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = map.remove(*key) {
            return Some(value);
        }
    }
    None
}

fn parse_bool_arg(flag: &str, raw: &str) -> bool {
    match raw.to_lowercase().as_str() {
        "true" | "1" | "yes" | "y" => true,
        "false" | "0" | "no" | "n" => false,
        other => {
            eprintln!("Invalid boolean value '{other}' for --extra-{flag}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_extra_args_empty() {
        let args = vec![
            "lex".to_string(),
            "inspect".to_string(),
            "file.lex".to_string(),
        ];
        let (cleaned, extra) = parse_extra_args(&args);

        assert_eq!(cleaned, args);
        assert!(extra.is_empty());
    }

    #[test]
    fn test_parse_extra_args_single_param() {
        let args = vec![
            "lex".to_string(),
            "inspect".to_string(),
            "file.lex".to_string(),
            "--extra-all-nodes".to_string(),
            "true".to_string(),
        ];
        let (cleaned, extra) = parse_extra_args(&args);

        assert_eq!(
            cleaned,
            vec![
                "lex".to_string(),
                "inspect".to_string(),
                "file.lex".to_string()
            ]
        );
        assert_eq!(extra.len(), 1);
        assert_eq!(extra.get("all-nodes"), Some(&"true".to_string()));
    }

    #[test]
    fn test_parse_extra_args_multiple_params() {
        let args = vec![
            "lex".to_string(),
            "inspect".to_string(),
            "file.lex".to_string(),
            "--extra-all-nodes".to_string(),
            "true".to_string(),
            "ast-treeviz".to_string(),
            "--extra-max-depth".to_string(),
            "5".to_string(),
        ];
        let (cleaned, extra) = parse_extra_args(&args);

        assert_eq!(
            cleaned,
            vec![
                "lex".to_string(),
                "inspect".to_string(),
                "file.lex".to_string(),
                "ast-treeviz".to_string()
            ]
        );
        assert_eq!(extra.len(), 2);
        assert_eq!(extra.get("all-nodes"), Some(&"true".to_string()));
        assert_eq!(extra.get("max-depth"), Some(&"5".to_string()));
    }

    #[test]
    fn test_parse_extra_args_mixed_with_regular_args() {
        let args = vec![
            "lex".to_string(),
            "convert".to_string(),
            "input.lex".to_string(),
            "--to".to_string(),
            "html".to_string(),
            "--extra-theme".to_string(),
            "dark".to_string(),
            "--from".to_string(),
            "lex".to_string(),
        ];
        let (cleaned, extra) = parse_extra_args(&args);

        assert_eq!(
            cleaned,
            vec![
                "lex".to_string(),
                "convert".to_string(),
                "input.lex".to_string(),
                "--to".to_string(),
                "html".to_string(),
                "--from".to_string(),
                "lex".to_string()
            ]
        );
        assert_eq!(extra.len(), 1);
        assert_eq!(extra.get("theme"), Some(&"dark".to_string()));
    }

    #[test]
    fn test_parse_extra_args_boolean_flag() {
        let args = vec![
            "lex".to_string(),
            "inspect".to_string(),
            "file.lex".to_string(),
            "ast-tag".to_string(),
            "--extra-ast-full".to_string(),
        ];
        let (cleaned, extra) = parse_extra_args(&args);

        assert_eq!(
            cleaned,
            vec![
                "lex".to_string(),
                "inspect".to_string(),
                "file.lex".to_string(),
                "ast-tag".to_string()
            ]
        );
        assert_eq!(extra.len(), 1);
        assert_eq!(extra.get("ast-full"), Some(&"true".to_string()));
    }

    #[test]
    fn test_parse_extra_args_boolean_flag_at_end() {
        let args = vec![
            "lex".to_string(),
            "inspect".to_string(),
            "file.lex".to_string(),
            "--extra-verbose".to_string(),
        ];
        let (cleaned, extra) = parse_extra_args(&args);

        assert_eq!(
            cleaned,
            vec![
                "lex".to_string(),
                "inspect".to_string(),
                "file.lex".to_string()
            ]
        );
        assert_eq!(extra.len(), 1);
        assert_eq!(extra.get("verbose"), Some(&"true".to_string()));
    }

    #[test]
    fn test_parse_extra_args_allows_extras_alias() {
        let args = vec![
            "lex".to_string(),
            "convert".to_string(),
            "doc.lex".to_string(),
            "--extras-css-path".to_string(),
            "styles.css".to_string(),
        ];

        let (cleaned, extra) = parse_extra_args(&args);
        assert_eq!(
            cleaned,
            vec![
                "lex".to_string(),
                "convert".to_string(),
                "doc.lex".to_string()
            ]
        );
        assert_eq!(extra.get("css-path"), Some(&"styles.css".to_string()));
    }

    #[test]
    fn test_parse_extra_args_mixed_boolean_and_value() {
        let args = vec![
            "lex".to_string(),
            "inspect".to_string(),
            "file.lex".to_string(),
            "--extra-verbose".to_string(),
            "--extra-max-depth".to_string(),
            "5".to_string(),
            "--extra-compact".to_string(),
        ];
        let (cleaned, extra) = parse_extra_args(&args);

        assert_eq!(
            cleaned,
            vec![
                "lex".to_string(),
                "inspect".to_string(),
                "file.lex".to_string()
            ]
        );
        assert_eq!(extra.len(), 3);
        assert_eq!(extra.get("verbose"), Some(&"true".to_string()));
        assert_eq!(extra.get("max-depth"), Some(&"5".to_string()));
        assert_eq!(extra.get("compact"), Some(&"true".to_string()));
    }

    #[test]
    fn apply_config_overrides_updates_known_flags() {
        let mut config = load_cli_config(None);
        let mut extras = HashMap::new();
        extras.insert("ast-full".to_string(), "true".to_string());
        extras.insert("color".to_string(), "false".to_string());
        extras.insert("size-mobile".to_string(), "true".to_string());

        apply_config_overrides(&mut config, &mut extras);

        assert!(config.inspect.ast.include_all_properties);
        assert!(!config.inspect.nodemap.color_blocks);
        assert_eq!(config.convert.pdf.size, PdfPageSize::Mobile);
        assert!(extras.is_empty());
    }

    #[test]
    fn apply_config_overrides_handles_css_path_overrides() {
        let mut config = load_cli_config(None);
        let mut extras = HashMap::new();
        extras.insert("css-path".to_string(), "custom.css".to_string());

        apply_config_overrides(&mut config, &mut extras);

        assert_eq!(
            config.convert.html.custom_css.as_deref(),
            Some("custom.css")
        );
        assert!(extras.is_empty());
    }

    #[test]
    fn inspect_params_include_configured_defaults() {
        let config = load_cli_config(None);
        let mut overrides = HashMap::new();
        overrides.insert("custom".to_string(), "value".to_string());

        let params = build_inspect_params(&config, &overrides);
        assert_eq!(params.get("show-linum"), Some(&"true".to_string()));
        assert_eq!(params.get("custom"), Some(&"value".to_string()));
    }

    #[test]
    fn pdf_params_follow_configured_profile() {
        let mut config = load_cli_config(None);
        config.convert.pdf.size = PdfPageSize::Mobile;
        let params = pdf_params_from_config(&config);
        assert_eq!(params.get("size-mobile"), Some(&"true".to_string()));
        assert!(!params.contains_key("size-lexed"));
    }
}
