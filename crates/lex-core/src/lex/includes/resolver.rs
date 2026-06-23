//! The include resolution engine.
//!
//! [`resolve_from_source`] is the entry point: it runs the shared parser
//! front-end, stamps origins, normalises labels, then drives a post-order
//! recursive splice over the container tree. Every label whose schema
//! declares `hooks.resolve = true` flows through the same path — build a
//! [`LabelCtx`](lex_extension::wire::LabelCtx), dispatch through the
//! [`Registry`], decode the returned wire payload, recurse into the spliced
//! subtree, then splice in place. Cycle detection keys on `(label, origin,
//! start)` of the invocation site; depth and total-count limits bound
//! adversarial recursion and fan-out.

use super::wire::{
    decode_wire_to_items, handler_error_to_include_error, splice_items_first_origin,
    wire_node_origin_pathbuf,
};
use super::{stamp_doc, ContainerKind, IncludeError, ResolveConfig, KERNEL_DEPTH_BACKSTOP};
use crate::lex::assembling::stages::{ApplyTableConfig, NormalizeLabels};
use crate::lex::assembling::AttachAnnotations;
use crate::lex::ast::elements::container::GeneralContainer;
use crate::lex::ast::elements::content_item::ContentItem;
use crate::lex::ast::range::Range;
use crate::lex::ast::Document;
use crate::lex::transforms::Runnable;
use lex_extension_host::registry::Registry;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Resolve every `hooks.resolve = true` labelled annotation starting
/// from `source`, dispatching through `registry`, and recursively
/// processing the spliced content.
///
/// `source_path` identifies the entry-point file. It is used to
/// (a) stamp `Range.origin_path` on every node so downstream code
/// (file-ref resolution, diagnostics, LSP goto) can report locations
/// against the authoring file, and (b) provide the host directory
/// the built-in `lex.include` handler resolves relative `src=` paths
/// against (via `LabelCtx.node.origin`). When `None`, origin stamping
/// is skipped on the entry and the handler resolves relative paths
/// against `config.root`.
///
/// # Generic dispatch
///
/// Every label whose schema declares `hooks.resolve = true` flows
/// through the same path: build a [`LabelCtx`](lex_extension::wire::LabelCtx)
/// from the annotation, call [`Registry::dispatch_resolve_raw`], decode the
/// returned [`WireNode`](lex_extension::wire::WireNode) back into typed
/// [`ContentItem`]s via [`crate::lex::wire::from_wire_node`], and splice in
/// place. The built-in `lex.include` handler is registered the same way as
/// any third-party namespace.
///
/// # Pre/post-attachment
///
/// Internally this re-parses the entry source *without* annotation
/// attachment so labelled annotations stay visible as standalone
/// children. The handler does its own `parse_no_attach` for loaded
/// content. After all splices, [`AttachAnnotations`] runs once on
/// the merged tree.
///
/// # Recursion + cycle detection
///
/// Cycle detection keys on `(label, origin_path, start_position)` of
/// the invocation site. A handler that returns content containing
/// another invocation at the same source position is caught
/// immediately. A handler that varies the invocation position each
/// iteration terminates at `min(config.max_depth, KERNEL_DEPTH_BACKSTOP)`
/// with `IncludeError::DepthExceeded`. The total-includes counter
/// caps adversarial fan-out independent of depth.
pub fn resolve_from_source(
    source: &str,
    source_path: Option<PathBuf>,
    config: &ResolveConfig,
    registry: &Registry,
) -> Result<Document, IncludeError> {
    let entry_origin = source_path.as_ref().map(|p| Arc::new(p.clone()));

    // Run the SHARED parser front-end (the same one `run_string_to_ast`
    // uses): source → assembled Document (annotations still standalone)
    // plus the reference-line pre-pass results. This is the de-duplication
    // fix for lex#722 — before this, the resolver had its own hand-rolled
    // copy of the front-end (`parse_without_annotation_attachment`) that
    // never ran the reference-line pre-pass, so whole-element anchors were
    // silently dropped on the default `lexd <file> --to <fmt>` path. Now
    // there is exactly one front-end and it can't drift.
    let (mut doc, prepass) = crate::lex::transforms::standard::parse_to_attached_root(
        source.to_string(),
    )
    .map_err(|e| IncludeError::ParseFailed {
        path: source_path.clone().unwrap_or_default(),
        message: e.to_string(),
    })?;

    // Carry the entry file's reference lines (whole-element anchors) onto
    // the document so the babel serializers / LSP documentLink can render
    // them. These ranges are in the entry source's original coordinates,
    // which is correct for the entry file. Reference lines that live
    // *inside* included files are handled separately after splicing (see
    // below); they are NOT in `prepass`, which only saw the entry source.
    doc.reference_lines = prepass.reference_lines;
    doc.reference_line_diagnostics = prepass.diagnostics;

    if let Some(origin) = entry_origin.as_ref() {
        stamp_doc(&mut doc, origin);
    }

    // Normalise labels in the entry source BEFORE the resolve walk so
    // shortcut spellings (`:: include ::`, `:: image ::`, …) are
    // rewritten to their canonical form. The resolve dispatcher keys
    // on `registry.schema_for(label)` with the canonical spelling, so
    // without this an `:: include src=... ::` annotation would be
    // skipped because no schema is registered under the bare alias.
    //
    // Permissive mode: unknown labels are left as-is rather than
    // erroring. The standard parse pipeline enforces strict-mode
    // namespace policy (`STRING_TO_AST`); the resolve entry point is
    // a downstream stage that just needs the shortcut table applied
    // so dispatch finds the right handler.
    let mut doc =
        NormalizeLabels::permissive()
            .run(doc)
            .map_err(|e| IncludeError::ParseFailed {
                path: source_path.clone().unwrap_or_default(),
                message: format!("label normalisation failed: {e}"),
            })?;

    let mut chain: Vec<ResolveKey> = Vec::new();
    let mut state = ResolverState {
        config,
        registry,
        chain: &mut chain,
        depth: 0,
        total_resolved: 0,
    };

    splice_in_session_container(doc.root.children.as_mut_vec(), &mut state)?;

    let doc = AttachAnnotations::new()
        .run(doc)
        .map_err(|e| IncludeError::ParseFailed {
            path: source_path.clone().unwrap_or_default(),
            message: format!("annotation attachment failed: {e}"),
        })?;

    // Re-normalise after splicing. Each included file is parsed via
    // `parse_no_attach` (no normalisation), so shortcut labels in the
    // spliced content — e.g. `:: image src=... ::` inside an included
    // chapter — need rewriting before downstream IR/format passes can
    // dispatch them.
    let doc = NormalizeLabels::permissive()
        .run(doc)
        .map_err(|e| IncludeError::ParseFailed {
            path: source_path.clone().unwrap_or_default(),
            message: format!("label normalisation failed: {e}"),
        })?;

    // Apply table configuration so `:: table header=N align=... ::`
    // annotations attached to tables (here or in spliced content) take
    // effect — matches the order the standard pipeline runs them.
    let doc = ApplyTableConfig::new()
        .run(doc)
        .map_err(|e| IncludeError::ParseFailed {
            path: source_path.unwrap_or_default(),
            message: format!("table config application failed: {e}"),
        })?;

    Ok(doc)
}

