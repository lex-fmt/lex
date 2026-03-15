//! This is the semantic token collector, which editors use for syntax highlighting.
//! It's worth going over the general approach.
//!
//! Semantic Tokens and Editor Highlighting Architecture
//!
//!     1. LSP emits semantic tokens using our format's native terminology (e.g., `Verbatim`
//! Annotation, etc). The LSP declares a token legend at initialization and emits tokens as indices
//! into that legend—it has no knowledge of editor-specific theming.
//!     2. Editor plugins map our token types to the editor's theme primitives. This lets users
//! leverage their existing theme choices while our core LSP code remains editor-agnostic.
//!     
//! Editor-Specific Mapping
//!
//!     VSCode — declarative mapping in `package.json`:
//!         "semanticTokenScopes": [{
//!         "language": "ourformat",
//!         "scopes": {
//!         "Verbatim": ["markup.inline.raw"],
//!         "Heading": ["markup.heading"],
//!         "Emphasis": ["markup.italic"]
//!         }
//!         }]
//!     :: javascript
//!
//!     We map to TextMate scopes (`markup.*`) as they have broad theme support and are a natural
//! fit for markup.
//!
//!     Neovim — imperative mapping in the plugin:
//!         vim.api.nvim_set_hl(0, '@lsp.type.Verbatim', { link = '@markup.raw' })
//!         `vim.api.nvim_set_hl(0, '@lsp.type.Heading', { link = '@markup.heading' })
//!         vim.api.nvim_set_hl(0, '@lsp.type.Emphasis', { link = '@markup.italic' })
//!     :: lua
//!     We link to treesitter's `@markup.*` groups for equivalent theme coverage.
//!     Benefits:
//!         - LSP speaks our format's semantics—no impedance mismatch
//!         - Users get syntax highlighting that respects their theme
//!         - Mapping logic is isolated to editor plugins; adding a new editor doesn't touch the LSP
//!
//! The file editors/vscode/themes/lex-light.json has the reocommended theming for Lex to be used in
//! tests and so forth.
use lex_core::lex::ast::{
    Annotation, ContentItem, Definition, Document, List, ListItem, Paragraph, Position, Range,
    Session, Table, TextContent, Verbatim,
};
use lex_core::lex::inlines::{InlineNode, ReferenceType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LexSemanticTokenKind {
    DocumentTitle,
    SessionMarker,
    SessionTitleText,
    DefinitionSubject,
    DefinitionContent,
    ListMarker,
    ListItemText,
    AnnotationLabel,
    AnnotationParameter,
    AnnotationContent,
    InlineStrong,
    InlineEmphasis,
    InlineCode,
    InlineMath,
    Reference,
    ReferenceCitation,
    ReferenceFootnote,
    VerbatimSubject,
    VerbatimLanguage,
    VerbatimAttribute,
    VerbatimContent,
    InlineMarkerStrongStart,
    InlineMarkerStrongEnd,
    InlineMarkerEmphasisStart,
    InlineMarkerEmphasisEnd,
    InlineMarkerCodeStart,
    InlineMarkerCodeEnd,
    InlineMarkerMathStart,
    InlineMarkerMathEnd,
    InlineMarkerRefStart,
    InlineMarkerRefEnd,
}

impl LexSemanticTokenKind {
    /// Returns the semantic token type string for LSP.
    ///
    /// These token type names are mapped to standard TextMate scopes in editor configurations
    /// to ensure compatibility with existing themes (Neovim, VSCode, etc.).
    ///
    /// Mapping rationale (based on Lex↔Markdown mapping from lex-babel):
    /// - Session → Heading → maps to "markup.heading"
    /// - Definition → Term: Desc → maps to "variable.other.definition"
    /// - InlineStrong → bold → maps to "markup.bold"
    /// - InlineEmphasis → *italic* → maps to "markup.italic"
    /// - InlineCode → `code` → maps to "markup.inline.raw"
    /// - InlineMath → $math$ → maps to "constant.numeric"
    /// - Reference → \[citation\] → maps to "markup.underline.link"
    /// - Verbatim → ```block``` → maps to "markup.raw.block"
    /// - Annotation → <!-- comment --> → maps to "comment.block"
    /// - ListMarker → - or 1. → maps to "punctuation.definition.list"
    pub fn as_str(self) -> &'static str {
        match self {
            LexSemanticTokenKind::DocumentTitle => "DocumentTitle",
            LexSemanticTokenKind::SessionMarker => "SessionMarker",
            LexSemanticTokenKind::SessionTitleText => "SessionTitleText",
            LexSemanticTokenKind::DefinitionSubject => "DefinitionSubject",
            LexSemanticTokenKind::DefinitionContent => "DefinitionContent",
            LexSemanticTokenKind::ListMarker => "ListMarker",
            LexSemanticTokenKind::ListItemText => "ListItemText",
            LexSemanticTokenKind::AnnotationLabel => "AnnotationLabel",
            LexSemanticTokenKind::AnnotationParameter => "AnnotationParameter",
            LexSemanticTokenKind::AnnotationContent => "AnnotationContent",
            LexSemanticTokenKind::InlineStrong => "InlineStrong",
            LexSemanticTokenKind::InlineEmphasis => "InlineEmphasis",
            LexSemanticTokenKind::InlineCode => "InlineCode",
            LexSemanticTokenKind::InlineMath => "InlineMath",
            LexSemanticTokenKind::Reference => "Reference",
            LexSemanticTokenKind::ReferenceCitation => "ReferenceCitation",
            LexSemanticTokenKind::ReferenceFootnote => "ReferenceFootnote",
            LexSemanticTokenKind::VerbatimSubject => "VerbatimSubject",
            LexSemanticTokenKind::VerbatimLanguage => "VerbatimLanguage",
            LexSemanticTokenKind::VerbatimAttribute => "VerbatimAttribute",
            LexSemanticTokenKind::VerbatimContent => "VerbatimContent",
            LexSemanticTokenKind::InlineMarkerStrongStart => "InlineMarker_strong_start",
            LexSemanticTokenKind::InlineMarkerStrongEnd => "InlineMarker_strong_end",
            LexSemanticTokenKind::InlineMarkerEmphasisStart => "InlineMarker_emphasis_start",
            LexSemanticTokenKind::InlineMarkerEmphasisEnd => "InlineMarker_emphasis_end",
            LexSemanticTokenKind::InlineMarkerCodeStart => "InlineMarker_code_start",
            LexSemanticTokenKind::InlineMarkerCodeEnd => "InlineMarker_code_end",
            LexSemanticTokenKind::InlineMarkerMathStart => "InlineMarker_math_start",
            LexSemanticTokenKind::InlineMarkerMathEnd => "InlineMarker_math_end",
            LexSemanticTokenKind::InlineMarkerRefStart => "InlineMarker_ref_start",
            LexSemanticTokenKind::InlineMarkerRefEnd => "InlineMarker_ref_end",
        }
    }
}

