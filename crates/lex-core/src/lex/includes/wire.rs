//! Wire-payload decoding and origin helpers for the resolve pass.
//!
//! When a resolve-hooked handler runs it returns a
//! [`WireNode`](lex_extension::wire::WireNode); these helpers decode that
//! payload back into typed [`ContentItem`]s, lift its origin so
//! container-policy errors point at the *spliced content's* source file
//! rather than the invocation site, and map a [`HandlerError`] onto the
//! most specific [`IncludeError`] variant available.

use super::errors::IncludeError;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::range::Range;
use lex_extension::handler::HandlerError;
use std::path::PathBuf;

/// Lift a [`WireNode`](lex_extension::wire::WireNode)'s top-level `origin`
/// field into a `PathBuf` when present. Used by the resolve pass to
/// attribute container-policy errors to the *spliced content's* source
/// file rather than the invocation site.
pub(super) fn wire_node_origin_pathbuf(node: &lex_extension::wire::WireNode) -> Option<PathBuf> {
    use lex_extension::wire::WireNode as W;
    let s = match node {
        W::Document { origin, .. } => origin.as_deref(),
        W::Session { origin, .. } => origin.as_deref(),
        W::Definition { origin, .. } => origin.as_deref(),
        W::Paragraph { origin, .. } => origin.as_deref(),
        W::List { origin, .. } => origin.as_deref(),
        W::Verbatim { origin, .. } => origin.as_deref(),
        W::Table { origin, .. } => origin.as_deref(),
        W::Annotation { origin, .. } => origin.as_deref(),
        W::Blank { origin, .. } => origin.as_deref(),
        _ => None,
    };
    s.map(PathBuf::from)
}

/// Fallback when `WireNode::Document.origin` is unset: walk the
/// decoded splice list and return the first item that carries an
/// origin. The interner from `from_wire_node` ensures every item
/// shares one Arc per origin string, so iterating is cheap.
pub(super) fn splice_items_first_origin(items: &[ContentItem]) -> Option<PathBuf> {
    for item in items {
        let r = match item {
            ContentItem::Paragraph(p) => &p.location,
            ContentItem::Session(s) => &s.location,
            ContentItem::Definition(d) => &d.location,
            ContentItem::List(l) => &l.location,
            ContentItem::ListItem(li) => &li.location,
            ContentItem::Annotation(a) => &a.location,
            ContentItem::VerbatimBlock(v) => &v.location,
            ContentItem::VerbatimLine(vl) => &vl.location,
            ContentItem::Table(t) => &t.location,
            ContentItem::TextLine(tl) => &tl.location,
            ContentItem::BlankLineGroup(blg) => &blg.location,
        };
        if let Some(arc) = r.origin_path.as_ref() {
            return Some((**arc).clone());
        }
    }
    None
}

/// Convert a handler-returned [`WireNode`](lex_extension::wire::WireNode)
/// back into a list of [`ContentItem`]s ready for splicing.
/// `WireNode::Document` is unwrapped (its children become the splice list);
/// any other root shape is wrapped as a single-item list.
///
/// `invocation_label` is the label whose handler produced `wire` —
/// threaded through so wire-decode failures are attributed to the
/// real namespace rather than a hardcoded `lex.include`. A
/// third-party `acme.expand` handler that returns malformed wire
/// will surface as `IncludeError::HandlerFailed { label:
/// "acme.expand", .. }`.
pub(super) fn decode_wire_to_items(
    wire: &lex_extension::wire::WireNode,
    invocation_label: &str,
    include_site: &Range,
) -> Result<Vec<ContentItem>, IncludeError> {
    use crate::lex::wire::from_wire_node;

    from_wire_node(wire).map_err(|e| IncludeError::HandlerFailed {
        include_site: include_site.clone(),
        label: invocation_label.to_string(),
        code: "wire.decode".into(),
        message: format!("decoding handler-returned wire payload failed: {e}"),
    })
}

/// Map a [`HandlerError`] returned by the registry into the most
/// specific [`IncludeError`] variant available. Codes in the
/// `-32001..=-32005` range emitted by [`crate::lex::builtins::LexIncludeHandler`]
/// translate back to their corresponding pre-extension-system
/// variants so existing CLI/LSP error rendering and the integration
/// test suite keep working unchanged. Unknown codes (third-party
/// namespaces, future built-ins) surface as `HandlerFailed`.
pub(super) fn handler_error_to_include_error(
    err: &HandlerError,
    label: &str,
    include_site: &Range,
) -> IncludeError {
    use crate::lex::builtins::include::{
        CODE_ABSOLUTE_PATH, CODE_IO, CODE_MISSING_SRC, CODE_NOT_FOUND, CODE_OUTSIDE_ROOT,
        CODE_PARSE_FAILED, CODE_TOO_LARGE,
    };

    match err {
        HandlerError::Custom {
            code,
            message,
            data,
        } => match *code {
            CODE_NOT_FOUND => IncludeError::NotFound {
                include_site: include_site.clone(),
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
            },
            CODE_OUTSIDE_ROOT => IncludeError::RootEscape {
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
                root: data_str(data, "root")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
            },
            CODE_TOO_LARGE => IncludeError::FileTooLarge {
                include_site: include_site.clone(),
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
                size: data_u64(data, "size").unwrap_or(0),
                limit: data_u64(data, "limit").unwrap_or(0),
            },
            CODE_ABSOLUTE_PATH => IncludeError::AbsolutePath {
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
            },
            CODE_IO => IncludeError::LoaderIo {
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
                message: message.clone(),
            },
            CODE_MISSING_SRC => IncludeError::MissingSrc {
                include_site: include_site.clone(),
            },
            CODE_PARSE_FAILED => IncludeError::ParseFailed {
                path: data_str(data, "path")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
                message: data_str(data, "message").unwrap_or_else(|| message.clone()),
            },
            other => IncludeError::HandlerFailed {
                include_site: include_site.clone(),
                label: label.to_string(),
                code: format!("handler.custom({other})"),
                message: message.clone(),
            },
        },
        HandlerError::Internal { message } => IncludeError::HandlerFailed {
            include_site: include_site.clone(),
            label: label.to_string(),
            code: "handler.internal".into(),
            message: message.clone(),
        },
        HandlerError::Unsupported { detail } => IncludeError::HandlerFailed {
            include_site: include_site.clone(),
            label: label.to_string(),
            code: "handler.unsupported".into(),
            message: detail.clone(),
        },
    }
}

fn data_str(data: &Option<serde_json::Value>, key: &str) -> Option<String> {
    data.as_ref()?.get(key)?.as_str().map(str::to_string)
}

fn data_u64(data: &Option<serde_json::Value>, key: &str) -> Option<u64> {
    data.as_ref()?.get(key)?.as_u64()
}
