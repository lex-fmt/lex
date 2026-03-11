use clap::{Arg, ArgAction, Command, ValueHint};
use clap_complete::{generate_to, shells::*};
use std::env;
use std::io::Error;

// Mirror of the transforms from src/transforms.rs
// We need to duplicate this here since build scripts can't access src/ modules
const AVAILABLE_TRANSFORMS: &[&str] = &[
    "token-core-json",
    "token-core-simple",
    "token-core-pprint",
    "token-simple",
    "token-pprint",
    "token-line-json",
    "token-line-simple",
    "token-line-pprint",
    "ir-json",
    "ast-json",
    "ast-tag",
    "ast-treeviz",
];

fn main() -> Result<(), Error> {
    let outdir = match env::var_os("OUT_DIR") {
        None => return Ok(()),
        Some(outdir) => outdir,
    };

    let mut cmd = Command::new("lex")
        .version(env!("CARGO_PKG_VERSION"))
        .about("A tool for inspecting and processing lex files")
        .arg_required_else_help(true)
        .arg(
            Arg::new("path")
                .help("Path to the lex file")
                .required_unless_present("list-transforms")
                .index(1)
                .value_hint(ValueHint::FilePath),
        )
        .arg(
            Arg::new("transform")
                .help("Transform to apply (stage-format, e.g., 'ast-tag', 'token-core-json')")
                .required_unless_present("list-transforms")
                .value_parser(clap::builder::PossibleValuesParser::new(
                    AVAILABLE_TRANSFORMS,
                ))
                .index(2)
                .value_hint(ValueHint::Other),
        )
        .arg(
            Arg::new("list-transforms")
                .long("list-transforms")
                .help("List available transforms")
                .action(ArgAction::SetTrue),
        );

    // Generate completions for bash
    generate_to(Bash, &mut cmd, "lex", &outdir)?;

    // Generate completions for zsh
    generate_to(Zsh, &mut cmd, "lex", &outdir)?;

    // Generate completions for fish
    generate_to(Fish, &mut cmd, "lex", &outdir)?;

    println!("cargo:warning=Shell completions generated in {outdir:?}");

    Ok(())
}
