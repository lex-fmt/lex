//! Spec file finder.
//!
//! This module makes it easy to resolve a Lex spec file from a conceptual description
//! (i.e. Paragraph test file #3) to an actual file path, handling version number and other details.
//!
//! ## How Does it Work?
//!
//! There is a spec file root, which is $PROJECT_ROOT/comms/specs/ (via git submodule)
//!
//! From there one finds several categories of files: benchmarks, trifectas, elements, etc.
//! Some of these categories have further subcategories. Elements use per-element directories
//! named `<element>.docs` that contain numbered samples, while the canonical spec lives one
//! level up as `<element>.lex`.
//!
//! For example: get_doc_root(category, subcategory) -> path
//! - spec_file_root/category/subcategory?
//!
//! Once we've got the doc root, we need to be able to find a file by a number.
//! In these specs, files are prefixed by a number, which can take forms like 001, 01-, or 1-.
//! We iterate through the root's files, split on the first dash, and if that first part
//! can be converted to a number, that's the number. As a safety valve, if multiple files
//! would resolve to the same number (1-, 001, 0001, etc), we raise an error.
//!
//! ## This module offers:
//! - `get_doc_root(category, subcategory)` - Get directory path for a category/subcategory
//! - `list_files_by_number(path)` - Build a map of number -> filepath for a directory
//! - `find_specfile_by_number(category, subcategory, number)` - Orchestrates the above

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const SPECS_ROOT: &str = "../../comms/specs";

/// Element types that can be loaded from the per-element library
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    Paragraph,
    List,
    Session,
    Definition,
    Annotation,
    Verbatim,
    Table,
    Document,
    Footnotes,
}

/// Document collection types for comprehensive testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentType {
    Benchmark,
    Trifecta,
}

impl ElementType {
    /// Get the directory name for this element type
    pub fn dir_name(&self) -> &'static str {
        match self {
            ElementType::Paragraph => "paragraph",
            ElementType::List => "list",
            ElementType::Session => "session",
            ElementType::Definition => "definition",
            ElementType::Annotation => "annotation",
            ElementType::Verbatim => "verbatim",
            ElementType::Table => "table",
            ElementType::Document => "document",
            ElementType::Footnotes => "footnotes",
        }
    }
}

impl DocumentType {
    /// Get the directory name for this document type
    pub fn dir_name(&self) -> &'static str {
        match self {
            DocumentType::Benchmark => "benchmark",
            DocumentType::Trifecta => "trifecta",
        }
    }
}

/// Errors that can occur when finding spec files
#[derive(Debug, Clone)]
pub enum SpecFileError {
    FileNotFound(String),
    IoError(String),
    DuplicateNumber(String),
}

impl std::fmt::Display for SpecFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecFileError::FileNotFound(msg) => write!(f, "File not found: {msg}"),
            SpecFileError::IoError(msg) => write!(f, "IO error: {msg}"),
            SpecFileError::DuplicateNumber(msg) => write!(f, "Duplicate number: {msg}"),
        }
    }
}

impl std::error::Error for SpecFileError {}

impl From<std::io::Error> for SpecFileError {
    fn from(err: std::io::Error) -> Self {
        SpecFileError::IoError(err.to_string())
    }
}

/// Get the doc root path for a category and optional subcategory
///
/// # Examples
/// ```ignore
/// get_doc_root("elements", Some("paragraph")) -> "comms/specs/elements/paragraph.docs"
/// get_doc_root("benchmark", None) -> "comms/specs/benchmark"
/// ```
pub fn get_doc_root(category: &str, subcategory: Option<&str>) -> PathBuf {
    // CARGO_MANIFEST_DIR points to the crate root where specs/ lives
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let crate_root = std::path::Path::new(manifest_dir);

    let mut path = crate_root.join(SPECS_ROOT);
    path.push(category);
    if let Some(subcat) = subcategory {
        if category == "elements" {
            path.push(format!("{subcat}.docs"));
        } else {
            path.push(subcat);
        }
    }
    path
}

