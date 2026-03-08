//! Inline parser implementation
//!
//!     Inline parsing is done by a declarative engine that will process each element declaration.
//!     For some, this is a flat transformation (i.e. it only wraps up the text into a node, as
//!     in bold or italic). Others are more involved, as in references, in which the engine will
//!     execute a callback with the text content and return a node.
//!
//!     This solves elegantly the fact that most inlines are simple and very much the same
//!     structure, while allowing for more complex ones to handle their specific needs.
//!
//!     The parser processes inline elements in order, matching start tokens and finding
//!     corresponding end tokens. Simple elements like bold and italic are flat transformations,
//!     while complex elements like references use post-processing callbacks.
//!
//! Simple (Flat) Inline Elements
//!
//!     Most inline elements are simple transformations that just wrap text content:
//!
//!     - **Strong** (*text*): Wraps content in `InlineNode::Strong(children)`
//!     - **Emphasis** (_text_): Wraps content in `InlineNode::Emphasis(children)`
//!     - **Code** (`text`): Wraps literal text in `InlineNode::Code(string)` - no nested parsing
//!     - **Math** (#formula#): Wraps literal text in `InlineNode::Math(string)` - no nested parsing
//!
//!     These are defined in the `default_specs()` function with just start/end tokens and whether
//!     they're literal (no nested inline parsing inside).
//!
//! Complex Inline Elements (with Post-Processing)
//!
//!     Some inline elements need additional logic after parsing:
//!
//!     - **References** ([target]): After wrapping content, the `classify_reference_node` callback
//!       analyzes the target text to determine the reference type (URL, citation, footnote, etc.)
//!       and creates the appropriate `ReferenceType` variant.
//!
//!     Example: `[https://example.com]` is classified as a URL reference, while `[@doe2024]` becomes
//!     a citation reference.
//!
//! Adding New Inline Types
//!
//!     To add a new inline element type:
//!
//!     1. Add a variant to `InlineKind` enum in [crate::lex::token::inline]
//!     2. Add a variant to `InlineNode` in the ast module
//!     3. Add an `InlineSpec` to `default_specs()` with start/end tokens
//!     4. If complex logic is needed, implement a post-processor callback:
//!        ```
//!        fn my_post_processor(node: InlineNode) -> InlineNode {
//!            // Transform the node based on its content
//!            node
//!        }
//!        ```
//!     5. Attach the callback via `.with_post_processor(InlineKind::MyType, my_post_processor)`
//!
//! Extension Pattern
//!
//!     The parser can be customized by creating an `InlineParser` instance and attaching
//!     post-processors for specific inline types:
//!     ```
//!     let parser = InlineParser::new()
//!         .with_post_processor(InlineKind::Strong, my_custom_processor);
//!     let result = parser.parse("*text*");
//!     ```

use super::references::classify_reference_node;
use crate::lex::ast::elements::inlines::{InlineContent, InlineNode, ReferenceInline};
use crate::lex::escape::unescape_inline_char;
use crate::lex::token::InlineKind;
use once_cell::sync::Lazy;
use std::collections::HashMap;

static DEFAULT_INLINE_PARSER: Lazy<InlineParser> = Lazy::new(InlineParser::new);

/// Parse inline nodes from a raw string using the default inline parser configuration.
pub fn parse_inlines(text: &str) -> InlineContent {
    DEFAULT_INLINE_PARSER.parse(text)
}

/// Parse inline nodes using a custom parser configuration.
pub fn parse_inlines_with_parser(text: &str, parser: &InlineParser) -> InlineContent {
    parser.parse(text)
}

/// Optional transformation applied to a parsed inline node.
pub type InlinePostProcessor = fn(InlineNode) -> InlineNode;

/// Specification for an inline element type
///
/// Defines how to parse and process a specific inline element. Each spec includes:
/// - The kind of inline element (from [InlineKind])
/// - Start and end tokens (single characters)
/// - Whether content is literal (no nested inline parsing)
/// - Optional post-processing callback for complex transformations
#[derive(Clone)]
pub struct InlineSpec {
    pub kind: InlineKind,
    pub start_token: char,
    pub end_token: char,
    pub literal: bool,
    pub post_process: Option<InlinePostProcessor>,
}

impl InlineSpec {
    fn apply_post_process(&self, node: InlineNode) -> InlineNode {
        if let Some(callback) = self.post_process {
            callback(node)
        } else {
            node
        }
    }
}

