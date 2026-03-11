use lex_babel::format::Format;
use lex_babel::formats::markdown::MarkdownFormat;
use lex_core::lex::ast::elements::Session;
use lex_core::lex::ast::ContentItem;

#[test]
fn test_session_hierarchy() {
    // H1 is reserved for document title, use H2+ for sessions
    let md = r#"
## Level 1
Content 1

### Level 2
Content 2

#### Level 3
Content 3

## Level 1 again
Content 1b
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");

    // Verify hierarchy
    // Root -> Session(Level 1) -> Session(Level 2) -> Session(Level 3)
    // Root -> Session(Level 1 again)

    let root_children = &doc.root.children;
    assert_eq!(root_children.len(), 2);

    // First Level 1
    let s1 = match &root_children[0] {
        ContentItem::Session(s) => s,
        _ => panic!("Expected Session"),
    };
    assert!(s1.title.as_string().contains("Level 1"));
    assert_eq!(s1.children.len(), 2); // Content 1 + Level 2 Session

    // Level 2
    let s2 = match &s1.children[1] {
        // Index 1 because index 0 is "Content 1" paragraph
        ContentItem::Session(s) => s,
        _ => panic!("Expected Session"),
    };
    assert!(s2.title.as_string().contains("Level 2"));
    assert_eq!(s2.children.len(), 2); // Content 2 + Level 3 Session

    // Level 3
    let s3 = match &s2.children[1] {
        ContentItem::Session(s) => s,
        _ => panic!("Expected Session"),
    };
    assert!(s3.title.as_string().contains("Level 3"));
    assert_eq!(s3.children.len(), 1); // Content 3

    // Second Level 1
    let s1b = match &root_children[1] {
        ContentItem::Session(s) => s,
        _ => panic!("Expected Session"),
    };
    assert!(s1b.title.as_string().contains("Level 1 again"));
}

#[test]
fn test_mixed_levels() {
    // H1 is reserved for document title, use H2+ for sessions
    let md = r#"
## Level 1
### Level 2
#### Level 3
### Level 2 again
"#;

    let doc = MarkdownFormat.parse(md).expect("Failed to parse markdown");
    let root_children = &doc.root.children;

    let s1 = match &root_children[0] {
        ContentItem::Session(s) => s,
        _ => panic!("Expected Session"),
    };

    // Level 1 should have two children: Level 2 and Level 2 again
    // Note: They might be interleaved with empty content if parser emits it, but let's check count of sessions

    let sessions: Vec<&Session> = s1
        .children
        .iter()
        .filter_map(|c| {
            if let ContentItem::Session(s) = c {
                Some(s)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(sessions.len(), 2);
    assert!(sessions[0].title.as_string().contains("Level 2"));
    assert!(sessions[1].title.as_string().contains("Level 2 again"));

    // Check Level 3 inside first Level 2
    let s2 = sessions[0];
    let s2_children: Vec<&Session> = s2
        .children
        .iter()
        .filter_map(|c| {
            if let ContentItem::Session(s) = c {
                Some(s)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(s2_children.len(), 1);
    assert!(s2_children[0].title.as_string().contains("Level 3"));
}
