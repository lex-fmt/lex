//! AST Snapshot - a normalized intermediate representation of the AST tree
//!
//! This module provides a canonical, format-agnostic representation of the AST
//! suitable for serialization to any output format (JSON, YAML, treeviz, tag, etc.)
//!
//! The snapshot captures the complete tree structure with node types, labels,
//! attributes, and children - allowing each serializer to focus solely on
//! presentation without reimplementing AST traversal logic.
//!
//! ## Building Snapshots
//!
//! This module provides the canonical AST traversal that creates a normalized snapshot
//! representation of the entire tree. All serializers should consume the output
//! of `snapshot_from_document()` or `snapshot_from_content()` rather than reimplementing
//! traversal logic.

use super::trait_helpers::get_visual_header;
use super::traits::{AstNode, Container};
use super::{
    Annotation, ContentItem, Definition, Document, List, ListItem, Paragraph, Range, Session,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A snapshot of an AST node in a normalized, serializable form
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AstSnapshot {
    /// The type of node (e.g., "Paragraph", "Session", "List")
    pub node_type: String,

    /// The primary label or text content of the node
    pub label: String,

    /// Additional attributes specific to the node type
    pub attributes: HashMap<String, String>,

    /// The source range of the node
    pub range: Range,

    /// Child nodes in the tree
    pub children: Vec<AstSnapshot>,
}

impl AstSnapshot {
    /// Create a new snapshot with the given node type and label
    pub fn new(node_type: String, label: String, range: Range) -> Self {
        Self {
            node_type,
            label,
            attributes: HashMap::new(),
            range,
            children: Vec::new(),
        }
    }

    /// Add an attribute to this snapshot
    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.insert(key, value);
        self
    }

    /// Add a child snapshot
    pub fn with_child(mut self, child: AstSnapshot) -> Self {
        self.children.push(child);
        self
    }

    /// Add multiple children
    pub fn with_children(mut self, children: Vec<AstSnapshot>) -> Self {
        self.children.extend(children);
        self
    }
}

// ============================================================================
// Snapshot Building Functions
// ============================================================================

/// Create a snapshot of a single AST node and all its children
///
/// This function recursively builds a complete snapshot tree for a node and all its descendants.
pub fn snapshot_node<T: AstNode>(node: &T) -> AstSnapshot {
    // We match on concrete types here - since this is called with concrete types from ContentItem,
    // we don't need to do any casting
    let node_type = node.node_type();
    let label = node.display_label();

    // For container types, we need to visit children
    // But without unsafe casting, we can only do this if we have the concrete type
    // This is a limitation of the generic approach
    //
    // The solution: use ContentItem enum variants directly in callers
    // See snapshot_from_content below

    AstSnapshot::new(node_type.to_string(), label, node.range().clone())
}

/// Build snapshot from a concrete ContentItem enum
///
/// This is the preferred way to call the snapshot builder since it avoids unsafe casting.
pub fn snapshot_from_content(item: &ContentItem) -> AstSnapshot {
    snapshot_from_content_with_options(item, false)
}

/// Build snapshot from a concrete ContentItem enum with options
///
/// When `include_all` is true, all AST node properties (annotations, labels, parameters, etc.)
/// are included as children in the snapshot.
pub fn snapshot_from_content_with_options(item: &ContentItem, include_all: bool) -> AstSnapshot {
    match item {
        ContentItem::Session(session) => build_session_snapshot(session, include_all),
        ContentItem::Paragraph(para) => build_paragraph_snapshot(para, include_all),
        ContentItem::List(list) => build_list_snapshot(list, include_all),
        ContentItem::ListItem(li) => build_list_item_snapshot(li, include_all),
        ContentItem::Definition(def) => build_definition_snapshot(def, include_all),
        ContentItem::VerbatimBlock(fb) => build_verbatim_block_snapshot(fb, include_all),
        ContentItem::VerbatimLine(fl) => AstSnapshot::new(
            "VerbatimLine".to_string(),
            fl.display_label(),
            fl.range().clone(),
        ),
        ContentItem::Annotation(ann) => build_annotation_snapshot(ann, include_all),
        ContentItem::TextLine(tl) => AstSnapshot::new(
            "TextLine".to_string(),
            tl.display_label(),
            tl.range().clone(),
        ),
        ContentItem::BlankLineGroup(blg) => AstSnapshot::new(
            "BlankLineGroup".to_string(),
            blg.display_label(),
            blg.range().clone(),
        ),
    }
}

/// Build a snapshot for the document root, flattening the root session
///
/// When `include_all` is false: Document-level annotations are not included in this snapshot.
/// This reflects the document structure where annotations are separate from content.
/// When `include_all` is true: All nodes including annotations are included.
///
/// The root session is flattened so its children appear as direct children of the Document.
pub fn snapshot_from_document(doc: &Document) -> AstSnapshot {
    snapshot_from_document_with_options(doc, false)
}

