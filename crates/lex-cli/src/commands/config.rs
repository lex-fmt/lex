//! `lexd config gen` — augment clapfig's template with an extension
//! diagnostic-code discovery channel.

use crate::cli_support::find_nearest_lex_toml_dir;
use clap::ArgMatches;
use clapfig::{Boundary, Clapfig, ConfigAction, SchemaConfigBuilder, SearchPath};
use lex_config::{LexConfig, CONFIG_FILE_NAME};
use std::fs;
use std::path::{Path, PathBuf};

/// Build a clapfig builder with search paths and CLI overrides from
/// parsed args. `accept_dotted_extension_keys_in` lets extension-
/// emitted diagnostic codes under `[diagnostics.rules]` pass strict-
/// mode validation without errors. The CLI doesn't consume the
/// collected entries — only the LSP runs `apply_rules` — so they're
/// ignored at load time.
pub(crate) fn make_builder(matches: &ArgMatches) -> SchemaConfigBuilder<LexConfig> {
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

/// Dispatch `lexd config gen`.
///
/// Augments clapfig's schema-derived template with a "discovery
/// channel" (#659 / #707): after the static sample, lexd boots the
/// extension registry from the workspace `[labels]` block (and any
/// `--ext-schema` flags) and, for every registered namespace, appends
/// a commented-out `[diagnostics.rules]` entry per *declared*
/// diagnostic code. Each line is annotated with the code's declared
/// `description` and `default_severity` so users see concrete lines to
/// uncomment instead of guessing at what codes a namespace emits.
///
/// The rule *value* itself is the user-facing `[diagnostics.rules]`
/// severity vocabulary (`allow` / `warn` / `deny`), defaulting to
/// `warn`; the declared `default_severity` (an LSP-level
/// `error`/`warning`/`info`/`hint`) is surfaced in the comment only,
/// since it is declaration metadata, not a rule override value.
pub(crate) fn handle_config_gen(
    builder: SchemaConfigBuilder<LexConfig>,
    matches: &ArgMatches,
    output: Option<&Path>,
) {
    // Base template from clapfig (schema-derived commented sample).
    let mut rendered = builder
        .handle_to_string(&ConfigAction::Gen { output: None })
        .unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        });

    // Boot the registry the same way `lexd labels` does so the
    // generated template reflects the namespaces actually visible to
    // this workspace.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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
            eprintln!("lexd config gen: {e}");
            std::process::exit(1);
        }
    };
    let ext_schemas: Vec<PathBuf> = matches
        .get_many::<PathBuf>("ext-schema")
        .map(|values| values.cloned().collect())
        .unwrap_or_default();
    let enable_handlers = matches.get_flag("enable-handlers");
    let outcome = lexd::extension_setup::boot_registry(lexd::extension_setup::ExtensionSetup {
        workspace_root: &workspace,
        labels_config: &labels_config,
        ext_schemas: &ext_schemas,
        enable_handlers,
        surface_override: None,
    });

    // Surface non-fatal boot problems (unresolvable namespaces, trust-gate
    // denials, …) to stderr — otherwise a namespace that failed to register
    // would silently drop its declared codes from the discovery section below.
    for diag in &outcome.diagnostics {
        match &diag.namespace {
            Some(ns) => eprintln!("lexd config gen: {ns}: {}", diag.message),
            None => eprintln!("lexd config gen: {}", diag.message),
        }
    }

    if let Some(section) = render_declared_diagnostics_section(&outcome) {
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push('\n');
        rendered.push_str(&section);
    }

    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        eprintln!("lexd config gen: {}: {e}", parent.display());
                        std::process::exit(1);
                    }
                }
            }
            if let Err(e) = fs::write(path, &rendered) {
                eprintln!("lexd config gen: {}: {e}", path.display());
                std::process::exit(1);
            }
            println!("Wrote {}", path.display());
        }
        None => print!("{rendered}"),
    }
}

/// Build the commented `[diagnostics.rules]` discovery section from a
/// booted registry, or `None` when no registered namespace declares
/// any diagnostic codes (so `config gen` output is unchanged for the
/// common no-extension case).
fn render_declared_diagnostics_section(
    outcome: &lexd::extension_setup::BootOutcome,
) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();

    for ns in &outcome.registered {
        let Some(declared) = outcome.registry.declared_diagnostic_codes(&ns.name) else {
            continue;
        };
        if declared.is_empty() {
            continue;
        }

        let mut block = format!("# {} (declared diagnostic codes)\n", ns.name);
        for decl in &declared {
            if let Some(desc) = &decl.description {
                // Comment EVERY line — a multi-line description (e.g. a YAML
                // block scalar) would otherwise leave its continuation lines
                // bare and produce invalid TOML once an entry is uncommented.
                for line in desc.lines() {
                    block.push_str(&format!("# {line}\n"));
                }
            }
            block.push_str(&format!(
                "# default severity: {}\n",
                declared_severity_str(decl.default_severity)
            ));
            block.push_str(&format!("# \"{}.{}\" = \"warn\"\n", ns.name, decl.code));
        }
        blocks.push(block);
    }

    if blocks.is_empty() {
        return None;
    }

    // The discovery section is fully COMMENTED and emits NO `[diagnostics.rules]`
    // table header: the base clapfig template already defines that table, so a
    // second header (or a top-level dotted key) would redefine it — invalid TOML.
    // Users copy an entry up under the existing `[diagnostics.rules]` table.
    let mut section = String::from(
        "# Extension diagnostic rules\n\
         # The extension diagnostic codes below can be overridden under the\n\
         # `[diagnostics.rules]` table above — copy an entry there and set its\n\
         # severity. Allowed values: \"allow\", \"warn\", \"deny\".\n",
    );
    section.push_str(&blocks.join("#\n"));
    Some(section)
}

/// Render a declared (`DiagnosticDecl`) LSP-level severity as the
/// lowercase string used in `config gen` annotations.
fn declared_severity_str(severity: lex_extension::DiagnosticSeverity) -> &'static str {
    use lex_extension::DiagnosticSeverity;
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Hint => "hint",
        _ => "info",
    }
}