// ============================================================================
// Splicing
// ============================================================================

/// One frame on the resolve-pass cycle stack. Two invocations at the
/// same `(label, origin, start)` position are a cycle, regardless of
/// what parameters either invocation uses — a handler that varies
/// params per call (random IDs, timestamps) cannot defeat the
/// detector by changing param values.
#[derive(Debug, Clone, PartialEq)]
struct ResolveKey {
    label: String,
    /// `Range.origin_path` of the annotation — the file the
    /// invocation was authored in. `None` when stamping was skipped
    /// (e.g., entry source loaded from a string with no path).
    origin: Option<PathBuf>,
    start: crate::lex::ast::range::Position,
}

impl ResolveKey {
    fn from_annotation(a: &crate::lex::ast::elements::annotation::Annotation) -> Self {
        Self {
            label: a.data.label.value.clone(),
            origin: a.location.origin_path.as_ref().map(|p| (**p).clone()),
            start: a.location.start,
        }
    }
}

/// Per-resolution state threaded through the recursive walker. Keeps the
/// signatures of the splice/process functions short and ensures
/// `chain`/`depth` are updated in lock-step (push/pop, +1/back-out) at
/// each invocation.
struct ResolverState<'a> {
    config: &'a ResolveConfig,
    registry: &'a Registry,
    /// Active resolution stack of `(label, origin, position)` keys.
    /// Pushed when we begin dispatching for an invocation and popped
    /// when its splice subtree is fully resolved. A push that finds
    /// the same key already on the stack is a cycle.
    chain: &'a mut Vec<ResolveKey>,
    /// Number of dispatch hops from the entry point. Each recursion
    /// increments by 1. Hitting `config.max_depth` or the
    /// [`KERNEL_DEPTH_BACKSTOP`] (whichever is lower) is an error.
    depth: usize,
    /// Total invocations resolved across the entire walk
    /// (depth × breadth). Incremented on every successful dispatch.
    /// Hitting `config.max_total_includes` aborts with
    /// `TotalIncludesExceeded`.
    total_resolved: usize,
}