/// Build a snapshot for the document root with options for controlling what's included
///
/// When `include_all` is false: Document-level annotations are not included in this snapshot.
/// When `include_all` is true: All nodes including annotations are included.
///
/// The root session is flattened so its children appear as direct children of the Document.
pub fn snapshot_from_document_with_options(doc: &Document, include_all: bool) -> AstSnapshot {
    let mut snapshot = AstSnapshot::new(
        "Document".to_string(),
        format!(
            "Document ({} annotations, {} items)",
            doc.annotations.len(),
            doc.root.children.len()
        ),
        doc.root.range().clone(),
    );

    // If include_all is true, include document-level annotations
    if include_all {
        for annotation in &doc.annotations {
            snapshot.children.push(snapshot_from_content_with_options(
                &ContentItem::Annotation(annotation.clone()),
                include_all,
            ));
        }
    }

    // Flatten the root session - its children become direct children of the Document
    for child in &doc.root.children {
        snapshot
            .children
            .push(snapshot_from_content_with_options(child, include_all));
    }

    snapshot
}

fn build_session_snapshot(session: &Session, include_all: bool) -> AstSnapshot {
    let item = ContentItem::Session(session.clone());
    let mut snapshot = AstSnapshot::new(
        "Session".to_string(),
        session.display_label(),
        session.range().clone(),
    );

    // If include_all, use trait helper to get visual header
    if include_all {
        if let Some(header) = get_visual_header(&item) {
            snapshot.children.push(AstSnapshot::new(
                "SessionTitle".to_string(),
                header,
                session.range().clone(), // Title shares range with session for now
            ));
        }
    }

    // If include_all, show session annotations
    if include_all {
        for ann in &session.annotations {
            snapshot.children.push(snapshot_from_content_with_options(
                &ContentItem::Annotation(ann.clone()),
                include_all,
            ));
        }
    }

    // Show main children
    for child in session.children() {
        snapshot
            .children
            .push(snapshot_from_content_with_options(child, include_all));
    }
    snapshot
}

fn build_paragraph_snapshot(para: &Paragraph, include_all: bool) -> AstSnapshot {
    let mut snapshot = AstSnapshot::new(
        "Paragraph".to_string(),
        para.display_label(),
        para.range().clone(),
    );
    for line in &para.lines {
        snapshot
            .children
            .push(snapshot_from_content_with_options(line, include_all));
    }
    snapshot
}

fn build_list_snapshot(list: &List, include_all: bool) -> AstSnapshot {
    let mut snapshot = AstSnapshot::new(
        "List".to_string(),
        list.display_label(),
        list.range().clone(),
    );
    for item in &list.items {
        snapshot
            .children
            .push(snapshot_from_content_with_options(item, include_all));
    }
    snapshot
}

fn build_list_item_snapshot(item: &ListItem, include_all: bool) -> AstSnapshot {
    let mut snapshot = AstSnapshot::new(
        "ListItem".to_string(),
        item.display_label(),
        item.range().clone(),
    );

    // If include_all, show the marker and text
    if include_all {
        snapshot.children.push(AstSnapshot::new(
            "Marker".to_string(),
            item.marker.as_string().to_string(),
            item.range().clone(), // Marker shares range with item for now
        ));

        for text_part in item.text.iter() {
            snapshot.children.push(AstSnapshot::new(
                "Text".to_string(),
                text_part.as_string().to_string(),
                item.range().clone(), // Text shares range with item for now
            ));
        }

        // Show list item annotations
        for ann in &item.annotations {
            snapshot.children.push(snapshot_from_content_with_options(
                &ContentItem::Annotation(ann.clone()),
                include_all,
            ));
        }
    }

    // Show main children
    for child in item.children() {
        snapshot
            .children
            .push(snapshot_from_content_with_options(child, include_all));
    }
    snapshot
}

fn build_definition_snapshot(def: &Definition, include_all: bool) -> AstSnapshot {
    let item = ContentItem::Definition(def.clone());
    let mut snapshot = AstSnapshot::new(
        "Definition".to_string(),
        def.display_label(),
        def.range().clone(),
    );

    // If include_all, use trait helper to get visual header
    if include_all {
        if let Some(header) = get_visual_header(&item) {
            snapshot.children.push(AstSnapshot::new(
                "Subject".to_string(),
                header,
                def.range().clone(), // Subject shares range with definition for now
            ));
        }

        // Show definition annotations
        for ann in &def.annotations {
            snapshot.children.push(snapshot_from_content_with_options(
                &ContentItem::Annotation(ann.clone()),
                include_all,
            ));
        }
    }

    // Show main children
    for child in def.children() {
        snapshot
            .children
            .push(snapshot_from_content_with_options(child, include_all));
    }
    snapshot
}