pub const SEMANTIC_TOKEN_KINDS: &[LexSemanticTokenKind] = &[
    LexSemanticTokenKind::DocumentTitle,
    LexSemanticTokenKind::SessionMarker,
    LexSemanticTokenKind::SessionTitleText,
    LexSemanticTokenKind::DefinitionSubject,
    LexSemanticTokenKind::DefinitionContent,
    LexSemanticTokenKind::ListMarker,
    LexSemanticTokenKind::ListItemText,
    LexSemanticTokenKind::AnnotationLabel,
    LexSemanticTokenKind::AnnotationParameter,
    LexSemanticTokenKind::AnnotationContent,
    LexSemanticTokenKind::InlineStrong,
    LexSemanticTokenKind::InlineEmphasis,
    LexSemanticTokenKind::InlineCode,
    LexSemanticTokenKind::InlineMath,
    LexSemanticTokenKind::Reference,
    LexSemanticTokenKind::ReferenceCitation,
    LexSemanticTokenKind::ReferenceFootnote,
    LexSemanticTokenKind::VerbatimSubject,
    LexSemanticTokenKind::VerbatimLanguage,
    LexSemanticTokenKind::VerbatimAttribute,
    LexSemanticTokenKind::VerbatimContent,
    LexSemanticTokenKind::InlineMarkerStrongStart,
    LexSemanticTokenKind::InlineMarkerStrongEnd,
    LexSemanticTokenKind::InlineMarkerEmphasisStart,
    LexSemanticTokenKind::InlineMarkerEmphasisEnd,
    LexSemanticTokenKind::InlineMarkerCodeStart,
    LexSemanticTokenKind::InlineMarkerCodeEnd,
    LexSemanticTokenKind::InlineMarkerMathStart,
    LexSemanticTokenKind::InlineMarkerMathEnd,
    LexSemanticTokenKind::InlineMarkerRefStart,
    LexSemanticTokenKind::InlineMarkerRefEnd,
];