#[derive(Clone)]
pub struct InlineParser {
    specs: Vec<InlineSpec>,
    token_map: HashMap<char, usize>,
}

impl InlineParser {
    pub fn new() -> Self {
        Self::from_specs(default_specs())
    }

    /// Attach a post-processing callback to a specific inline kind.
    pub fn with_post_processor(mut self, kind: InlineKind, processor: InlinePostProcessor) -> Self {
        if let Some(spec) = self.specs.iter_mut().find(|spec| spec.kind == kind) {
            spec.post_process = Some(processor);
        }
        self
    }

    pub fn parse(&self, text: &str) -> InlineContent {
        parse_with(self, text)
    }

    fn from_specs(specs: Vec<InlineSpec>) -> Self {
        let mut token_map = HashMap::new();
        for (index, spec) in specs.iter().enumerate() {
            token_map.insert(spec.start_token, index);
        }
        Self { specs, token_map }
    }

    fn spec(&self, index: usize) -> &InlineSpec {
        &self.specs[index]
    }

    fn spec_index_for_start(&self, ch: char) -> Option<usize> {
        self.token_map.get(&ch).copied()
    }

    fn spec_count(&self) -> usize {
        self.specs.len()
    }
}

impl Default for InlineParser {
    fn default() -> Self {
        InlineParser::new()
    }
}

fn default_specs() -> Vec<InlineSpec> {
    vec![
        InlineSpec {
            kind: InlineKind::Strong,
            start_token: '*',
            end_token: '*',
            literal: false,
            post_process: None,
        },
        InlineSpec {
            kind: InlineKind::Emphasis,
            start_token: '_',
            end_token: '_',
            literal: false,
            post_process: None,
        },
        InlineSpec {
            kind: InlineKind::Code,
            start_token: '`',
            end_token: '`',
            literal: true,
            post_process: None,
        },
        InlineSpec {
            kind: InlineKind::Math,
            start_token: '#',
            end_token: '#',
            literal: true,
            post_process: None,
        },
        InlineSpec {
            kind: InlineKind::Reference,
            start_token: '[',
            end_token: ']',
            literal: true,
            post_process: Some(classify_reference_node),
        },
    ]
}

fn parse_with(parser: &InlineParser, text: &str) -> InlineContent {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut stack = vec![InlineFrame::root()];
    let mut blocked = BlockedClosings::new(parser.spec_count());

    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        let next = if i + 1 < chars.len() {
            Some(chars[i + 1])
        } else {
            None
        };

        if ch == '\\' {
            match unescape_inline_char(next) {
                crate::lex::escape::EscapeAction::Escape(escaped) => {
                    stack.last_mut().unwrap().push_char(escaped);
                    i += 2;
                    continue;
                }
                crate::lex::escape::EscapeAction::Literal => {
                    stack.last_mut().unwrap().push_char('\\');
                    if next.is_none() {
                        break;
                    }
                    i += 1;
                    continue;
                }
            }
        }

        let mut consumed = false;
        if let Some(spec_index) = stack.last().unwrap().spec_index {
            let spec = parser.spec(spec_index);
            if ch == spec.end_token {
                if blocked.consume(spec_index) {
                    // Literal closing paired to a disallowed nested start.
                } else if is_valid_end(prev, next, spec) {
                    let mut frame = stack.pop().unwrap();
                    frame.flush_buffer();
                    let had_content = frame.has_content();
                    if !had_content {
                        let parent = stack.last_mut().unwrap();
                        parent.push_char(spec.start_token);
                        parent.push_char(spec.end_token);
                    } else {
                        let node = frame.into_node(spec);
                        let node = spec.apply_post_process(node);
                        stack.last_mut().unwrap().push_node(node);
                    }
                    consumed = true;
                }
            }
        }

        if !consumed && !stack.last().unwrap().is_literal(parser) {
            if let Some(spec_index) = parser.spec_index_for_start(ch) {
                let spec = parser.spec(spec_index);
                if is_valid_start(prev, next, spec) {
                    if stack
                        .iter()
                        .any(|frame| frame.spec_index == Some(spec_index))
                    {
                        blocked.increment(spec_index);
                    } else {
                        stack.last_mut().unwrap().flush_buffer();
                        stack.push(InlineFrame::new(spec_index));
                        consumed = true;
                    }
                }
            }
        }

        if !consumed {
            stack.last_mut().unwrap().push_char(ch);
        }

        i += 1;
    }

    if let Some(frame) = stack.last_mut() {
        frame.flush_buffer();
    }

    while stack.len() > 1 {
        let mut frame = stack.pop().unwrap();
        frame.flush_buffer();
        let spec_index = frame
            .spec_index
            .expect("non-root stack frame must have a spec");
        let spec = parser.spec(spec_index);
        let parent = stack.last_mut().unwrap();
        parent.push_char(spec.start_token);
        for child in frame.children {
            parent.push_node(child);
        }
    }

    let mut root = stack.pop().unwrap();
    root.flush_buffer();
    root.children
}

