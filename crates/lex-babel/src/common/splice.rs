//! State machine for splicing handler-rendered raw passthrough content
//! into a format's event walk in place of the default annotation rendering.
//!
//! Every serialization-oriented format on our horizon already has a
//! native concept of "raw passthrough content" (Markdown's
//! `HtmlBlock`, HTML's raw text, Pandoc's `RawBlock`, RFC-XML's
//! `<raw>`, LaTeX's verbatim string append). This module factors the
//! splice book-keeping — annotation indexing, depth-tracking,
//! skip-until-EndAnnotation logic — out of every format adapter so
//! each one only needs to supply a one-line "emit raw passthrough"
//! callback for its native mechanism.
//!
//! `dispatch_render` walks the IR and `tree_to_events` walks the same
//! IR, so both visit body annotations in matching document order. A
//! cursor over the plan advances only when the event walker's label
//! matches the next plan entry — that way an unregistered annotation
//! (no plan entry) leaves the cursor parked instead of shifting past
//! the next registered entry. When the matched plan entry has output,
//! `advance_at_start` returns it and enters skip-state so the
//! annotation's children — including any inner labelled annotations —
//! are suppressed (the handler owns the entire subtree).
//!
//! ## HTML and sentinel substitution
//!
//! Markdown can splice raw passthrough directly via `HtmlBlock`. HTML's
//! DOM library (`markup5ever_rcdom`) has no raw-text node, so the HTML
//! adapter still emits a sentinel comment into the DOM and substitutes
//! it after serialization. The [`SentinelBuffer`] helper here owns the
//! sentinel-encoding contract so the substitution logic lives in one
//! place instead of being re-implemented in every DOM-based format.

use crate::render_dispatch::RenderedNode;

/// Splice state machine. Construct one per serialization pass with the
/// `body_plan` slice (the dispatch plan's body entries, post any
/// doc-scope prefix the format consumes separately). The event walk
/// calls [`advance_at_start`](Self::advance_at_start) on every
/// `StartAnnotation`, [`advance_at_end`](Self::advance_at_end) on every
/// `EndAnnotation`, and [`should_skip`](Self::should_skip) on every
/// other event to decide whether the event falls inside a
/// handler-owned subtree.
pub struct SpliceState<'a> {
    plan: Option<&'a [RenderedNode]>,
    annotation_idx: usize,
    skip_depth: usize,
}

impl<'a> SpliceState<'a> {
    /// New splice state. `None` means "no splicing" — every
    /// `advance_at_start` returns `None` and `should_skip` is always
    /// `false`, so the event walk emits its default rendering for
    /// every annotation. `Some(plan)` engages splicing against the
    /// supplied plan.
    pub fn new(plan: Option<&'a [RenderedNode]>) -> Self {
        Self {
            plan,
            annotation_idx: 0,
            skip_depth: 0,
        }
    }

    /// Call on every `Event::StartAnnotation`. Returns `Some(rendered)`
    /// if this annotation should be replaced by raw passthrough content
    /// (and enters skip-state so the body events emit nothing).
    /// Returns `None` to fall through to default rendering.
    ///
    /// The cursor advances only when the next plan entry's label matches
    /// the event's label. An unregistered annotation arriving from the
    /// event walker (one without a schema, so no plan entry) leaves the
    /// cursor untouched — otherwise an `unknown.label` arriving before a
    /// registered `acme.task` would shift the cursor past the
    /// `acme.task` entry and the handler output would never splice.
    ///
    /// Inside an active splice (skip-depth > 0), nested annotations
    /// with their own plan entries (the dispatch walker visits every
    /// annotation regardless of nesting) also advance the cursor so
    /// the post-splice cursor lands on the next outer entry.
    ///
    /// Known limitation: renderable verbatim plan entries don't
    /// correspond to a `StartAnnotation` event, so they aren't advanced
    /// past by this method. The HTML and markdown event walkers don't
    /// splice verbatim renders today either, so the limitation is
    /// confined to scenarios with both a renderable verbatim and a
    /// downstream registered annotation in the same document — a
    /// pattern not exercised by any current test.
    pub fn advance_at_start(&mut self, label: &str) -> Option<&str> {
        let plan = self.plan?;

        if self.skip_depth > 0 {
            // Inside a handler-owned subtree. The dispatch walker still
            // pushes a plan entry for nested annotations it visits, so
            // consume the cursor entry that matches this inner label so
            // it doesn't shadow the next outer annotation after the
            // splice closes.
            if let Some(entry) = plan.get(self.annotation_idx) {
                if entry.label == label {
                    self.annotation_idx += 1;
                }
            }
            self.skip_depth += 1;
            return None;
        }

        let entry = plan.get(self.annotation_idx)?;
        if entry.label != label {
            // Walker is ahead of (or out of sync with) the plan — most
            // commonly an unregistered annotation that left no plan
            // entry. Leave the cursor parked so the next event still
            // sees this entry.
            return None;
        }
        self.annotation_idx += 1;
        let content = entry.output.as_deref()?;
        self.skip_depth = 1;
        Some(content)
    }

