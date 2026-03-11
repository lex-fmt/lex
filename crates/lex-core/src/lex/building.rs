//! AST building utilities for parsers
//!
//!     This module provides utilities for building AST nodes from IR nodes. From the IR nodes,
//!     we build the actual AST nodes. During this step, important things happen:
//!
//!         1. We unroll source tokens so that ast nodes have access to token values.
//!         2. The location from tokens is used to calculate the location for the ast node.
//!         3. The location is transformed from byte range to a dual byte range + line:column
//!            position.
//!
//!     At this stage we create the root session tree, which will later be attached to the
//!     [`Document`](crate::lex::ast::Document) during assembling.
//!
//! Three-Layer Architecture
//!
//!     The building process follows a three-layer architecture:
//!
//!         1. Token Normalization - Convert various token formats to standard vectors
//!         2. Data Extraction - Extract primitive data (text, byte ranges) from tokens
//!         3. AST Creation - Convert primitives to AST nodes with ast::Range
//!
//!     Parsers should primarily use the [api](api) module which provides the public API.
//!
//!     See [location](location) for location tracking utilities, and [ast_tree](ast_tree) for
//!     the IR nodes to AST nodes conversion.

pub mod api;
pub mod ast_tree;
pub mod location;

pub(super) mod ast_nodes;
pub(super) mod extraction;

// Re-export public API
pub use api::*;
