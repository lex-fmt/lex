//! IETF RFC XML (v3) format implementation
//!
//! # Status: Experimental
//!
//! This is a parse-only proof-of-concept import for IETF RFC XML (v3)
//! documents. It is kept in-tree because it's a useful demo of
//! Lex-as-target, not because it is a release-quality format. The
//! project invests **no bespoke time** here — only improvements that
//! fall out of IR-symmetry work for free.
//!
//! Do not invest engineering effort in this format unless the v1
//! interop bar (Markdown round-trip, HTML/PDF export) is already
//! solid. See the full tiering at `comms/docs/interop-scope.lex`
//! in-repo, or
//! <https://github.com/lex-fmt/comms/blob/main/docs/interop-scope.lex>
//! on the web.

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
