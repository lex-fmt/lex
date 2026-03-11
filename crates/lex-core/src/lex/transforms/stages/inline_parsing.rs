use crate::lex::ast::{
    Annotation, ContentItem, Definition, List, ListItem, Paragraph, Session, TextContent, TextLine,
    Verbatim,
};
use crate::lex::transforms::{Runnable, TransformError};

/// Transform stage that walks the AST and parses inline elements for every TextContent.
pub struct ParseInlines;

impl ParseInlines {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ParseInlines {
    fn default() -> Self {
        Self::new()
    }
}

impl Runnable<Session, Session> for ParseInlines {
    fn run(&self, mut input: Session) -> Result<Session, TransformError> {
        InlineProcessor.process_session(&mut input);
        Ok(input)
    }
}

struct InlineProcessor;

impl InlineProcessor {
    fn process_session(&self, session: &mut Session) {
        self.process_text_content(&mut session.title);
        for annotation in &mut session.annotations {
            self.process_annotation(annotation);
        }
        for child in session.children.iter_mut() {
            self.process_content_item(child);
        }
    }

    fn process_definition(&self, definition: &mut Definition) {
        self.process_text_content(&mut definition.subject);
        for annotation in &mut definition.annotations {
            self.process_annotation(annotation);
        }
        for child in definition.children.iter_mut() {
            self.process_content_item(child);
        }
    }

    fn process_list(&self, list: &mut List) {
        for annotation in &mut list.annotations {
            self.process_annotation(annotation);
        }
        for child in list.items.iter_mut() {
            self.process_content_item(child);
        }
    }

    fn process_list_item(&self, item: &mut ListItem) {
        for text in item.text.iter_mut() {
            self.process_text_content(text);
        }
        for annotation in &mut item.annotations {
            self.process_annotation(annotation);
        }
        for child in item.children.iter_mut() {
            self.process_content_item(child);
        }
    }

    fn process_paragraph(&self, paragraph: &mut Paragraph) {
        for annotation in &mut paragraph.annotations {
            self.process_annotation(annotation);
        }
        for line in paragraph.lines.iter_mut() {
            self.process_content_item(line);
        }
    }

    fn process_text_line(&self, line: &mut TextLine) {
        self.process_text_content(&mut line.content);
    }

    fn process_verbatim(&self, verbatim: &mut Verbatim) {
        self.process_text_content(&mut verbatim.subject);
        for group in verbatim.additional_groups_mut() {
            self.process_text_content(&mut group.subject);
        }
        for annotation in &mut verbatim.annotations {
            self.process_annotation(annotation);
        }
        // Verbatim content is literal; do not parse inline for children.
    }

    fn process_annotation(&self, annotation: &mut Annotation) {
        for child in annotation.children.iter_mut() {
            self.process_content_item(child);
        }
    }

    fn process_content_item(&self, item: &mut ContentItem) {
        match item {
            ContentItem::Paragraph(paragraph) => self.process_paragraph(paragraph),
            ContentItem::Session(session) => self.process_session(session),
            ContentItem::List(list) => self.process_list(list),
            ContentItem::ListItem(list_item) => self.process_list_item(list_item),
            ContentItem::TextLine(text_line) => self.process_text_line(text_line),
            ContentItem::Definition(definition) => self.process_definition(definition),
            ContentItem::Annotation(annotation) => self.process_annotation(annotation),
            ContentItem::VerbatimBlock(verbatim) => self.process_verbatim(verbatim),
            ContentItem::VerbatimLine(_) => {}
            ContentItem::BlankLineGroup(_) => {}
        }
    }

    fn process_text_content(&self, content: &mut TextContent) {
        content.ensure_inline_parsed();
    }
}
