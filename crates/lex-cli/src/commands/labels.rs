//! `lexd labels {list,validate,emit}` — inspect extension namespaces.

use crate::cli_support::find_nearest_lex_toml_dir;
use clap::ArgMatches;
use lex_config::CONFIG_FILE_NAME;
use std::path::PathBuf;

/// Dispatch `lexd labels {list,validate}`. Returns the exit code
/// to propagate.
pub(crate) fn handle_labels_command(top: &ArgMatches, sub: &ArgMatches) -> i32 {
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
