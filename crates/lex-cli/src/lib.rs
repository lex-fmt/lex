//! The `lexd` command-line interface for the Lex toolchain.
//!
//! Exposes the CLI subcommand surface: document checking, label management,
//! format transforms, extension setup, and help.

pub mod check;
pub mod extension_setup;
pub mod help;
pub mod labels_subcommand;
pub mod transforms;