fn build_annotation_snapshot(ann: &Annotation, include_all: bool) -> AstSnapshot {
    let item = ContentItem::Annotation(ann.clone());
    let mut snapshot = AstSnapshot::new(
        "Annotation".to_string(),
        ann.display_label(),
        ann.range().clone(),
    );

    // If include_all, use trait helper for label, keep parameter handling special
    if include_all {
        if let Some(header) = get_visual_header(&item) {
            snapshot.children.push(AstSnapshot::new(
                "Label".to_string(),
                header,
                ann.range().clone(), // Label shares range with annotation for now
            ));
        }

        // Parameters need special handling (not in Container trait)
        for param in &ann.data.parameters {
            snapshot.children.push(AstSnapshot::new(
                "Parameter".to_string(),
                format!("{}={}", param.key, param.value),
                ann.range().clone(), // Parameter shares range with annotation for now
            ));
        }
    }

    // Show main children
    for child in ann.children() {
        snapshot
            .children
            .push(snapshot_from_content_with_options(child, include_all));
    }
    snapshot
}

fn build_verbatim_block_snapshot(fb: &super::Verbatim, include_all: bool) -> AstSnapshot {
    let group_count = fb.group_len();
    let group_word = if group_count == 1 { "group" } else { "groups" };
    let label = format!("{} ({} {})", fb.display_label(), group_count, group_word);
    let mut snapshot = AstSnapshot::new("VerbatimBlock".to_string(), label, fb.range().clone());

    for (idx, group) in fb.group().enumerate() {
        let label = if group_count == 1 {
            group.subject.as_string().to_string()
        } else {
            format!(
                "{} (group {} of {})",
                group.subject.as_string(),
                idx + 1,
                group_count
            )
        };
        let mut group_snapshot = AstSnapshot::new(
            "VerbatimGroup".to_string(),
            label,
            fb.range().clone(), // Group shares range with block for now
        );
        for child in group.children.iter() {
            group_snapshot
                .children
                .push(snapshot_from_content_with_options(child, include_all));
        }
        snapshot.children.push(group_snapshot);
    }

    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::ast::elements::annotation::Annotation;
    use crate::lex::ast::elements::paragraph::Paragraph;
    use crate::lex::ast::elements::session::Session;
    use crate::lex::ast::elements::typed_content::ContentElement;

    #[test]
    fn test_snapshot_from_document_empty() {
        let doc = Document::new();
        let snapshot = snapshot_from_document(&doc);

        assert_eq!(snapshot.node_type, "Document");
        assert_eq!(snapshot.label, "Document (0 annotations, 0 items)");
        assert!(snapshot.children.is_empty());
    }

    #[test]
    fn test_snapshot_from_document_with_content() {
        let mut doc = Document::new();
        doc.root
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Test".to_string(),
            )));
        doc.root
            .children
            .push(ContentItem::Session(Session::with_title(
                "Section".to_string(),
            )));

        let snapshot = snapshot_from_document(&doc);

        assert_eq!(snapshot.node_type, "Document");
        assert_eq!(snapshot.label, "Document (0 annotations, 2 items)");
        assert_eq!(snapshot.children.len(), 2);
        assert_eq!(snapshot.children[0].node_type, "Paragraph");
        assert_eq!(snapshot.children[1].node_type, "Session");
    }

    #[test]
    fn test_snapshot_excludes_annotations() {
        use crate::lex::ast::elements::label::Label;

        let annotation = Annotation::new(
            Label::new("test-label".to_string()),
            vec![],
            Vec::<ContentElement>::new(),
        );
        let doc = Document::with_annotations_and_content(
            vec![annotation],
            vec![ContentItem::Paragraph(Paragraph::from_line(
                "Test".to_string(),
            ))],
        );

        let snapshot = snapshot_from_document(&doc);

        assert_eq!(snapshot.label, "Document (1 annotations, 1 items)");
        // Metadata should not appear as children - they are kept separate
        assert_eq!(snapshot.children.len(), 1);
        assert_eq!(snapshot.children[0].node_type, "Paragraph");
        // Verify no Annotation nodes in children
        assert!(snapshot
            .children
            .iter()
            .all(|child| child.node_type != "Annotation"));
    }

    #[test]
    fn test_snapshot_from_document_preserves_structure() {
        let mut session = Session::with_title("Main".to_string());
        session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Para 1".to_string(),
            )));

        let mut doc = Document::new();
        doc.root.children.push(ContentItem::Session(session));

        let snapshot = snapshot_from_document(&doc);

        assert_eq!(snapshot.node_type, "Document");
        assert_eq!(snapshot.children.len(), 1);

        let session_snapshot = &snapshot.children[0];
        assert_eq!(session_snapshot.node_type, "Session");
        assert_eq!(session_snapshot.children.len(), 1);
        assert_eq!(session_snapshot.children[0].node_type, "Paragraph");
    }
}
