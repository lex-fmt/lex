use crate::error::FormatError;
use crate::format::{Format, SerializedDocument};
use lex_core::lex::ast::Document;
use std::collections::HashMap;

mod parser;

pub struct RfcXmlFormat;

impl Format for RfcXmlFormat {
    fn name(&self) -> &str {
        "rfc_xml"
    }

    fn description(&self) -> &str {
        "IETF RFC XML Format (v3)"
    }

    fn file_extensions(&self) -> &[&str] {
        &["rfcxml"]
    }

    fn supports_parsing(&self) -> bool {
        true
    }

    fn supports_serialization(&self) -> bool {
        false
    }

    fn parse(&self, source: &str) -> Result<Document, FormatError> {
        // Parse XML to IR
        let ir_doc = parser::parse_to_ir(source)?;

        // Convert IR to Lex AST using the common converter
        Ok(crate::from_ir(&ir_doc))
    }

    fn serialize(&self, _doc: &Document) -> Result<String, FormatError> {
        Err(FormatError::NotSupported(
            "RFC XML serialization not implemented".to_string(),
        ))
    }

    fn serialize_with_options(
        &self,
        _doc: &Document,
        _options: &HashMap<String, String>,
    ) -> Result<SerializedDocument, FormatError> {
        Err(FormatError::NotSupported(
            "RFC XML serialization not implemented".to_string(),
        ))
    }
}