fn splice_in_session_container(
    children: &mut Vec<ContentItem>,
    state: &mut ResolverState<'_>,
) -> Result<(), IncludeError> {
    // Post-order: recurse into nested containers first, splice this
    // container's invocations second. Recursion happens inside
    // `process_resolves` for any spliced subtree, so that subtree
    // is never re-walked at the parent level.
    recurse_into_children(children, state)?;
    process_resolves(children, state, ContainerKind::Session)
}

fn splice_in_general_container(
    container: &mut GeneralContainer,
    state: &mut ResolverState<'_>,
    kind: ContainerKind,
) -> Result<(), IncludeError> {
    recurse_into_children(container.as_mut_vec(), state)?;
    process_resolves(container.as_mut_vec(), state, kind)
}

/// Walk the children of a container, dispatch every annotation whose
/// schema declares `hooks.resolve = true` through the registry, and
/// splice the returned content in place of the annotation. Recurses
/// into the spliced content so nested invocations resolve too.
// Allow &mut Vec because `splice` needs Vec-specific operations.
#[allow(clippy::ptr_arg)]
fn process_resolves(
    children: &mut Vec<ContentItem>,
    state: &mut ResolverState<'_>,
    kind: ContainerKind,
) -> Result<(), IncludeError> {
    // Collect indices of annotations whose schema has hooks.resolve.
    let resolve_indices: Vec<usize> = children
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match item {
            ContentItem::Annotation(a) => {
                let label = &a.data.label.value;
                if state
                    .registry
                    .schema_for(label)
                    .map(|s| s.hooks.resolve)
                    .unwrap_or(false)
                {
                    Some(i)
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    for i in resolve_indices.into_iter().rev() {
        let annotation = match &children[i] {
            ContentItem::Annotation(a) => a.clone(),
            _ => unreachable!("index came from resolve filter"),
        };

        match resolve_one_invocation(&annotation, state, kind)? {
            ResolveOutcome::Spliced(splice_items) => {
                // Expansion replaces the directive with the included content. The
                // `lex.include` annotation is consumed — drop it. (It used to be
                // kept in the stream as provenance, relying on the serializer
                // dropping attached annotations; now that the serializer emits
                // them (lex#682), keeping it would leak `:: lex.include ::` into
                // expanded output. Origin provenance is tracked on
                // `Range.origin_path`, not this node.)
                children.splice(i..=i, splice_items);
            }
            ResolveOutcome::Unexpanded => {
                // Handler opted out of expanding this invocation. The
                // annotation stays in place, but its body wasn't
                // walked by `recurse_into_children` (that walker
                // skips resolve-hooked annotations to avoid double-
                // resolution). Walk the body now so any nested
                // invocations inside the unexpanded annotation get
                // resolved on the way back up.
                let mut owned = annotation;
                splice_in_general_container(
                    &mut owned.children,
                    state,
                    ContainerKind::AnnotationBody,
                )?;
                children[i] = ContentItem::Annotation(owned);
            }
        }
    }

    Ok(())
}

/// Outcome of dispatching a single resolve-hooked annotation. The
/// pass needs to distinguish between "handler returned content,
/// splice it in" and "handler opted out, leave the annotation
/// alone": the second case still requires walking the annotation's
/// body for nested invocations because `recurse_into_children`
/// otherwise skips resolve-hooked annotations to prevent double-
/// resolution.
enum ResolveOutcome {
    Spliced(Vec<ContentItem>),
    Unexpanded,
}

/// Dispatch a single resolve-hooked annotation through the registry,
/// decode the returned `WireNode` back into typed children, then
/// recursively walk the splice items so nested invocations resolve
/// before the splice is placed into the parent container.
///
/// Returns [`ResolveOutcome::Unexpanded`] when the handler returned
/// `Ok(None)` (third-party handlers can opt out of expanding a
/// particular invocation). The caller is then responsible for
/// walking the annotation's body for nested invocations — the
/// resolve walker normally skips resolve-hooked annotations'
/// bodies.
fn resolve_one_invocation(
    annotation: &crate::lex::ast::elements::annotation::Annotation,
    state: &mut ResolverState<'_>,
    parent_kind: ContainerKind,
) -> Result<ResolveOutcome, IncludeError> {
    let label = &annotation.data.label.value;
    let key = ResolveKey::from_annotation(annotation);

    // Cycle check on (label, origin, start) of the invocation site.
    if state.chain.contains(&key) {
        return Err(IncludeError::Cycle {
            include_site: annotation.location.clone(),
            path: key.origin.clone().unwrap_or_default(),
            chain: state
                .chain
                .iter()
                .map(|k| k.origin.clone().unwrap_or_default())
                .collect(),
        });
    }

    // Depth check. The effective limit is the lower of the
    // user-facing `config.max_depth` (default 8) and the hard
    // [`KERNEL_DEPTH_BACKSTOP`] (32, fixed). The kernel backstop
    // exists for adversarial varying-position recursion that the
    // cycle key can't catch — even if a user bumps `max_depth`
    // higher than 32 for legitimate deep atomization, the backstop
    // still terminates. The error reports `effective_depth_limit`
    // (the actual cap that fired) rather than `config.max_depth`,
    // so when the backstop is the binding limit the user sees `32`
    // and not the (higher) config value.
    let effective_depth_limit = state.config.max_depth.min(KERNEL_DEPTH_BACKSTOP);
    if state.depth >= effective_depth_limit {
        return Err(IncludeError::DepthExceeded {
            include_site: annotation.location.clone(),
            limit: effective_depth_limit,
            chain: state
                .chain
                .iter()
                .map(|k| k.origin.clone().unwrap_or_default())
                .collect(),
        });
    }

    // Total-count check before dispatch.
    if state.total_resolved >= state.config.max_total_includes {
        return Err(IncludeError::TotalIncludesExceeded {
            include_site: annotation.location.clone(),
            limit: state.config.max_total_includes,
        });
    }

    let ctx = build_label_ctx(annotation);

    let wire_node = match state.registry.dispatch_resolve_raw(&ctx) {
        Ok(Some(node)) => node,
        Ok(None) => {
            // Handler returned "nothing to splice" — leave the
            // annotation in place. The caller still needs to walk
            // its body for nested invocations (built-in lex.include
            // never returns None; this path is reachable only via
            // third-party handlers that opt out per-invocation).
            return Ok(ResolveOutcome::Unexpanded);
        }
        Err(handler_err) => {
            return Err(handler_error_to_include_error(
                &handler_err,
                label,
                &annotation.location,
            ));
        }
    };

    state.total_resolved += 1;

    // Decode the wire payload into typed lex-core ContentItems.
    let mut splice_items = decode_wire_to_items(&wire_node, label, &annotation.location)?;

    // Recurse into the spliced subtree FIRST so nested resolve-hooked
    // annotations are processed before the splice lands. Validation
    // must wait until *after* this step: a nested invocation can
    // splice in content (e.g. a top-level `Session` from a chained
    // `lex.include`) that wasn't in the handler's original output,
    // and the final shape is what has to satisfy the parent
    // container's policy.
    //
    // The `IncludeError::ContainerPolicy.file` field describes the
    // *spliced content's* source file (the file containing the
    // disallowed shape), not the invocation site. Take it from the
    // handler-returned wire payload's origin when present, falling
    // back to the first decoded item's origin path if the wire
    // payload didn't stamp a `Document` origin.
    let included_path = wire_node_origin_pathbuf(&wire_node)
        .or_else(|| splice_items_first_origin(&splice_items))
        .unwrap_or_default();
    state.chain.push(key);
    let saved_depth = state.depth;
    state.depth = saved_depth + 1;
    let recurse_result = splice_in_session_container(&mut splice_items, state);
    state.depth = saved_depth;
    state.chain.pop();
    recurse_result?;

    // Container-policy validation: enforce no-Sessions inside
    // `GeneralContainer` (Definition / Annotation body / ListItem).
    // Runs against the post-recursion splice list so nested
    // expansions can't smuggle disallowed shapes past the check.
    validate_against_kind(
        &splice_items,
        parent_kind,
        &annotation.location,
        &included_path,
    )?;

    Ok(ResolveOutcome::Spliced(splice_items))
}

/// Build a [`LabelCtx`](lex_extension::wire::LabelCtx) from a lex-core
/// [`Annotation`](crate::lex::ast::elements::annotation::Annotation). The
/// body is derived from the annotation's children (parsed-Lex form), the
/// params from `Annotation::data::parameters`, and the host node info
/// from `Annotation::location`.
fn build_label_ctx(
    a: &crate::lex::ast::elements::annotation::Annotation,
) -> lex_extension::wire::LabelCtx {
    use crate::lex::wire::to_wire_node;
    use lex_extension::wire::{AnnotationBody, LabelCtx, NodeRef};

    let label = a.data.label.value.clone();
    let params = {
        // Pass *semantic* parameter values to handlers (quotes
        // stripped, escape sequences resolved). Handlers consume
        // params as JSON values, where there is no "quoted string"
        // vs "unquoted token" distinction; only the decoded value
        // is meaningful. The codec's `parameters_to_json` (used by
        // `annotation_to_wire` for round-tripping annotation
        // *content*) keeps the raw form to preserve source — the
        // two paths intentionally differ.
        let mut obj = serde_json::Map::with_capacity(a.data.parameters.len());
        for p in &a.data.parameters {
            obj.insert(p.key.clone(), serde_json::Value::String(p.unquoted_value()));
        }
        serde_json::Value::Object(obj)
    };
    let body = if a.children.is_empty() {
        AnnotationBody::None
    } else {
        let wire_children: Vec<lex_extension::wire::WireNode> =
            a.children.iter().map(to_wire_node).collect();
        AnnotationBody::Lex {
            children: wire_children,
        }
    };
    let range = lex_extension::wire::Range::new(
        lex_extension::wire::Position::new(
            u32::try_from(a.location.start.line).unwrap_or(u32::MAX),
            u32::try_from(a.location.start.column).unwrap_or(u32::MAX),
        ),
        lex_extension::wire::Position::new(
            u32::try_from(a.location.end.line).unwrap_or(u32::MAX),
            u32::try_from(a.location.end.column).unwrap_or(u32::MAX),
        ),
    );
    let origin = a
        .location
        .origin_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    LabelCtx {
        label,
        params,
        body,
        node: NodeRef {
            kind: "annotation".into(),
            range,
            origin,
        },
    }
}

#[allow(clippy::ptr_arg)]
fn recurse_into_children(
    children: &mut Vec<ContentItem>,
    state: &mut ResolverState<'_>,
) -> Result<(), IncludeError> {
    for item in children.iter_mut() {
        match item {
            ContentItem::Session(s) => {
                splice_in_session_container(s.children.as_mut_vec(), state)?;
            }
            ContentItem::Definition(d) => {
                splice_in_general_container(&mut d.children, state, ContainerKind::Definition)?;
            }
            ContentItem::Annotation(a) => {
                // Skip the body of annotations whose schema declares
                // `hooks.resolve = true` — those are dispatched at the
                // parent level by `process_resolves`. Walking their
                // bodies *here* would trip the resolve again on the
                // same invocation.
                //
                // The body is still walked when the resolve actually
                // runs: `process_resolves` calls
                // `resolve_one_invocation`, and the
                // [`ResolveOutcome::Spliced`] arm walks the splice
                // subtree (which replaces the annotation), while the
                // [`ResolveOutcome::Unexpanded`] arm explicitly
                // walks the kept annotation's body via
                // `splice_in_general_container`. So nested
                // resolve-hooked annotations inside an unexpanded
                // outer annotation are still reached.
                //
                // Non-resolve-hooked annotations recurse normally
                // here so their nested bodies get processed.
                let is_resolve_hooked = state
                    .registry
                    .schema_for(&a.data.label.value)
                    .map(|s| s.hooks.resolve)
                    .unwrap_or(false);
                if !is_resolve_hooked {
                    splice_in_general_container(
                        &mut a.children,
                        state,
                        ContainerKind::AnnotationBody,
                    )?;
                }
            }
            ContentItem::List(l) => {
                for li in l.items.as_mut_vec().iter_mut() {
                    if let ContentItem::ListItem(item) = li {
                        splice_in_general_container(
                            &mut item.children,
                            state,
                            ContainerKind::ListItem,
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_against_kind(
    items: &[ContentItem],
    kind: ContainerKind,
    site: &Range,
    file: &Path,
) -> Result<(), IncludeError> {
    if kind.allows_sessions() {
        return Ok(());
    }
    if items.iter().any(|i| matches!(i, ContentItem::Session(_))) {
        return Err(IncludeError::ContainerPolicy {
            include_site: site.clone(),
            container: kind.name(),
            file: file.to_path_buf(),
            violation: "Sessions",
        });
    }
    Ok(())
}