struct InlineFrame {
    spec_index: Option<usize>,
    buffer: String,
    children: InlineContent,
}

impl InlineFrame {
    fn root() -> Self {
        Self {
            spec_index: None,
            buffer: String::new(),
            children: Vec::new(),
        }
    }

    fn new(spec_index: usize) -> Self {
        Self {
            spec_index: Some(spec_index),
            buffer: String::new(),
            children: Vec::new(),
        }
    }

    fn has_content(&self) -> bool {
        !self.buffer.is_empty() || !self.children.is_empty()
    }

    fn push_char(&mut self, ch: char) {
        self.buffer.push(ch);
    }

    fn flush_buffer(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.buffer);
        if let Some(InlineNode::Plain { text: existing, .. }) = self.children.last_mut() {
            existing.push_str(&text);
        } else {
            self.children.push(InlineNode::Plain {
                text,
                annotations: Vec::new(),
            });
        }
    }

    fn push_node(&mut self, node: InlineNode) {
        self.flush_buffer();
        match node {
            InlineNode::Plain { text, annotations } => {
                if text.is_empty() {
                    return;
                }
                if let Some(InlineNode::Plain { text: existing, .. }) = self.children.last_mut() {
                    existing.push_str(&text);
                    // Note: annotations from the merged node are discarded
                    // This is intentional as Plain nodes are typically created without annotations
                } else {
                    self.children.push(InlineNode::Plain { text, annotations });
                }
            }
            other => self.children.push(other),
        }
    }

    fn into_node(self, spec: &InlineSpec) -> InlineNode {
        match spec.kind {
            InlineKind::Strong => InlineNode::Strong {
                content: self.children,
                annotations: Vec::new(),
            },
            InlineKind::Emphasis => InlineNode::Emphasis {
                content: self.children,
                annotations: Vec::new(),
            },
            InlineKind::Code => InlineNode::Code {
                text: flatten_literal(self.children),
                annotations: Vec::new(),
            },
            InlineKind::Math => InlineNode::Math {
                text: flatten_literal(self.children),
                annotations: Vec::new(),
            },
            InlineKind::Reference => InlineNode::Reference {
                data: ReferenceInline::new(flatten_literal(self.children)),
                annotations: Vec::new(),
            },
        }
    }

    fn is_literal(&self, parser: &InlineParser) -> bool {
        self.spec_index
            .map(|index| parser.spec(index).literal)
            .unwrap_or(false)
    }
}

fn flatten_literal(children: InlineContent) -> String {
    let mut text = String::new();
    for node in children {
        match node {
            InlineNode::Plain { text: segment, .. } => text.push_str(&segment),
            _ => fatal_literal_content(),
        }
    }
    text
}

fn fatal_literal_content() -> ! {
    panic!("Literal inline nodes must not contain nested nodes");
}

struct BlockedClosings {
    counts: Vec<usize>,
}

impl BlockedClosings {
    fn new(spec_len: usize) -> Self {
        Self {
            counts: vec![0; spec_len],
        }
    }

    fn increment(&mut self, spec_index: usize) {
        if let Some(slot) = self.counts.get_mut(spec_index) {
            *slot += 1;
        }
    }

    fn consume(&mut self, spec_index: usize) -> bool {
        if let Some(slot) = self.counts.get_mut(spec_index) {
            if *slot > 0 {
                *slot -= 1;
                return true;
            }
        }
        false
    }
}

fn is_valid_start(prev: Option<char>, next: Option<char>, spec: &InlineSpec) -> bool {
    if spec.kind == InlineKind::Reference {
        !is_word(prev) && next.is_some()
    } else {
        !is_word(prev) && is_word(next)
    }
}