#[derive(Debug, Clone, PartialEq)]
pub struct LexSemanticToken {
    pub kind: LexSemanticTokenKind,
    pub range: Range,
}

pub fn collect_semantic_tokens(document: &Document) -> Vec<LexSemanticToken> {
    let mut collector = TokenCollector::new();
    collector.process_document(document);
    collector.finish()
}

struct TokenCollector {
    tokens: Vec<LexSemanticToken>,
    in_annotation: bool,
    in_definition: bool,
}

impl TokenCollector {
    fn new() -> Self {
        Self {
            tokens: Vec::new(),
            in_annotation: false,
            in_definition: false,
        }
    }

    fn finish(mut self) -> Vec<LexSemanticToken> {
        self.tokens.sort_by(|a, b| {
            let a_start = (
                &a.range.start.line,
                &a.range.start.column,
                &a.range.end.line,
                &a.range.end.column,
            );
            let b_start = (
                &b.range.start.line,
                &b.range.start.column,
                &b.range.end.line,
                &b.range.end.column,
            );
            a_start.cmp(&b_start)
        });
        self.tokens
    }

    fn push_range(&mut self, range: &Range, kind: LexSemanticTokenKind) {
        if range.span.start < range.span.end {
            self.tokens.push(LexSemanticToken {
                kind,
                range: range.clone(),
            });
        }
    }

    fn process_document(&mut self, document: &Document) {
        self.process_annotations(document.annotations());
        self.process_session(&document.root, LexSemanticTokenKind::DocumentTitle);
    }

    fn process_session(&mut self, session: &Session, title_kind: LexSemanticTokenKind) {
        // Emit separate tokens for marker and title text
        if let Some(marker) = &session.marker {
            // Emit SessionMarker token for the sequence marker
            self.push_range(&marker.location, LexSemanticTokenKind::SessionMarker);
        }

        // Emit SessionTitleText token for the title text (without marker)
        // Create a range for the title text by using the full title location
        // and adjusting if there's a marker
        if let Some(header) = session.header_location() {
            if let Some(marker) = &session.marker {
                // Calculate the title text range (after the marker)
                let marker_text = marker.as_str();
                let full_title = session.full_title();

                // Find where the marker ends in the title
                if let Some(pos) = full_title.find(marker_text) {
                    let marker_end = pos + marker_text.len();
                    // Skip whitespace after marker
                    let title_start = full_title[marker_end..]
                        .chars()
                        .position(|c| !c.is_whitespace())
                        .map(|p| marker_end + p)
                        .unwrap_or(marker_end);

                    if title_start < full_title.len() {
                        // Create range for title text only
                        use lex_core::lex::ast::Position;
                        let title_text_range = Range::new(
                            header.span.start + title_start..header.span.end,
                            Position::new(header.start.line, header.start.column + title_start),
                            header.end,
                        );
                        self.push_range(&title_text_range, title_kind);
                    }
                }
            } else {
                // No marker, the entire header is title text
                self.push_range(header, title_kind);
            }
        }

        self.process_text_content(&session.title);

        self.process_annotations(session.annotations());
        for child in session.children.iter() {
            self.process_content_item(child);
        }
    }

    fn process_content_item(&mut self, item: &ContentItem) {
        match item {
            ContentItem::Paragraph(paragraph) => self.process_paragraph(paragraph),
            ContentItem::Session(session) => {
                self.process_session(session, LexSemanticTokenKind::SessionTitleText)
            }
            ContentItem::List(list) => self.process_list(list),
            ContentItem::ListItem(list_item) => self.process_list_item(list_item),
            ContentItem::Definition(definition) => self.process_definition(definition),
            ContentItem::Annotation(annotation) => self.process_annotation(annotation),
            ContentItem::VerbatimBlock(verbatim) => self.process_verbatim(verbatim),
            ContentItem::Table(table) => self.process_table(table),
            ContentItem::TextLine(text_line) => self.process_text_content(&text_line.content),
            ContentItem::VerbatimLine(_) => {}
            ContentItem::BlankLineGroup(_) => {}
        }
    }