    /// True if the current event arrives inside a handler-owned
    /// subtree and should be suppressed. Inspect `Start/EndAnnotation`
    /// even when this is true (they advance the depth counter); skip
    /// every other event.
    pub fn should_skip(&self) -> bool {
        self.skip_depth > 0
    }

    /// Call on every `Event::EndAnnotation`. Pops one level of
    /// skip-depth; the splice region ends when this reaches zero.
    pub fn advance_at_end(&mut self) {
        if self.skip_depth > 0 {
            self.skip_depth -= 1;
        }
    }
}

/// Sentinel buffer for DOM-based formats that can't inject raw content
/// directly (HTML's `markup5ever_rcdom`). The format embeds the string
/// returned by [`push`](Self::push) at the splice site, and
/// [`replace`](Self::replace) substitutes each sentinel for its
/// recorded content after DOM serialization.
///
/// Markdown and other formats whose DOM can carry raw passthrough
/// natively (Comrak's `NodeValue::HtmlBlock`) don't need this — the
/// splice content from [`SpliceState::advance_at_start`] goes straight
/// into the format's native raw-passthrough node.
#[derive(Default)]
pub struct SentinelBuffer {
    outputs: Vec<String>,
}

impl SentinelBuffer {
    /// The literal prefix used inside the sentinel comment. Public for
    /// formats that want to assert sentinels don't leak into output.
    pub const PREFIX: &'static str = "LEX-RENDER-SPLICE:";

    pub fn new() -> Self {
        Self::default()
    }

    /// Record `content` and return the sentinel's inner text (`PREFIX +
    /// N`). The caller embeds it inside a comment-style node — for
    /// rcdom that's `NodeData::Comment { contents: <returned string> }`,
    /// which the DOM serializer wraps in `<!--` ... `-->`. The returned
    /// string is the inner text only so callers don't accidentally
    /// double-wrap the markers.
    pub fn push(&mut self, content: String) -> String {
        let idx = self.outputs.len();
        self.outputs.push(content);
        format!("{}{}", Self::PREFIX, idx)
    }

    /// True if no sentinels were recorded — the caller can skip the
    /// substitution pass entirely.
    pub fn is_empty(&self) -> bool {
        self.outputs.is_empty()
    }

    /// Substitute every `<!--LEX-RENDER-SPLICE:N-->` comment in `html`
    /// with `outputs[N]`. Tolerates trailing whitespace inside the
    /// sentinel (DOM serializers sometimes normalize comment text).
    /// Non-numeric or out-of-range indices are left in place — a
    /// programming bug surfaces as a visible sentinel in the output
    /// instead of silent corruption.
    pub fn replace(&self, html: &str) -> String {
        if self.outputs.is_empty() {
            return html.to_string();
        }
        let mut out = String::with_capacity(html.len());
        let mut remaining = html;
        let pattern_open = format!("<!--{}", Self::PREFIX);
        while let Some(start) = remaining.find(&pattern_open) {
            out.push_str(&remaining[..start]);
            let after_prefix = &remaining[start + pattern_open.len()..];
            let Some(end_marker) = after_prefix.find("-->") else {
                out.push_str(&remaining[start..]);
                remaining = "";
                break;
            };
            let id_str = after_prefix[..end_marker].trim();
            match id_str.parse::<usize>() {
                Ok(idx) if idx < self.outputs.len() => {
                    out.push_str(&self.outputs[idx]);
                }
                _ => {
                    out.push_str(&remaining[start..start + pattern_open.len() + end_marker + 3]);
                }
            }
            remaining = &after_prefix[end_marker + 3..];
        }
        out.push_str(remaining);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(label: &str, output: Option<&str>) -> RenderedNode {
        RenderedNode {
            label: label.into(),
            output: output.map(str::to_string),
            diagnostic: None,
        }
    }

    #[test]
    fn no_plan_never_splices() {
        let mut s = SpliceState::new(None);
        assert!(s.advance_at_start("acme.task").is_none());
        assert!(!s.should_skip());
        s.advance_at_end();
    }

    #[test]
    fn plan_entry_without_output_falls_through() {
        let plan = [rendered("acme.task", None)];
        let mut s = SpliceState::new(Some(&plan));
        assert!(s.advance_at_start("acme.task").is_none());
        assert!(!s.should_skip());
    }

    #[test]
    fn plan_entry_with_output_returns_content_and_skips_body() {
        let plan = [rendered("acme.task", Some("<RENDERED/>"))];
        let mut s = SpliceState::new(Some(&plan));
        assert_eq!(s.advance_at_start("acme.task"), Some("<RENDERED/>"));
        assert!(s.should_skip());
        s.advance_at_end();
        assert!(!s.should_skip());
    }

    #[test]
    fn label_mismatch_falls_through() {
        // Plan and event walk diverged — defensive fall-through to
        // default rendering rather than splicing the wrong output.
        let plan = [rendered("acme.task", Some("<X/>"))];
        let mut s = SpliceState::new(Some(&plan));
        assert!(s.advance_at_start("other.label").is_none());
        assert!(!s.should_skip());
    }

    #[test]
    fn nested_annotations_inside_splice_keep_depth() {
        let plan = [rendered("outer", Some("<OUTER/>"))];
        let mut s = SpliceState::new(Some(&plan));
        assert_eq!(s.advance_at_start("outer"), Some("<OUTER/>"));
        assert!(s.should_skip());
        // Inner annotation inside the spliced subtree — counter
        // advances, skip-depth nests.
        assert!(s.advance_at_start("inner").is_none());
        assert!(s.should_skip());
        s.advance_at_end(); // close inner
        assert!(s.should_skip());
        s.advance_at_end(); // close outer
        assert!(!s.should_skip());
    }

    #[test]
    fn counter_advances_across_unspliced_annotations() {
        let plan = [
            rendered("a", None),
            rendered("b", Some("<B/>")),
            rendered("c", None),
        ];
        let mut s = SpliceState::new(Some(&plan));
        assert!(s.advance_at_start("a").is_none());
        s.advance_at_end();
        assert_eq!(s.advance_at_start("b"), Some("<B/>"));
        s.advance_at_end();
        assert!(s.advance_at_start("c").is_none());
        s.advance_at_end();
    }

    #[test]
    fn sentinel_buffer_round_trips_single_splice() {
        let mut buf = SentinelBuffer::new();
        let sentinel = buf.push("<DIV/>".into());
        let html = format!("<p><!--{sentinel}--></p>");
        assert_eq!(buf.replace(&html), "<p><DIV/></p>");
    }

    #[test]
    fn sentinel_buffer_round_trips_multiple_in_order() {
        let mut buf = SentinelBuffer::new();
        let s0 = buf.push("ZERO".into());
        let s1 = buf.push("ONE".into());
        let html = format!("a<!--{s0}-->b<!--{s1}-->c");
        assert_eq!(buf.replace(&html), "aZERObONEc");
    }

    #[test]
    fn sentinel_buffer_empty_is_no_op() {
        let buf = SentinelBuffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.replace("plain html"), "plain html");
    }

