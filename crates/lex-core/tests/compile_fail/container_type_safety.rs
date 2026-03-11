// Combined compile-fail test for container type safety
// Tests that GeneralContainer (used by Annotation and Definition) rejects Sessions

use lex_core::lex::ast::elements::typed_content::SessionContent;
use lex_core::lex::ast::{Annotation, Definition, Label, Session, TextContent};

fn main() {
    let label = Label::new("note".to_string());
    let session = Session::with_title("Nested".to_string());
    
    // Test 1: Annotation should reject SessionContent
    let _annotation = Annotation::new(label.clone(), vec![], vec![SessionContent::Session(session.clone())]);
    
    // Test 2: Definition should reject SessionContent  
    let subject = TextContent::from_string("Term".to_string(), None);
    let _definition = Definition::new(subject, vec![SessionContent::Session(session)]);
}
