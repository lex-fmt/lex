//! # lex-parser
//!
//!     A parser for the Lex plain text document format.
//!
//! Overview
//!
//!     Lex is a plain text format for structured information that can scale from a quick one-line
//!     note to scientific writings, while being easy to write without tooling. The parser transforms
//!     Lex source text into an abstract syntax tree (AST).
//!
//! Parser Architecture
//!
//!     The parser uses a multi-stage design that breaks down complexity into simple chunks:
//!
//!     1. **Lexing** - Tokenization, semantic indentation, and line classification
//!     2. **Tree Building** - Creates hierarchical LineContainer structure
//!     3. **Parsing** - Pattern-based semantic analysis producing IR nodes
//!     4. **Building** - Constructs AST from IR nodes with location tracking
//!     5. **Assembly** - Attaches annotations and resolves references
//!
//!     This design enables single-pass parsing with each nesting level parsed in isolation.
//!
//! Getting Started
//!
//!     - For the complete parser design and pipeline details, see the [lex](lex) module
//!     - For the end-to-end processing pipeline, see [lex::parsing]
//!     - For AST node types and structure, see [lex::ast]
//!     - For testing guidelines, see [lex::testing]
//!
//! File Layout
//!
//!     For the time being, and probably at times, we will be running multiple lexer and parser
//!     designs side by side. As the code gets more complicated comparing versions is key, and
//!     having them side by side makes this easier, including comparison testing. These versions
//!     might, as they do now, have different lexer outputs and parser inputs The contract is to
//!     have the same global input (the lex source) and the same global output (the AST).
//!
//!     But various designs will make different tradeoffs on what gets done in lexing and parsing,
//!     so we do not commit to a common lexer or parser outputs. But often different designs do
//!     share a significant amount of code.
//!
//!     Hence the layout should be:
//!         src/lex/parser
//!           ├── parser       The current parser design
//!           └── <common>     Shared code for AST building and IR
//!
//!     So the general form is src/lex/parser|lexer|design|common
//!
//! Testing
//!
//!     For comprehensive testing guidelines, see the [testing module](lex::testing).
//!     All parser tests must follow strict rules using verified lex sources and AST assertions.

#![allow(rustdoc::invalid_html_tags)]

pub mod lex;

/// A simple function to demonstrate the library works
pub fn hello() -> &'static str {
    "Hello from lex!"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello() {
        assert_eq!(hello(), "Hello from lex!");
    }
}
