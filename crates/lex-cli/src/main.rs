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
//
// ## Module layout
//
// This file is the binary root: arg parsing + dispatch only.
//
// - [`cli`] — the clap `Command` tree ([`cli::build_cli`]).
// - [`cli_support`] — cross-command helpers (include options, path/source
//   utilities, config→params translators, the mojibake-scanning loader).
// - [`commands`] — one submodule per command group, each owning its
//   `handle_*` entry point(s).
//
// The library half of this crate (package `lexd`) lives in `lib.rs` and is
// reached via the `lexd::` path (e.g. `lexd::check`, `lexd::transforms`);
// binary-local items are reached via `crate::`.

mod cli;
mod cli_support;
mod commands;

use crate::cli::build_cli;
use crate::cli_support::{warnings_enabled, IncludeOptions};
use crate::commands::{
    handle_check_command, handle_config_gen, handle_convert_command, handle_element_at_command,
    handle_generate_lex_css_command, handle_inspect_command, handle_labels_command,
    handle_list_transforms_command, handle_token_at_command, make_builder,
};
use clapfig::{ConfigAction, ConfigCommand};
use lex_babel::FormatRegistry;
use lexd::transforms;

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
                "check",
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
            // `config gen` is augmented over clapfig's static template:
            // after the schema-derived sample, lexd boots the extension
            // registry and appends a commented-out `[diagnostics.rules]`
            // entry per declared extension diagnostic code (the
            // "discovery channel" of #659 / #707). The other config
            // actions (list/get/set/unset/schema) are plain clapfig.
            if let ConfigAction::Gen { output } = &action {
                handle_config_gen(builder, &matches, output.as_deref());
            } else {
                builder.handle_and_print(&action).unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(1);
                });
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
                Some(("check", sub_matches)) => {
                    let exit = handle_check_command(&matches, sub_matches, &config);
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