    fn process_paragraph(&mut self, paragraph: &Paragraph) {
        for line in &paragraph.lines {
            if let ContentItem::TextLine(text_line) = line {
                // Don't emit full-line tokens for DefinitionContent or AnnotationContent
                // as they overlap with inline tokens. The context is already clear from
                // the DefinitionSubject and AnnotationLabel tokens.
                self.process_text_content(&text_line.content);
            }
        }
        self.process_annotations(paragraph.annotations());
    }

    fn process_list(&mut self, list: &List) {
        self.process_annotations(list.annotations());
        for item in list.items.iter() {
            if let ContentItem::ListItem(list_item) = item {
                self.process_list_item(list_item);
            }
        }
    }

    fn process_list_item(&mut self, list_item: &ListItem) {
        if let Some(marker_range) = &list_item.marker.location {
            self.push_range(marker_range, LexSemanticTokenKind::ListMarker);
        }
        for text in &list_item.text {
            if let Some(location) = &text.location {
                self.push_range(location, LexSemanticTokenKind::ListItemText);
            }
            self.process_text_content(text);
        }
        self.process_annotations(list_item.annotations());
        for child in list_item.children.iter() {
            self.process_content_item(child);
        }
    }

    fn process_definition(&mut self, definition: &Definition) {
        if let Some(header) = definition.header_location() {
            self.push_range(header, LexSemanticTokenKind::DefinitionSubject);
        }
        self.process_text_content(&definition.subject);
        self.process_annotations(definition.annotations());
        let was_in_definition = self.in_definition;
        self.in_definition = true;
        for child in definition.children.iter() {
            self.process_content_item(child);
        }
        self.in_definition = was_in_definition;
    }

    fn process_verbatim(&mut self, verbatim: &Verbatim) {
        for group in verbatim.group() {
            self.process_text_content(group.subject);
            if let Some(location) = &group.subject.location {
                self.push_range(location, LexSemanticTokenKind::VerbatimSubject);
            }
            for child in group.children {
                if let ContentItem::VerbatimLine(line) = child {
                    self.push_range(&line.location, LexSemanticTokenKind::VerbatimContent);
                }
            }
        }

        self.push_range(
            &verbatim.closing_data.label.location,
            LexSemanticTokenKind::VerbatimLanguage,
        );
        for parameter in &verbatim.closing_data.parameters {
            self.push_range(&parameter.location, LexSemanticTokenKind::VerbatimAttribute);
        }

        self.process_annotations(verbatim.annotations());
    }

    fn process_table(&mut self, table: &Table) {
        self.process_text_content(&table.subject);
        if let Some(location) = &table.subject.location {
            self.push_range(location, LexSemanticTokenKind::VerbatimSubject);
        }

        // Process cell content: inline text and block children
        for row in table.all_rows() {
            for cell in &row.cells {
                self.process_text_content(&cell.content);
                for child in cell.children.iter() {
                    self.process_content_item(child);
                }
            }
        }

        self.push_range(
            &table.closing_data.label.location,
            LexSemanticTokenKind::VerbatimLanguage,
        );
        for parameter in &table.closing_data.parameters {
            self.push_range(&parameter.location, LexSemanticTokenKind::VerbatimAttribute);
        }

        self.process_annotations(table.annotations());
    }

    fn process_annotation(&mut self, annotation: &Annotation) {
        self.push_range(
            annotation.header_location(),
            LexSemanticTokenKind::AnnotationLabel,
        );
        for parameter in &annotation.data.parameters {
            self.push_range(
                &parameter.location,
                LexSemanticTokenKind::AnnotationParameter,
            );
        }
        let was_in_annotation = self.in_annotation;
        self.in_annotation = true;
        for child in annotation.children.iter() {
            self.process_content_item(child);
        }
        self.in_annotation = was_in_annotation;
    }

