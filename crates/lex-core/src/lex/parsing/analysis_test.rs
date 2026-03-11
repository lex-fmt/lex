
#[cfg(test)]
mod analysis_tests {
    use crate::lex::parsing::ir::NodeType;
    use crate::lex::parsing::parser::parse_with_declarative_grammar;
    use crate::lex::token::line::LineToken;
    use crate::lex::token::LineType;
    use crate::lex::token::Token;
    use crate::lex::token::LineContainer;

    fn make_line(text: &str, line_type: LineType) -> LineContainer {
        LineContainer::Token(LineToken {
            source_tokens: vec![Token::Text(text.to_string())],
            token_spans: vec![0..text.len()],
            line_type,
        })
    }

    fn make_blank() -> LineContainer {
        LineContainer::Token(LineToken {
            source_tokens: vec![Token::BlankLine(Some("\n".to_string()))],
            token_spans: vec![0..1],
            line_type: LineType::BlankLine,
        })
    }

    fn make_container(children: Vec<LineContainer>) -> LineContainer {
        LineContainer::Container {
            header: None,
            children,
        }
    }

    #[test]
    fn test_paragraph_vs_session_structure() {
        // Case 1: Title + Blank + Non-indented Content
        // Expected: Paragraph, Blank, Paragraph
        let tokens_flat = vec![
            make_line("Title", LineType::ParagraphLine),
            make_blank(),
            make_line("Content", LineType::ParagraphLine),
        ];
        
        let nodes_flat = parse_with_declarative_grammar(tokens_flat, "source").unwrap();
        println!("Flat nodes: {:?}", nodes_flat.iter().map(|n| n.node_type.clone()).collect::<Vec<_>>());
        
        assert_eq!(nodes_flat[0].node_type, NodeType::Paragraph);
        assert_eq!(nodes_flat[1].node_type, NodeType::BlankLineGroup);
        assert_eq!(nodes_flat[2].node_type, NodeType::Paragraph);

        // Case 2: Title + Indented Content (Container)
        // Expected: Session
        let tokens_nested = vec![
            make_line("Title", LineType::SubjectOrListItemLine), // Needs to be potentially a subject
            make_container(vec![
                make_line("Content", LineType::ParagraphLine)
            ]),
        ];

        let nodes_nested = parse_with_declarative_grammar(tokens_nested, "source").unwrap();
        println!("Nested nodes: {:?}", nodes_nested.iter().map(|n| n.node_type.clone()).collect::<Vec<_>>());
        
        assert_eq!(nodes_nested[0].node_type, NodeType::Session);
    }
    
    #[test]
    fn test_paragraph_followed_by_container() {
        // Case 3: ParagraphLine + Container
        // Does this become a Session or Paragraph + Container?
        // If it's a ParagraphLine, it usually doesn't start a session unless it's a SubjectLine.
        // But let's see what the parser does.
        
        let tokens = vec![
            make_line("Para", LineType::ParagraphLine),
            make_container(vec![
                make_line("Child", LineType::ParagraphLine)
            ]),
        ];
        
        let nodes = parse_with_declarative_grammar(tokens, "source").unwrap();
        println!("Para + Container nodes: {:?}", nodes.iter().map(|n| n.node_type.clone()).collect::<Vec<_>>());
    }
}