/// List all files in a directory by their number prefix
///
/// Files are expected to have a number prefix followed by a dash (e.g., "01-foo.lex").
/// Returns a map of number -> filepath.
///
/// # Panics
/// Panics if duplicate numbers are found (critical error in test corpus)
pub fn list_files_by_number(dir: &PathBuf) -> Result<HashMap<usize, PathBuf>, SpecFileError> {
    let mut number_map: HashMap<usize, PathBuf> = HashMap::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process .lex files
        if !path.extension().map(|e| e == "lex").unwrap_or(false) {
            continue;
        }

        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            // Extract number from filename
            // Files can be either "NNN-hint.lex" or "prefix-NN-hint.lex"
            // Try all dash-separated parts until we find a number
            for part in filename.split('-') {
                if let Ok(num) = part.parse::<usize>() {
                    // Check for duplicates
                    if let Some(existing_path) = number_map.get(&num) {
                        panic!(
                            "DUPLICATE TEST NUMBERS DETECTED!\n\
                            Number {} found in multiple files:\n\
                            - {}\n\
                            - {}\n\n\
                            ERROR: Test numbers must be unique within each directory.\n\
                            FIX: Rename the duplicate files to use unique numbers.\n\
                            Directory: {}",
                            num,
                            existing_path.display(),
                            path.display(),
                            dir.display()
                        );
                    }
                    number_map.insert(num, path);
                    break; // Found the number, no need to check other parts
                }
            }
        }
    }

    Ok(number_map)
}

/// Find a spec file by category, subcategory, and number
///
/// This is the main orchestrator function.
/// Optimized to stop as soon as the file is found, without building the full map.
/// Note: This skips duplicate detection for performance. Duplicate detection should be handled by a separate test.
///
/// # Examples
/// ```ignore
/// // Find paragraph element test #1
/// find_specfile_by_number("elements", Some("paragraph"), 1)
///
/// // Find benchmark document #10
/// find_specfile_by_number("benchmark", None, 10)
/// ```
pub fn find_specfile_by_number(
    category: &str,
    subcategory: Option<&str>,
    number: usize,
) -> Result<PathBuf, SpecFileError> {
    let dir = get_doc_root(category, subcategory);

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process .lex files
        if !path.extension().map(|e| e == "lex").unwrap_or(false) {
            continue;
        }

        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            // Extract number from filename
            // Files can be either "NNN-hint.lex" or "prefix-NN-hint.lex"
            for part in filename.split('-') {
                if let Ok(num) = part.parse::<usize>() {
                    if num == number {
                        return Ok(path);
                    }
                    break; // Found a number but it didn't match, stop checking parts for this file
                }
            }
        }
    }

    let location = if let Some(subcat) = subcategory {
        format!("{category}/{subcat}")
    } else {
        category.to_string()
    };

    Err(SpecFileError::FileNotFound(format!(
        "No file with number {number} found in {location}"
    )))
}

/// List all available numbers for a given category/subcategory
///
/// Useful for test discovery and validation.
pub fn list_available_numbers(
    category: &str,
    subcategory: Option<&str>,
) -> Result<Vec<usize>, SpecFileError> {
    let dir = get_doc_root(category, subcategory);
    let number_map = list_files_by_number(&dir)?;

    let mut numbers: Vec<usize> = number_map.keys().copied().collect();
    numbers.sort_unstable();
    Ok(numbers)
}

// ============================================================================
// Convenience helpers for common use cases
// ============================================================================

/// Find a spec file for an element type and number
///
/// # Example
/// ```ignore
/// find_element_file(ElementType::Paragraph, 1)
/// ```
pub fn find_element_file(
    element_type: ElementType,
    number: usize,
) -> Result<PathBuf, SpecFileError> {
    find_specfile_by_number("elements", Some(element_type.dir_name()), number)
}

/// Find a spec file for a document type and number
///
/// # Example
/// ```ignore
/// find_document_file(DocumentType::Benchmark, 10)
/// ```
pub fn find_document_file(doc_type: DocumentType, number: usize) -> Result<PathBuf, SpecFileError> {
    find_specfile_by_number(doc_type.dir_name(), None, number)
}

/// List all available numbers for a given element type
///
/// # Example
/// ```ignore
/// list_element_numbers(ElementType::Paragraph)
/// ```
pub fn list_element_numbers(element_type: ElementType) -> Result<Vec<usize>, SpecFileError> {
    list_available_numbers("elements", Some(element_type.dir_name()))
}