    fn process_annotations(&mut self, annotations: &[Annotation]) {
        for annotation in annotations {
            self.process_annotation(annotation);
        }
    }

    fn process_text_content(&mut self, text: &TextContent) {
        let Some(base_range) = text.location.as_ref() else {
            return;
        };
        let raw = text.as_string();
        if raw.is_empty() {
            return;
        }
        let nodes = text.inline_items();
        let mut walker = InlineWalker {
            raw,
            base_range,
            cursor: 0,
            tokens: &mut self.tokens,
            in_annotation: self.in_annotation,
            in_definition: self.in_definition,
            in_formatted: false,
        };
        walker.walk_nodes(&nodes);
    }
}

/// Walks the InlineNode tree and raw text in parallel to produce positioned semantic tokens.
///
/// The inline parser consumes escape sequences and delimiters, so InlineNode text doesn't
/// directly correspond to byte offsets in the raw source. This walker maintains a cursor
/// into the raw text and advances it according to the same rules the inline parser uses,
/// producing correctly positioned Range values for each token.
struct InlineWalker<'a> {
    raw: &'a str,
    base_range: &'a Range,
    cursor: usize,
    tokens: &'a mut Vec<LexSemanticToken>,
    in_annotation: bool,
    in_definition: bool,
    /// True when inside a formatting container (Strong/Emphasis). Plain text inside
    /// containers is covered by the container's content span, so context-dependent
    /// tokens (AnnotationContent, DefinitionContent) are suppressed.
    in_formatted: bool,
}

impl<'a> InlineWalker<'a> {
    fn walk_nodes(&mut self, nodes: &[InlineNode]) {
        for node in nodes {
            self.walk_node(node);
        }
    }

    fn walk_node(&mut self, node: &InlineNode) {
        match node {
            InlineNode::Plain { text, .. } => self.walk_plain(text),
            InlineNode::Strong { content, .. } => self.walk_container(
                content,
                '*',
                LexSemanticTokenKind::InlineStrong,
                LexSemanticTokenKind::InlineMarkerStrongStart,
                LexSemanticTokenKind::InlineMarkerStrongEnd,
            ),
            InlineNode::Emphasis { content, .. } => self.walk_container(
                content,
                '_',
                LexSemanticTokenKind::InlineEmphasis,
                LexSemanticTokenKind::InlineMarkerEmphasisStart,
                LexSemanticTokenKind::InlineMarkerEmphasisEnd,
            ),
            InlineNode::Code { text, .. } => self.walk_literal(
                text,
                '`',
                LexSemanticTokenKind::InlineCode,
                LexSemanticTokenKind::InlineMarkerCodeStart,
                LexSemanticTokenKind::InlineMarkerCodeEnd,
            ),
            InlineNode::Math { text, .. } => self.walk_literal(
                text,
                '#',
                LexSemanticTokenKind::InlineMath,
                LexSemanticTokenKind::InlineMarkerMathStart,
                LexSemanticTokenKind::InlineMarkerMathEnd,
            ),
            InlineNode::Reference { data, .. } => self.walk_reference(data),
        }
    }

    /// Walk a Plain text node, advancing cursor through escape sequences in raw text.
    /// Emits AnnotationContent or DefinitionContent when inside those contexts.
    fn walk_plain(&mut self, text: &str) {
        let start = self.cursor;
        self.advance_unescaped(text);
        let end = self.cursor;

        if start < end {
            let kind = if self.in_formatted {
                None // Covered by the container's content span
            } else if self.in_annotation {
                Some(LexSemanticTokenKind::AnnotationContent)
            } else if self.in_definition {
                Some(LexSemanticTokenKind::DefinitionContent)
            } else {
                None
            };
            if let Some(kind) = kind {
                self.push(self.make_range(start, end), kind);
            }
        }
    }