fn is_valid_end(prev: Option<char>, next: Option<char>, spec: &InlineSpec) -> bool {
    let inside_valid = if spec.literal {
        prev.is_some()
    } else {
        matches!(prev, Some(ch) if !ch.is_whitespace())
    };

    inside_valid && !is_word(next)
}

fn is_word(ch: Option<char>) -> bool {
    ch.map(|c| c.is_alphanumeric()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::inlines::{InlineNode, PageFormat, ReferenceType};

    #[test]
    fn parses_plain_text() {
        let nodes = parse_inlines("hello world");
        assert_eq!(
            nodes,
            vec![InlineNode::Plain {
                text: "hello world".into(),
                annotations: Vec::new()
            }]
        );
    }

    #[test]
    fn parses_strong_and_emphasis() {
        let nodes = parse_inlines("*strong _inner_* text");
        assert_eq!(nodes.len(), 2);
        match &nodes[0] {
            InlineNode::Strong { content, .. } => {
                assert_eq!(content.len(), 2);
                assert_eq!(
                    content[0],
                    InlineNode::Plain {
                        text: "strong ".into(),
                        annotations: Vec::new()
                    }
                );
                match &content[1] {
                    InlineNode::Emphasis { content: inner, .. } => {
                        assert_eq!(
                            inner,
                            &vec![InlineNode::Plain {
                                text: "inner".into(),
                                annotations: Vec::new()
                            }]
                        );
                    }
                    other => panic!("Unexpected child: {other:?}"),
                }
            }
            other => panic!("Unexpected node: {other:?}"),
        }
        assert_eq!(
            nodes[1],
            InlineNode::Plain {
                text: " text".into(),
                annotations: Vec::new()
            }
        );
    }

    #[test]
    fn nested_emphasis_inside_strong() {
        let nodes = parse_inlines("*strong and _emphasis_* text");
        assert_eq!(nodes.len(), 2);
        match &nodes[0] {
            InlineNode::Strong { content, .. } => {
                assert_eq!(content.len(), 2);
                assert_eq!(content[0], InlineNode::plain("strong and ".into()));
                match &content[1] {
                    InlineNode::Emphasis { content: inner, .. } => {
                        assert_eq!(inner, &vec![InlineNode::plain("emphasis".into())]);
                    }
                    other => panic!("Unexpected child: {other:?}"),
                }
            }
            _ => panic!("Expected strong node"),
        }
    }

    #[test]
    fn code_is_literal() {
        let nodes = parse_inlines("`a * literal _` text");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0], InlineNode::code("a * literal _".into()));
        assert_eq!(nodes[1], InlineNode::plain(" text".into()));
    }

    #[test]
    fn math_is_literal() {
        let nodes = parse_inlines("#x + y#");
        assert_eq!(nodes, vec![InlineNode::math("x + y".into())]);
    }

    #[test]
    fn unmatched_start_is_literal() {
        let nodes = parse_inlines("prefix *text");
        assert_eq!(nodes, vec![InlineNode::plain("prefix *text".into())]);
    }

    #[test]
    fn unmatched_nested_preserves_children() {
        let nodes = parse_inlines("*a _b_ c");
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0], InlineNode::plain("*a ".into()));
        match &nodes[1] {
            InlineNode::Emphasis { content, .. } => {
                assert_eq!(content, &vec![InlineNode::plain("b".into())]);
            }
            other => panic!("Unexpected node: {other:?}"),
        }
        assert_eq!(nodes[2], InlineNode::plain(" c".into()));
    }

    #[test]
    fn same_type_nesting_skips_inner_pair() {
        let nodes = parse_inlines("*outer *inner* text*");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            InlineNode::Strong { content, .. } => {
                assert_eq!(
                    content,
                    &vec![InlineNode::plain("outer *inner* text".into())]
                );
            }
            other => panic!("Unexpected node: {other:?}"),
        }
    }

    #[test]
    fn reference_detects_url() {
        let nodes = parse_inlines("[https://example.com]");
        match &nodes[0] {
            InlineNode::Reference { data, .. } => match &data.reference_type {
                ReferenceType::Url { target } => assert_eq!(target, "https://example.com"),
                other => panic!("Expected URL reference, got {other:?}"),
            },
            other => panic!("Unexpected node: {other:?}"),
        }
    }

    #[test]
    fn reference_detects_tk_identifier() {
        let nodes = parse_inlines("[TK-feature]");
        match &nodes[0] {
            InlineNode::Reference { data, .. } => match &data.reference_type {
                ReferenceType::ToCome { identifier } => {
                    assert_eq!(identifier.as_deref(), Some("feature"));
                }
                other => panic!("Expected TK reference, got {other:?}"),
            },
            other => panic!("Unexpected node: {other:?}"),
        }
    }

    #[test]
    fn reference_detects_citation_and_footnotes() {
        let citation = parse_inlines("[@doe2024]");
        let labeled = parse_inlines("[^note1]");
        let numbered = parse_inlines("[42]");

        match &citation[0] {
            InlineNode::Reference { data, .. } => match &data.reference_type {
                ReferenceType::Citation(citation_data) => {
                    assert_eq!(citation_data.keys, vec!["doe2024".to_string()]);
                    assert!(citation_data.locator.is_none());
                }
                other => panic!("Expected citation, got {other:?}"),
            },
            _ => panic!("Expected reference"),
        }
        match &labeled[0] {
            InlineNode::Reference { data, .. } => match &data.reference_type {
                ReferenceType::FootnoteLabeled { label } => assert_eq!(label, "note1"),
                other => panic!("Expected labeled footnote, got {other:?}"),
            },
            _ => panic!("Expected reference"),
        }
        match &numbered[0] {
            InlineNode::Reference { data, .. } => match &data.reference_type {
                ReferenceType::FootnoteNumber { number } => assert_eq!(*number, 42),
                other => panic!("Expected numeric footnote, got {other:?}"),
            },
            _ => panic!("Expected reference"),
        }
    }

    #[test]
    fn reference_parses_citation_locator() {
        let nodes = parse_inlines("[@doe2024; @smith2023, pp. 45-46,47]");
        match &nodes[0] {
            InlineNode::Reference { data, .. } => match &data.reference_type {
                ReferenceType::Citation(citation_data) => {
                    assert_eq!(
                        citation_data.keys,
                        vec!["doe2024".to_string(), "smith2023".to_string()]
                    );
                    let locator = citation_data.locator.as_ref().expect("expected locator");
                    assert!(matches!(locator.format, PageFormat::Pp));
                    assert_eq!(locator.ranges.len(), 2);
                    assert_eq!(locator.ranges[0].start, 45);
                    assert_eq!(locator.ranges[0].end, Some(46));
                    assert_eq!(locator.ranges[1].start, 47);
                    assert!(locator.ranges[1].end.is_none());
                }
                other => panic!("Expected citation, got {other:?}"),
            },
            _ => panic!("Expected reference"),
        }
    }

    #[test]
    fn reference_detects_general_and_not_sure() {
        let general = parse_inlines("[Section Title]");
        let unsure = parse_inlines("[!!!]");
        match &general[0] {
            InlineNode::Reference { data, .. } => match &data.reference_type {
                ReferenceType::General { target } => assert_eq!(target, "Section Title"),
                other => panic!("Expected general reference, got {other:?}"),
            },
            _ => panic!("Expected reference"),
        }
        match &unsure[0] {
            InlineNode::Reference { data, .. } => {
                assert!(matches!(data.reference_type, ReferenceType::NotSure));
            }
            _ => panic!("Expected reference"),
        }
    }

    fn annotate_strong(node: InlineNode) -> InlineNode {
        match node {
            InlineNode::Strong {
                mut content,
                annotations,
            } => {
                let mut annotated = vec![InlineNode::plain("[strong]".into())];
                annotated.append(&mut content);
                InlineNode::Strong {
                    content: annotated,
                    annotations,
                }
            }
            other => other,
        }
    }

    #[test]
    fn post_process_callback_transforms_node() {
        let parser = InlineParser::new().with_post_processor(InlineKind::Strong, annotate_strong);
        let nodes = parser.parse("*bold*");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            InlineNode::Strong { content, .. } => {
                assert_eq!(content[0], InlineNode::plain("[strong]".into()));
                assert_eq!(content[1], InlineNode::plain("bold".into()));
            }
            other => panic!("Unexpected inline node: {other:?}"),
        }
    }

    #[test]
    fn escaped_tokens_are_literal() {
        let nodes = parse_inlines("\\*literal\\*");
        assert_eq!(nodes, vec![InlineNode::plain("*literal*".into())]);
    }

    #[test]
    fn backslash_before_alphanumeric_preserved() {
        let nodes = parse_inlines("C:\\Users\\name");
        assert_eq!(nodes, vec![InlineNode::plain("C:\\Users\\name".into())]);
    }

    #[test]
    fn escape_works_in_paths() {
        let nodes = parse_inlines("Path: C:\\\\Users\\\\name");
        assert_eq!(
            nodes,
            vec![InlineNode::plain("Path: C:\\Users\\name".into())]
        );
    }

    #[test]
    fn arithmetic_not_parsed_as_inline() {
        let nodes = parse_inlines("7 * 8");
        assert_eq!(nodes, vec![InlineNode::plain("7 * 8".into())]);
    }

    #[test]
    fn word_boundary_start_invalid() {
        let nodes = parse_inlines("word*s*");
        assert_eq!(nodes, vec![InlineNode::plain("word*s*".into())]);
    }

    #[test]
    fn multiple_arithmetic_expressions() {
        let nodes = parse_inlines("Calculate 7 * 8 + 3 * 4");
        assert_eq!(
            nodes,
            vec![InlineNode::plain("Calculate 7 * 8 + 3 * 4".into())]
        );
    }

    #[test]
    fn inline_node_annotations_empty_by_default() {
        let nodes = parse_inlines("*bold* text");
        assert_eq!(nodes.len(), 2);
        assert!(nodes[0].annotations().is_empty());
        assert!(nodes[1].annotations().is_empty());
    }

    #[test]
    fn with_annotation_adds_annotation_to_node() {
        use crate::lex::ast::elements::{Annotation, Label};

        let annotation = Annotation::marker(Label::new("test".to_string()));
        let node = InlineNode::plain("text".into()).with_annotation(annotation.clone());

        assert_eq!(node.annotations().len(), 1);
        assert_eq!(node.annotations()[0].data.label.value, "test");
    }

    #[test]
    fn with_annotations_adds_multiple_annotations() {
        use crate::lex::ast::elements::{Annotation, Label, Parameter};

        let anno1 = Annotation::marker(Label::new("doc.data".to_string()));
        let anno2 = Annotation::with_parameters(
            Label::new("test".to_string()),
            vec![Parameter::new("key".to_string(), "value".to_string())],
        );

        let node = InlineNode::math("x + y".into()).with_annotations(vec![anno1, anno2]);

        assert_eq!(node.annotations().len(), 2);
        assert_eq!(node.annotations()[0].data.label.value, "doc.data");
        assert_eq!(node.annotations()[1].data.label.value, "test");
    }

    #[test]
    fn annotations_mut_allows_modification() {
        use crate::lex::ast::elements::{Annotation, Label};

        let mut node = InlineNode::code("code".into());
        assert!(node.annotations().is_empty());

        let annotation = Annotation::marker(Label::new("highlighted".to_string()));
        node.annotations_mut().push(annotation);

        assert_eq!(node.annotations().len(), 1);
        assert_eq!(node.annotations()[0].data.label.value, "highlighted");
    }

    #[test]
    fn post_processor_can_add_annotations() {
        use crate::lex::ast::elements::{Annotation, Label, Parameter};

        fn add_mathml_annotation(node: InlineNode) -> InlineNode {
            match node {
                InlineNode::Math {
                    text,
                    mut annotations,
                } => {
                    let anno = Annotation::with_parameters(
                        Label::new("doc.data".to_string()),
                        vec![Parameter::new("type".to_string(), "mathml".to_string())],
                    );
                    annotations.push(anno);
                    InlineNode::Math { text, annotations }
                }
                other => other,
            }
        }

        let parser =
            InlineParser::new().with_post_processor(InlineKind::Math, add_mathml_annotation);
        let nodes = parser.parse("#x + y#");

        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            InlineNode::Math { text, annotations } => {
                assert_eq!(text, "x + y");
                assert_eq!(annotations.len(), 1);
                assert_eq!(annotations[0].data.label.value, "doc.data");
                assert_eq!(annotations[0].data.parameters.len(), 1);
                assert_eq!(annotations[0].data.parameters[0].key, "type");
                assert_eq!(annotations[0].data.parameters[0].value, "mathml");
            }
            other => panic!("Expected math node, got {other:?}"),
        }
    }
}
