//! Data Extraction from Tokens
//!
//! This module extracts primitive data (text, byte ranges, metadata) from normalized
//! token vectors. It returns data structures containing only primitives - no AST types.
//!
//! # Architecture
//!
//! ```text
//! Vec<(Token, Range<usize>)> → Data Extraction → Data Structs (primitives only)
//!                               ↓
//!                               - Extract text from source
//!                               - Compute byte range bounding boxes
//!                               - Process tokens intelligently
//!                               ↓
//!                               { text: String, byte_range: Range<usize> }
//! ```
//!
//! # Responsibilities
//!
//! - Extract text from source using token byte ranges
//! - Compute bounding boxes from token ranges
//! - Implement smart token processing (e.g., indentation wall stripping)
//! - Return primitive data structures (String, Range<usize>, etc.)
//! - NO AST types (ast::Range is converted later in ast_creation)
//!
//! # Key Design Principle
//!
//! This layer works with primitives only. Byte ranges stay as `Range<usize>`.
//! The conversion to `ast::Range` happens later in the ast_creation layer.

// Module declarations
mod data;
mod definition;
mod list_item;
mod paragraph;
mod parameter;
mod session;
mod verbatim;

// Re-export data structures and functions
pub(super) use data::{extract_data, DataExtraction};
pub(super) use definition::{extract_definition_data, DefinitionData};
pub(super) use list_item::{extract_list_item_data, ListItemData};
pub(super) use paragraph::{extract_paragraph_data, ParagraphData};
pub(super) use session::{extract_session_data, SessionData};
pub(super) use verbatim::{extract_verbatim_block_data, VerbatimBlockData, VerbatimGroupData};

// Re-export VerbatimGroupTokenLines as public for use in ast_tree.rs