    /// Walk a container node (Strong/Emphasis) which has an opening marker, children, and closing marker.
    fn walk_container(
        &mut self,
        content: &[InlineNode],
        marker: char,
        content_kind: LexSemanticTokenKind,
        start_marker_kind: LexSemanticTokenKind,
        end_marker_kind: LexSemanticTokenKind,
    ) {
        let marker_len = marker.len_utf8();

        // Opening marker
        let marker_start = self.cursor;
        self.cursor += marker_len;
        self.push(
            self.make_range(marker_start, self.cursor),
            start_marker_kind,
        );

        // Recurse into children — record span boundaries for the content token
        let content_start = self.cursor;
        let was_in_formatted = self.in_formatted;
        self.in_formatted = true;
        self.walk_nodes(content);
        self.in_formatted = was_in_formatted;
        let content_end = self.cursor;

        // Emit a single content span covering all children
        if content_start < content_end {
            self.push(self.make_range(content_start, content_end), content_kind);
        }

        // Closing marker
        let close_start = self.cursor;
        self.cursor += marker_len;
        self.push(self.make_range(close_start, self.cursor), end_marker_kind);
    }

    /// Walk a literal node (Code/Math) — no escape processing inside.
    fn walk_literal(
        &mut self,
        text: &str,
        marker: char,
        content_kind: LexSemanticTokenKind,
        start_marker_kind: LexSemanticTokenKind,
        end_marker_kind: LexSemanticTokenKind,
    ) {
        let marker_len = marker.len_utf8();

        // Opening marker
        let marker_start = self.cursor;
        self.cursor += marker_len;
        self.push(
            self.make_range(marker_start, self.cursor),
            start_marker_kind,
        );

        // Literal content (verbatim, no escape processing)
        let content_start = self.cursor;
        self.cursor += text.len();
        if content_start < self.cursor {
            self.push(self.make_range(content_start, self.cursor), content_kind);
        }

        // Closing marker
        let close_start = self.cursor;
        self.cursor += marker_len;
        self.push(self.make_range(close_start, self.cursor), end_marker_kind);
    }

    /// Walk a Reference node — literal content wrapped in `[` `]`.
    fn walk_reference(&mut self, data: &lex_core::lex::inlines::ReferenceInline) {
        let ref_kind = match &data.reference_type {
            ReferenceType::Citation(_) => LexSemanticTokenKind::ReferenceCitation,
            ReferenceType::FootnoteNumber { .. } | ReferenceType::FootnoteLabeled { .. } => {
                LexSemanticTokenKind::ReferenceFootnote
            }
            _ => LexSemanticTokenKind::Reference,
        };

        // Opening bracket
        let open_start = self.cursor;
        self.cursor += 1;
        self.push(
            self.make_range(open_start, self.cursor),
            LexSemanticTokenKind::InlineMarkerRefStart,
        );

        // Reference content (literal — matches raw verbatim)
        let content_start = self.cursor;
        self.cursor += data.raw.len();
        if content_start < self.cursor {
            self.push(self.make_range(content_start, self.cursor), ref_kind);
        }

        // Closing bracket
        let close_start = self.cursor;
        self.cursor += 1;
        self.push(
            self.make_range(close_start, self.cursor),
            LexSemanticTokenKind::InlineMarkerRefEnd,
        );
    }

    /// Advance the raw-text cursor to match unescaped `text` from an InlineNode::Plain.
    ///
    /// The inline parser applies escape rules: `\*` → `*`, `\\` → `\`, but `\n` stays `\n`.
    /// This function mirrors that logic to track how many raw bytes correspond to each
    /// unescaped character.
    fn advance_unescaped(&mut self, text: &str) {
        for expected in text.chars() {
            if self.cursor >= self.raw.len() {
                break;
            }
            let raw_ch = self.raw[self.cursor..].chars().next().unwrap();
            if raw_ch == '\\' {
                if self.cursor + 1 >= self.raw.len() {
                    // Trailing backslash: treat as literal to mirror parser behavior and
                    // avoid out-of-bounds slicing on `self.raw[self.cursor + 1..]`.
                    self.cursor += 1;
                } else {
                    let next_ch = self.raw[self.cursor + 1..].chars().next();
                    match next_ch {
                        Some(nc) if !nc.is_alphanumeric() => {
                            // Escaped: raw `\X` maps to unescaped `X`
                            self.cursor += 1 + nc.len_utf8();
                        }
                        _ => {
                            // Literal backslash: raw `\` stays as `\` in the node
                            self.cursor += 1;
                        }
                    }
                }
            } else {
                self.cursor += raw_ch.len_utf8();
            }
            let _ = expected; // cursor already advanced
        }
    }