    #[test]
    fn sentinel_buffer_leaves_unknown_index_visible() {
        // Seed the buffer so `replace` doesn't early-return on an empty
        // outputs vec — otherwise the out-of-range branch isn't
        // exercised at all (Copilot review on PR #625).
        let mut buf = SentinelBuffer::new();
        buf.push("recorded".to_string());
        let html = "<!--LEX-RENDER-SPLICE:7-->";
        assert_eq!(
            buf.replace(html),
            html,
            "out-of-range index must stay visible even when other sentinels are recorded"
        );
    }

    /// Regression for Copilot's review on PR #625: an unregistered
    /// annotation arriving from the event walker before a registered
    /// one must not shift the cursor — otherwise the registered
    /// annotation's handler output never splices because the plan
    /// entry it points at has the wrong label.
    #[test]
    fn unregistered_annotation_before_registered_keeps_cursor_aligned() {
        let plan = [rendered("acme.task", Some("<HANDLER/>"))];
        let mut s = SpliceState::new(Some(&plan));
        // Unregistered annotation event: no plan entry, must not consume.
        assert!(s.advance_at_start("unknown.label").is_none());
        s.advance_at_end();
        // Registered annotation event: must match plan[0] and splice.
        assert_eq!(s.advance_at_start("acme.task"), Some("<HANDLER/>"));
        s.advance_at_end();
    }

    /// Regression for Copilot's review on PR #625: a nested registered
    /// annotation inside a handler-owned splice must consume its plan
    /// entry so subsequent outer annotations align correctly.
    #[test]
    fn nested_registered_annotation_inside_splice_consumes_its_plan_entry() {
        let plan = [
            rendered("outer", Some("<OUTER/>")),
            rendered("inner", Some("<INNER_UNUSED/>")),
            rendered("after", Some("<AFTER/>")),
        ];
        let mut s = SpliceState::new(Some(&plan));
        // Outer fires and enters skip-state.
        assert_eq!(s.advance_at_start("outer"), Some("<OUTER/>"));
        assert!(s.should_skip());
        // Inner is registered (has a plan entry) but inside the splice
        // — must consume its cursor entry so the next outer is aligned.
        assert!(s.advance_at_start("inner").is_none());
        s.advance_at_end(); // close inner
        s.advance_at_end(); // close outer
                            // After the splice closes, the next outer annotation aligns
                            // with plan[2], not plan[1].
        assert_eq!(s.advance_at_start("after"), Some("<AFTER/>"));
        s.advance_at_end();
    }
}
