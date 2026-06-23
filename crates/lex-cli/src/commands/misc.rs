//! `lexd generate-lex-css` and `--list-transforms` — small standalone outputs.

use crate::cli_support::format_tier;
use lex_babel::FormatRegistry;
use lexd::transforms;

/// Handle the generate-lex-css command
pub(crate) fn handle_generate_lex_css_command() {
    print!("{}", lex_babel::formats::get_default_css());
}

/// Handle the list-transforms command
pub(crate) fn handle_list_transforms_command() {
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