    fn make_range(&self, start: usize, end: usize) -> Range {
        let start_pos = self.position_at(start);
        let end_pos = self.position_at(end);
        Range::new(
            (self.base_range.span.start + start)..(self.base_range.span.start + end),
            start_pos,
            end_pos,
        )
    }

    fn position_at(&self, offset: usize) -> Position {
        let mut line = self.base_range.start.line;
        let mut column = self.base_range.start.column;
        for ch in self.raw[..offset].chars() {
            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += ch.len_utf8();
            }
        }
        Position::new(line, column)
    }

    fn push(&mut self, range: Range, kind: LexSemanticTokenKind) {
        if range.span.start < range.span.end {
            self.tokens.push(LexSemanticToken { kind, range });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{sample_document, sample_source};
    use lex_core::lex::testing::lexplore::Lexplore;

    fn snippets(
        tokens: &[LexSemanticToken],
        kind: LexSemanticTokenKind,
        source: &str,
    ) -> Vec<String> {
        tokens
            .iter()
            .filter(|token| token.kind == kind)
            .map(|token| source[token.range.span.clone()].to_string())
            .collect()
    }

    #[test]
    fn collects_structural_tokens() {
        let document = sample_document();
        let tokens = collect_semantic_tokens(&document);
        let source = sample_source();

        // Session titles are now split into SessionMarker and SessionTitleText
        assert!(
            snippets(&tokens, LexSemanticTokenKind::SessionMarker, source)
                .iter()
                .any(|snippet| snippet.trim() == "1.")
        );
        assert!(
            snippets(&tokens, LexSemanticTokenKind::SessionTitleText, source)
                .iter()
                .any(|snippet| snippet.trim() == "Intro")
        );
        // Cache is parsed as VerbatimSubject
        assert!(
            snippets(&tokens, LexSemanticTokenKind::VerbatimSubject, source)
                .iter()
                .any(|snippet| snippet.trim_end() == "Cache")
        );
        let markers = snippets(&tokens, LexSemanticTokenKind::ListMarker, source);
        assert_eq!(markers.len(), 4);
        assert!(markers
            .iter()
            .all(|snippet| snippet.trim_start().starts_with('-')
                || snippet.trim_start().chars().next().unwrap().is_numeric()));
        let annotation_labels = snippets(&tokens, LexSemanticTokenKind::AnnotationLabel, source);
        assert!(annotation_labels
            .iter()
            .any(|snippet| snippet.contains("doc.note")));
        let parameters = snippets(&tokens, LexSemanticTokenKind::AnnotationParameter, source);
        assert!(parameters
            .iter()
            .any(|snippet| snippet.contains("severity=info")));
        let verbatim_subjects = snippets(&tokens, LexSemanticTokenKind::VerbatimSubject, source);
        assert!(verbatim_subjects
            .iter()
            .any(|snippet| snippet.contains("CLI Example")));
        assert!(
            snippets(&tokens, LexSemanticTokenKind::VerbatimLanguage, source)
                .iter()
                .any(|snippet| snippet.contains("shell"))
        );
    }

    #[test]
    fn collects_inline_tokens() {
        let document = sample_document();
        let tokens = collect_semantic_tokens(&document);
        let source = sample_source();
        assert!(
            snippets(&tokens, LexSemanticTokenKind::InlineStrong, source)
                .iter()
                .any(|snippet| snippet.contains("Lex"))
        );
        assert!(
            snippets(&tokens, LexSemanticTokenKind::InlineEmphasis, source)
                .iter()
                .any(|snippet| snippet.contains("format"))
        );
        assert!(snippets(&tokens, LexSemanticTokenKind::InlineCode, source)
            .iter()
            .any(|snippet| snippet.contains("code")));
        assert!(snippets(&tokens, LexSemanticTokenKind::InlineMath, source)
            .iter()
            .any(|snippet| snippet.contains("math")));
    }

    #[test]
    fn classifies_references() {
        let document = sample_document();
        let tokens = collect_semantic_tokens(&document);
        let source = sample_source();
        assert!(
            snippets(&tokens, LexSemanticTokenKind::ReferenceCitation, source)
                .iter()
                .any(|snippet| snippet.contains("@spec2025"))
        );
        assert!(
            snippets(&tokens, LexSemanticTokenKind::ReferenceFootnote, source)
                .iter()
                .any(|snippet| snippet.contains("^source"))
        );
        assert!(
            snippets(&tokens, LexSemanticTokenKind::ReferenceFootnote, source)
                .iter()
                .any(|snippet| snippet.contains("1"))
        );
        assert!(snippets(&tokens, LexSemanticTokenKind::Reference, source)
            .iter()
            .any(|snippet| snippet.contains("Cache")));
    }

    #[test]
    fn empty_document_has_no_tokens() {
        let document = Lexplore::benchmark(0)
            .parse()
            .expect("failed to parse empty benchmark fixture");
        let tokens = collect_semantic_tokens(&document);
        assert!(tokens.is_empty());
    }

    #[test]
    fn emits_annotation_content_for_inline_annotation() {
        let document = sample_document();
        let tokens = collect_semantic_tokens(&document);
        let source = sample_source();

        // The fixture starts with `:: doc.note severity=info :: Document preface.`
        // "Document preface." is inline annotation content — plain text inside annotation context.
        let annotation_content = snippets(&tokens, LexSemanticTokenKind::AnnotationContent, source);
        assert!(
            annotation_content
                .iter()
                .any(|snippet| snippet.contains("Document preface")),
            "AnnotationContent should be emitted for plain text inside annotations, got: {annotation_content:?}"
        );
    }

    #[test]
    fn annotation_content_excludes_formatted_text() {
        // Inline formatting within annotation context should get its own token type,
        // not AnnotationContent — only Plain nodes emit AnnotationContent.
        let source = ":: note :: Some *bold* text.\n";
        let document = lex_core::lex::parsing::parse_document(source).expect("failed to parse");
        let tokens = collect_semantic_tokens(&document);

        let annotation_content: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == LexSemanticTokenKind::AnnotationContent)
            .map(|t| &source[t.range.span.clone()])
            .collect();

        // "Some " and " text." should be AnnotationContent, but "bold" should not
        assert!(
            annotation_content.iter().any(|s| s.contains("Some")),
            "Plain text before formatting should be AnnotationContent"
        );
        assert!(
            annotation_content.iter().any(|s| s.contains("text.")),
            "Plain text after formatting should be AnnotationContent"
        );
        assert!(
            !annotation_content.iter().any(|s| s.contains("bold")),
            "Formatted text should NOT be AnnotationContent"
        );

        // "bold" should be InlineStrong
        let strong: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == LexSemanticTokenKind::InlineStrong)
            .map(|t| &source[t.range.span.clone()])
            .collect();
        assert!(strong.contains(&"bold"));
    }

    #[test]
    fn table_cell_inline_formatting_gets_tokens() {
        let source = "Stats:\n    | *Name* | `code` |\n    | _test_ | #42#   |\n:: table ::\n";
        let document = lex_core::lex::parsing::parse_document(source).expect("failed to parse");
        let tokens = collect_semantic_tokens(&document);

        let strong = snippets(&tokens, LexSemanticTokenKind::InlineStrong, source);
        assert!(
            strong.iter().any(|s| s.contains("Name")),
            "Expected InlineStrong for *Name* in table cell, got: {strong:?}"
        );

        let code = snippets(&tokens, LexSemanticTokenKind::InlineCode, source);
        assert!(
            code.iter().any(|s| s.contains("code")),
            "Expected InlineCode for `code` in table cell, got: {code:?}"
        );

        let emphasis = snippets(&tokens, LexSemanticTokenKind::InlineEmphasis, source);
        assert!(
            emphasis.iter().any(|s| s.contains("test")),
            "Expected InlineEmphasis for _test_ in table cell, got: {emphasis:?}"
        );

        let math = snippets(&tokens, LexSemanticTokenKind::InlineMath, source);
        assert!(
            math.iter().any(|s| s.contains("42")),
            "Expected InlineMath for #42# in table cell, got: {math:?}"
        );
    }
}
