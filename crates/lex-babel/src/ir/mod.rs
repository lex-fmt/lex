//! Intermediate Representation (IR) for lex documents.
//!
//! This module defines a format-agnostic representation of a lex document,
//! designed to facilitate conversion to various output formats like HTML,
//! Markdown, etc.

pub mod events;
pub mod from_lex;
pub mod nodes;
pub mod to_events;
pub mod to_lex;
