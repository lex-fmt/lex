//! Main module for lex library functionality
//!
//!     This module orchestrates the complete lex parsing pipeline. Lex is a simple format,
//!     and yet quite hard to parse. Tactically it is stateful, recursive, line based and
//!     indentation significant. The combination of these makes it a parsing nightmare.
//!
//!     While these are all true, the format is designed with enough constraints so that,
//!     if correctly implemented, it's quite easy to parse. However it does mean that using
//!     available libraries simply won't work. Libraries can handle context free, token
//!     based, non indentation significant grammars. At best, they are flexible enough to
//!     handle one of these patterns, but never all of them.
//!
//! The Parser Design
//!
//!     After significant research and experimentation we settled on a design that is a bit
//!     off-the-beaten-path, but nicely breaks down complexity into very simple chunks.
//!
//!     Instead of a straight lexing -> parsing pipeline, lex-parser does the following steps:
//!
//!         1. Semantic Indentation: we convert indent tokens into semantic events as indent
//!            and dedent. This is a stateful machine that tracks changes in indentation
//!            levels and emits indent and dedent events. See
//!            [semantic_indentation](lexing::transformations::semantic_indentation).
//!
//!         2. Line Grouping: we group tokens into lines. Here we split tokens by line breaks
//!            into groups of tokens. Each group is a Line token and which category is
//!            determined by the tokens inside. See [line_grouping](lexing::line_grouping).
//!
//!         3. Tree Building (LineContainer): we build a tree of line groups reflecting the
//!            nesting structure. This groups line tokens into a hierarchical tree structure
//!            based on Indent/Dedent markers. See [to_line_container](token::to_line_container).
//!
//!         4. Context Injection: we inject context information into each group allowing parsing
//!            to only read each level's lines. For example, sessions require preceding blank
//!            lines, but for a session that is the first element in its parent, that preceding
//!            blank line belongs to the parent. A synthetic token is injected to capture this
//!            context.
//!
//!         5. Parsing by Level: parsing only needs to read each level's lines, which can
//!            include a LineContainer (that is, there is child content there), with no tree
//!            traversal needed. Parsing is done declaratively by processing the grammar patterns
//!            (regular strings) through rust's regex engine. See [parsing](parsing) module.
//!
//!     On their own, each step is fairly simple, their total sum being some 500 lines of code.
//!     Additionally they are easy to test and verify.
//!
//!     The key here is that parsing only needs to read each level's line, which can include
//!     a LineContainer (that is, there is child content there), with no tree traversal needed.
//!     Parsing is done declaratively by processing the grammar patterns (regular strings)
//!     through rust's regex engine. Put another way, once tokens are grouped into a tree of
//!     lines, parsing can be done in a regular single pass.
//!
//!     Whether passes 2-4 are indeed lexing or actual parsing is left as a bike shedding
//!     exercise. The criteria for calling these lexing has been that each transformation is
//!     simply a grouping of tokens, there is no semantics.
//!
//! Pipeline Separation
//!
//!     In addition to the transformations over tokens, the codebase separates the semantic
//!     analysis (in [parsing](parsing)) from the AST building (in [building](building)) and
//!     finally the final document assembly step (in [assembling](assembling)). These are done
//!     with the same intention: keeping complexity localized and shallow at every one of these
//!     layers and making the system more testable. Line grouping and tree building happen at
//!     the parsing stage, after lexing has already produced indent/dedent-aware flat tokens.
//!
//!     For the complete end-to-end pipeline documentation, see [parsing](parsing) module.

pub mod annotation;
pub mod assembling;
pub mod ast;
pub mod building;
pub mod escape;
pub mod formats;
pub mod inlines;
pub mod lexing;
pub mod loader;
pub mod parsing;
pub mod testing;
pub mod token;
pub mod transforms;
