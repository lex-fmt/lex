//! Include resolution for Lex documents.
//!
//! This module turns `:: lex.include src="..." ::` annotations into spliced
//! content from the referenced files. It is *opt-in*: callers that want the
//! unresolved tree (the formatter, tree-sitter parity, editor tooling that
//! displays include statements as authored) skip this pass entirely. The
//! parser itself never touches the filesystem — all I/O goes through the
//! injected [`Loader`] trait.
//!
//! See `comms/specs/proposals/includes.lex` for the full design.
//!
//! # Status
//!
//! This module is being built up across PRs 3–6:
//!
//! - PR 3: skeleton — trait, config, errors, stub.
//! - PR 4: single-pass splice + container-policy validation +
//!   doc-title/doc-annotation conversion + origin stamping + root-escape
//!   check.
//! - PR 5: recursive resolution into included files + cycle detection
//!   (chain stack) + depth limit. Each loaded file gets walked in its OWN
//!   directory, so relative paths inside an included file resolve from
//!   that file's directory, not the entry's.
//! - PR 6: origin-aware reference helpers. [`resolve_file_reference`]
//!   resolves a `ReferenceType::File` target from the authoring file's
//!   directory using `Range.origin_path`.
//!   `Document::find_annotation_by_label_in_origin` scopes footnote
//!   lookups to the file the reference was authored in.
//! - PR 7 (this PR): [`FsLoader`] — production loader that reads from the
//!   filesystem with `std::fs::read_to_string`. CLI wires the resolver
//!   into `lex convert` and `lex inspect` (default-on, opt-out via
//!   `--no-includes`); `lex format` never expands.
//!
//! # Layering
//!
//! Of all of lex-core, only [`FsLoader`] references `std::fs`. The
//! resolver itself does no I/O — it always goes through the [`Loader`]
//! trait. Callers can swap loaders to keep the resolver sandboxed:
//!
//! - The LSP wraps [`FsLoader`] with file-watch invalidation (PR 8).
//! - WASM builds provide a JS-backed loader instead of [`FsLoader`].
//! - Tests use [`MemoryLoader`] (gated behind `test-support`).
//!
//! For tests, lex-core itself ships [`MemoryLoader`] gated behind the
//! `test-support` cargo feature. It is not intended for production use.

// `IncludeError` carries diagnostic context (paths, source ranges,
// handler messages) on every variant; the `result_large_err` lint
// would have us box the whole error or split it into a thinner shape
// just to satisfy the size heuristic. The enum is already part of
// the public API and the error path is rare; suppress the lint for
// this module rather than churn the public surface.
#![allow(clippy::result_large_err)]

// This module is organised as a façade: the public surface
// (`ResolveConfig`, `ContainerKind`, `KERNEL_DEPTH_BACKSTOP`,
// `parse_no_attach`) lives here, and the implementation is split into
// cohesive submodules re-exported at the `includes::` path so callers
// (and crates.io consumers of `lex-core`) see no path change:
//
// - `errors`   — `LoadError` / `IncludeError` and their trait impls.
// - `loaders`  — the `Loader` trait, `LoadedFile`, `FsLoader`, `MemoryLoader`.
// - `resolver` — the resolution engine (`resolve_from_source` + recursive splice).
// - `wire`     — handler wire-payload decoding and origin helpers.
// - `paths`    — include-path / file-reference resolution.
// - `stamp`    — `Range.origin_path` stamping.
mod errors;
mod loaders;
mod paths;
mod resolver;
mod stamp;
mod wire;

pub use errors::{IncludeError, LoadError};
pub use loaders::{FsLoader, LoadedFile, Loader};
pub use paths::resolve_file_reference;
pub use resolver::resolve_from_source;
pub(crate) use stamp::stamp_doc;

#[cfg(any(test, feature = "test-support"))]
pub use loaders::MemoryLoader;

// `paths::{resolve_path, lexical_normalize}` are crate-internal helpers
// the resolver reaches through the `paths` module directly, so they need
// no re-export here. `stamp_doc` is `pub(crate)` because the resolver
// stamps entry-file origins through it.

// The `tests` submodule (`includes/tests.rs`) imports the include surface
// through `use super::*`. Besides the re-exports above it relies on these
// AST types being reachable as `super::{Range, Session}`; surface them for
// the test build only so the public API gains no new paths.
#[cfg(test)]
use crate::lex::ast::elements::session::Session;
#[cfg(test)]
use crate::lex::ast::range::Range;

use crate::lex::ast::Document;
use std::path::PathBuf;

