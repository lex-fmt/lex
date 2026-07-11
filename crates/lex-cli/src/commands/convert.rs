//! `lexd convert` / `lexd format` — convert between document formats.

use crate::cli_support::{
    absolutize_path, formatting_rules_from_config, pdf_params_from_config, read_source,
    IncludeOptions, MojibakeScanningLoader,
};
use lex_babel::{transforms::serialize_to_lex_with_rules, FormatRegistry, SerializedDocument};
use lex_config::LexConfig;
use lex_core::lex::builtins;
use lex_core::lex::includes::{resolve_from_source, FsLoader, ResolveConfig};
use lex_core::lex::mojibake::detect_mojibake;
use lex_extension_host::registry::Registry;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) fn handle_convert_command(
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
    // include concept; stdin (`None`, filtered out here) can't anchor
    // relative include paths.
    let include_entry = if from == "lex" && inc.enabled {
        input
    } else {
        None
    };
    let doc = if let Some(input) = include_entry {
        // Canonicalize so resolver-side path comparisons (cycle stack,
        // root-escape prefix check) work in absolute-path space.
        let entry = absolutize_path(&PathBuf::from(input));
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
