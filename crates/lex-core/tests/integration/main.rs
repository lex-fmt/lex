//! Consolidated integration-test binary.
//!
//! Each submodule used to be its own `tests/<name>.rs` file (and therefore its
//! own test binary). Consolidating into one binary eliminates N× redundant
//! linking of lex-core into per-file executables.

mod doc_collections;
mod elements_annotations;
mod elements_definitions;
mod elements_lists;
mod elements_paragraphs;
mod elements_sessions;
mod elements_table;
mod elements_verbatim;
mod integration_test;
mod lex_integrations;
mod location_integrity;
mod parser;
mod parser_kitchensink;
mod parser_regression;
mod runtime_type_safety;
mod spec_documents;
mod spec_validation;
mod table_escape_integration;
mod test_blank_line_group_parsing;
mod test_bug;
mod verbatim_dual;
mod verbatim_span_bounds;
