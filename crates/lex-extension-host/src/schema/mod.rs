//! Schema YAML loader and validator.
//!
//! Reads schema files (per the YAML format documented in *Extending Lex*
//! §13.2) from disk into the typed `lex_extension::Schema` structs and
//! enforces the post-deserialise invariants the type system can't express
//! on its own (enum value uniqueness, attachment-kind whitelist, verbatim
//! label legality, transport rules, …).
//!
//! Forward compatibility lives at the `wire_version` axis. The schema
//! itself is *strict*: unknown fields are rejected at load time. Adding a
//! new schema-format feature is a `schema_version` bump; this loader
//! refuses anything it doesn't understand rather than silently dropping
//! information.

pub mod loader;

pub use loader::{SchemaError, SchemaLoader};
