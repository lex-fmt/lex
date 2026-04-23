//! Consolidated property-based test binary.
//!
//! Each submodule used to be its own `tests/*.rs` file (and therefore its own
//! test binary). Consolidating into one binary eliminates N× redundant linking
//! of lex-core and proptest into per-file executables.
//!
//! Each submodule's own `*.proptest-regressions` file sits alongside it here
//! and continues to work: proptest resolves the regression file via the
//! submodule's source path.

mod escape_proptest;
mod lexer_proptest;
mod parameter_proptest;
mod parser_correctness_proptest;
mod parser_proptest;
mod proptest_annotation_attachment;
mod proptest_confusion_boundaries;
mod proptest_inline_edge_cases;
mod proptest_invariants;
mod proptest_nested_lists;
mod proptest_references;
mod proptest_table_cells;
mod proptest_table_config;
mod proptest_verbatim;
mod split_respecting_escape_proptest;
