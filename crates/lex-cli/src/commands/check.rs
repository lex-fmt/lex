//! `lexd check` — lint documents and report diagnostics (CI-friendly).

use clap::ArgMatches;
use lex_config::LexConfig;
use std::path::PathBuf;

/// Dispatch `lexd check [FILES...]`. Runs lex-analysis diagnostics over
/// each document's include-expanded AST and reports findings with the
/// CI-friendly exit-code contract (0 clean / 1 findings / 2 operational).
///
/// The actual collection + reporting lives in [`lexd::check`]; this
/// handler only assembles [`lexd::check::CheckOptions`] from parsed args
/// and the loaded config. `[diagnostics.rules]` rides on the
/// already-loaded [`LexConfig`]. `[labels]` is deliberately NOT loaded
/// here: `check` loads it *per entry* in
/// [`lexd::check::collect_file_diagnostics`] from each file's own
/// workspace (`workspace_for(entry)`), so a file outside the CWD's
/// workspace gets its own workspace's labels instead of a mismatched
/// CWD config.
pub(crate) fn handle_check_command(top: &ArgMatches, sub: &ArgMatches, config: &LexConfig) -> i32 {
    use lexd::check::{run, CheckOptions, OutputFormat, Severity};

    let paths: Vec<PathBuf> = sub
        .get_many::<String>("paths")
        .map(|vals| vals.map(PathBuf::from).collect())
        .unwrap_or_default();

    // clap's value_parser already constrains these to the legal token
    // set, so the `expect`s below cannot trip in practice.
    let fail_on = Severity::parse(
        sub.get_one::<String>("fail-on")
            .map(String::as_str)
            .unwrap_or("warning"),
    )
    .expect("clap value_parser constrains --fail-on");
    let format = match sub.get_one::<String>("format").map(String::as_str) {
        Some("json") => OutputFormat::Json,
        _ => OutputFormat::Human,
    };

    let expand_includes = !top.get_flag("no-includes");
    let check_references = sub.get_flag("references");

    // `[labels]` is NOT loaded here: `check` loads it per-entry from each
    // file's own workspace (see `check::collect_file_diagnostics`), so a
    // file outside the CWD's workspace gets its own workspace's labels
    // rather than a mismatched CWD config. Only the `--ext-schema` /
    // `--enable-handlers` knobs and the include settings ride on opts.
    let ext_schemas: Vec<PathBuf> = top
        .get_many::<PathBuf>("ext-schema")
        .map(|values| values.cloned().collect())
        .unwrap_or_default();
    let enable_handlers = top.get_flag("enable-handlers");

    let includes_root = top
        .get_one::<String>("includes-root")
        .map(PathBuf::from)
        .or_else(|| config.includes.root.as_ref().map(PathBuf::from));

    let opts = CheckOptions {
        expand_includes,
        includes_root,
        max_depth: config.includes.max_depth,
        max_total_includes: config.includes.max_total_includes,
        max_file_size: config.includes.max_file_size,
        fail_on,
        format,
        rules: &config.diagnostics.rules,
        ext_schemas: &ext_schemas,
        enable_handlers,
        check_references,
    };

    run(&paths, &opts)
}
