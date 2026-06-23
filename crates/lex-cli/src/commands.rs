//! Command handlers for the `lexd` binary.
//!
//! Each submodule owns the `handle_*` entry point(s) for one command group,
//! plus that command's private helpers. `main` parses args, then dispatches
//! to the re-exports below. Handlers pull shared helpers (include options,
//! path/source utilities, config→params translators) from
//! [`crate::cli_support`].
//!
//! - [`check`] — `lexd check`
//! - [`config`] — `lexd config gen` augmentation
//! - [`labels`] — `lexd labels {list,validate,emit}`
//! - [`inspect`] — `lexd inspect`
//! - [`convert`] — `lexd convert` / `lexd format`
//! - [`query`] — `lexd element-at` / `lexd token-at`
//! - [`misc`] — `lexd generate-lex-css` / `--list-transforms`

mod check;
mod config;
mod convert;
mod inspect;
mod labels;
mod misc;
mod query;

pub(crate) use check::handle_check_command;
pub(crate) use config::{handle_config_gen, make_builder};
pub(crate) use convert::handle_convert_command;
pub(crate) use inspect::handle_inspect_command;
pub(crate) use labels::handle_labels_command;
pub(crate) use misc::{handle_generate_lex_css_command, handle_list_transforms_command};
pub(crate) use query::{handle_element_at_command, handle_token_at_command};