/// Configuration for the include resolution pass.
#[derive(Debug, Clone)]
pub struct ResolveConfig {
    /// Directory all include paths resolve under. Any include that
    /// canonicalizes outside this root is a [`IncludeError::RootEscape`].
    ///
    /// Must be an **absolute** path. Lexical normalization treats `.`
    /// and `..` against an empty buffer as no-ops; passing a relative
    /// or unnormalized root weakens the root-escape prefix check.
    /// Callers (CLI, LSP) should canonicalize the root before
    /// constructing `ResolveConfig`.
    pub root: PathBuf,
    /// Maximum include depth. Default 8 (see [`ResolveConfig::DEFAULT_MAX_DEPTH`]).
    /// Hitting the limit is an error, not a silent truncation.
    pub max_depth: usize,
    /// Maximum total number of `lex.include` annotations resolved across
    /// the whole tree (depth × breadth). Default 1000
    /// (see [`ResolveConfig::DEFAULT_MAX_TOTAL_INCLUDES`]).
    ///
    /// Caps fan-out: `max_depth` alone bounds chain length but not
    /// breadth. A document with 100 thousand top-level includes at depth
    /// 1 sits inside `max_depth` but can still OOM the resolver / LSP /
    /// CI. Hitting this limit is an error, not a silent truncation.
    pub max_total_includes: usize,
}

impl ResolveConfig {
    /// Default maximum include depth — enough for any reasonable atomization
    /// strategy (aggregator → per-chapter → per-section), bounded enough to
    /// keep the resolver's worst-case work predictable.
    pub const DEFAULT_MAX_DEPTH: usize = 8;

    /// Default maximum total include count (DoS bound). Generous enough
    /// for a book-length document with thousands of small fragments,
    /// tight enough to contain adversarial fan-out within a few seconds
    /// of resolver work.
    pub const DEFAULT_MAX_TOTAL_INCLUDES: usize = 1000;

    /// Construct a config with the given root and default limits.
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            root,
            max_depth: Self::DEFAULT_MAX_DEPTH,
            max_total_includes: Self::DEFAULT_MAX_TOTAL_INCLUDES,
        }
    }
}

/// Which container the include site sits in. Determines the splice-time
/// policy check (the only one today is "no Sessions in `GeneralContainer`").
///
/// Defined here in the façade rather than in `resolver` because it is part
/// of the include pass's vocabulary; the resolution engine consumes it via
/// `super::ContainerKind`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ContainerKind {
    /// `Document.root.children` or `Session.children` — accepts everything.
    Session,
    /// `Definition.children` — `GeneralContainer`.
    Definition,
    /// `Annotation.children` — `GeneralContainer`.
    AnnotationBody,
    /// `ListItem.children` — `GeneralContainer`.
    ListItem,
}

impl ContainerKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            ContainerKind::Session => "Session",
            ContainerKind::Definition => "Definition",
            ContainerKind::AnnotationBody => "Annotation body",
            ContainerKind::ListItem => "ListItem",
        }
    }

    pub(crate) fn allows_sessions(self) -> bool {
        matches!(self, ContainerKind::Session)
    }
}

/// Hard cap on resolution depth, applied even when the
/// configurable [`ResolveConfig::max_depth`] is set higher. Bounds
/// adversarial varying-position recursion (a handler that returns
/// content with a different invocation site each iteration so the
/// cycle key never matches) so the resolver always terminates.
pub const KERNEL_DEPTH_BACKSTOP: usize = 32;

// ============================================================================
// Parser glue
// ============================================================================

/// Parse `source` into a Document but skip the annotation-attachment stage,
/// so include annotations are findable in container children lists.
///
/// Runs the shared parser front-end ([`parse_to_attached_root`]) — the same
/// one `run_string_to_ast` and `resolve_from_source` use — so the
/// reference-line pre-pass and any future front-end stage can never drift
/// from the standard path (lex#722). This is used by the built-in
/// `lex.include` handler to parse *included* files.
///
/// The returned document does **not** carry `reference_lines`: included
/// files reach the parent tree through the wire-AST codec, which has no
/// `reference_lines` field, so whole-element anchors authored *inside* an
/// included file are not propagated to the merged document (see the
/// follow-up note in `resolve_from_source`). The pre-pass still runs here
/// (it must, to keep a reference line from being mistaken for a structural
/// blank line in the included file's own parse), but its result is dropped
/// rather than emitted as a wrong-coordinate range in the merged document.
pub(crate) fn parse_no_attach(source: &str) -> Result<Document, String> {
    crate::lex::transforms::standard::parse_to_attached_root(source.to_string())
        .map(|(doc, _prepass)| doc)
        .map_err(|e| e.to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests;
