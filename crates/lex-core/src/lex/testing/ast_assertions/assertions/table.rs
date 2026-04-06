//! Table assertions

use crate::lex::ast::elements::verbatim::VerbatimBlockMode;
use crate::lex::ast::{Table, TableCellAlignment};

pub struct TableAssertion<'a> {
    pub(crate) table: &'a Table,
    pub(crate) context: String,
}

impl<'a> TableAssertion<'a> {
    pub fn subject(self, expected: &str) -> Self {
        let actual = self.table.subject.as_string();
        assert_eq!(
            actual, expected,
            "{}: Expected table subject '{}', got '{}'",
            self.context, expected, actual
        );
        self
    }

    pub fn mode(self, expected: VerbatimBlockMode) -> Self {
        assert_eq!(
            self.table.mode, expected,
            "{}: Expected table mode {:?}, got {:?}",
            self.context, expected, self.table.mode
        );
        self
    }

    /// Assert that the table has an annotation with the given label
    pub fn has_annotation_with_label(self, expected: &str) -> Self {
        let found = self
            .table
            .annotations
            .iter()
            .any(|a| a.data.label.value == expected);
        assert!(
            found,
            "{}: Expected annotation with label '{}'",
            self.context, expected
        );
        self
    }

    pub fn header_row_count(self, expected: usize) -> Self {
        let actual = self.table.header_rows.len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} header rows, got {}",
            self.context, expected, actual
        );
        self
    }

    pub fn body_row_count(self, expected: usize) -> Self {
        let actual = self.table.body_rows.len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} body rows, got {}",
            self.context, expected, actual
        );
        self
    }

    pub fn row_count(self, expected: usize) -> Self {
        let actual = self.table.row_count();
        assert_eq!(
            actual, expected,
            "{}: Expected {} total rows, got {}",
            self.context, expected, actual
        );
        self
    }

    pub fn column_count(self, expected: usize) -> Self {
        let actual = self.table.column_count();
        assert_eq!(
            actual, expected,
            "{}: Expected {} columns, got {}",
            self.context, expected, actual
        );
        self
    }

    /// Assert a specific header row's cell contents
    pub fn header_row(self, row_index: usize, assertion: impl FnOnce(RowAssertion<'a>)) -> Self {
        assert!(
            row_index < self.table.header_rows.len(),
            "{}: Header row index {} out of bounds ({} header rows)",
            self.context,
            row_index,
            self.table.header_rows.len()
        );
        assertion(RowAssertion {
            row: &self.table.header_rows[row_index],
            context: format!("{}::header[{}]", self.context, row_index),
        });
        self
    }

    /// Assert a specific body row's cell contents
    pub fn body_row(self, row_index: usize, assertion: impl FnOnce(RowAssertion<'a>)) -> Self {
        assert!(
            row_index < self.table.body_rows.len(),
            "{}: Body row index {} out of bounds ({} body rows)",
            self.context,
            row_index,
            self.table.body_rows.len()
        );
        assertion(RowAssertion {
            row: &self.table.body_rows[row_index],
            context: format!("{}::body[{}]", self.context, row_index),
        });
        self
    }

    /// Assert all header cell texts for a header row
    pub fn header_cells(self, row_index: usize, expected: &[&str]) -> Self {
        assert!(
            row_index < self.table.header_rows.len(),
            "{}: Header row index {} out of bounds",
            self.context,
            row_index
        );
        let row = &self.table.header_rows[row_index];
        let actual: Vec<&str> = row.cells.iter().map(|c| c.content.as_string()).collect();
        assert_eq!(
            actual, expected,
            "{}: Header row {} cells mismatch",
            self.context, row_index
        );
        self
    }

    /// Assert all body cell texts for a body row
    pub fn body_cells(self, row_index: usize, expected: &[&str]) -> Self {
        assert!(
            row_index < self.table.body_rows.len(),
            "{}: Body row index {} out of bounds",
            self.context,
            row_index
        );
        let row = &self.table.body_rows[row_index];
        let actual: Vec<&str> = row.cells.iter().map(|c| c.content.as_string()).collect();
        assert_eq!(
            actual, expected,
            "{}: Body row {} cells mismatch",
            self.context, row_index
        );
        self
    }

    /// Assert that one of the table's annotations has a parameter with the given key/value
    pub fn has_annotation_parameter_with_value(self, key: &str, value: &str) -> Self {
        let found = self.table.annotations.iter().any(|a| {
            a.data
                .parameters
                .iter()
                .any(|p| p.key == key && p.value == value)
        });
        assert!(
            found,
            "{}: Expected annotation parameter '{}={}'",
            self.context, key, value
        );
        self
    }

    pub fn has_footnotes(self) -> Self {
        assert!(
            self.table.footnotes.is_some(),
            "{}: Expected table to have footnotes",
            self.context
        );
        self
    }

    pub fn no_footnotes(self) -> Self {
        assert!(
            self.table.footnotes.is_none(),
            "{}: Expected table to have no footnotes",
            self.context
        );
        self
    }

    pub fn footnote_count(self, expected: usize) -> Self {
        let list = self
            .table
            .footnotes
            .as_ref()
            .unwrap_or_else(|| panic!("{}: Expected table to have footnotes", self.context));
        let items: Vec<_> = list.items.iter().collect();
        assert_eq!(
            items.len(),
            expected,
            "{}: Expected {} footnotes, got {}",
            self.context,
            expected,
            items.len()
        );
        self
    }

    pub fn footnote_text(self, index: usize, expected: &str) -> Self {
        let list = self
            .table
            .footnotes
            .as_ref()
            .unwrap_or_else(|| panic!("{}: Expected table to have footnotes", self.context));
        let items: Vec<_> = list.items.iter().collect();
        assert!(
            index < items.len(),
            "{}: Footnote index {} out of bounds ({} footnotes)",
            self.context,
            index,
            items.len()
        );
        let item = items[index]
            .as_list_item()
            .expect("Footnote should be a ListItem");
        let actual: String = item
            .text
            .iter()
            .map(|t| t.as_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            actual, expected,
            "{}: Footnote {} text mismatch",
            self.context, index
        );
        self
    }

    pub fn annotation_count(self, expected: usize) -> Self {
        let actual = self.table.annotations.len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} annotations, got {}",
            self.context, expected, actual
        );
        self
    }

    /// Assert that a cell has (or doesn't have) block content
    pub fn cell_has_block_content(
        self,
        row_type: &str,
        row_idx: usize,
        col: usize,
        expected: bool,
    ) -> Self {
        let rows = match row_type {
            "header" => &self.table.header_rows,
            "body" => &self.table.body_rows,
            _ => panic!("row_type must be 'header' or 'body'"),
        };
        assert!(
            row_idx < rows.len(),
            "{}: {} row {} out of bounds",
            self.context,
            row_type,
            row_idx
        );
        assert!(
            col < rows[row_idx].cells.len(),
            "{}: Cell {} out of bounds in {} row {}",
            self.context,
            col,
            row_type,
            row_idx
        );
        let actual = rows[row_idx].cells[col].has_block_content();
        assert_eq!(
            actual, expected,
            "{}: Expected {}[{}] cell {} has_block_content={}, got {}",
            self.context, row_type, row_idx, col, expected, actual
        );
        self
    }

    /// Assert specific child element within a cell using a callback
    pub fn cell_child(
        self,
        row_type: &str,
        row_idx: usize,
        col: usize,
        child_idx: usize,
        assertion: impl FnOnce(&crate::lex::ast::ContentItem),
    ) -> Self {
        let rows = match row_type {
            "header" => &self.table.header_rows,
            "body" => &self.table.body_rows,
            _ => panic!("row_type must be 'header' or 'body'"),
        };
        let cell = &rows[row_idx].cells[col];
        let children: Vec<&crate::lex::ast::ContentItem> = cell.children.iter().collect();
        assert!(
            child_idx < children.len(),
            "{}: Child index {} out of bounds ({} children in {}[{}] cell {})",
            self.context,
            child_idx,
            children.len(),
            row_type,
            row_idx,
            col
        );
        assertion(children[child_idx]);
        self
    }

    /// Count block children in a cell
    pub fn cell_child_count(
        self,
        row_type: &str,
        row_idx: usize,
        col: usize,
        expected: usize,
    ) -> Self {
        let rows = match row_type {
            "header" => &self.table.header_rows,
            "body" => &self.table.body_rows,
            _ => panic!("row_type must be 'header' or 'body'"),
        };
        let cell = &rows[row_idx].cells[col];
        let actual = cell.children.len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} children in {}[{}] cell {}, got {}",
            self.context, expected, row_type, row_idx, col, actual
        );
        self
    }
}

pub struct RowAssertion<'a> {
    row: &'a crate::lex::ast::TableRow,
    context: String,
}

impl<'a> RowAssertion<'a> {
    pub fn cell_count(self, expected: usize) -> Self {
        let actual = self.row.cells.len();
        assert_eq!(
            actual, expected,
            "{}: Expected {} cells, got {}",
            self.context, expected, actual
        );
        self
    }

    pub fn cell_text(self, col: usize, expected: &str) -> Self {
        assert!(
            col < self.row.cells.len(),
            "{}: Cell index {} out of bounds ({} cells)",
            self.context,
            col,
            self.row.cells.len()
        );
        let actual = self.row.cells[col].content.as_string();
        assert_eq!(
            actual, expected,
            "{}: Cell {} text mismatch",
            self.context, col
        );
        self
    }

    pub fn cell_colspan(self, col: usize, expected: usize) -> Self {
        assert!(col < self.row.cells.len());
        assert_eq!(
            self.row.cells[col].colspan, expected,
            "{}: Cell {} colspan mismatch",
            self.context, col
        );
        self
    }

    pub fn cell_rowspan(self, col: usize, expected: usize) -> Self {
        assert!(col < self.row.cells.len());
        assert_eq!(
            self.row.cells[col].rowspan, expected,
            "{}: Cell {} rowspan mismatch",
            self.context, col
        );
        self
    }

    pub fn cell_align(self, col: usize, expected: TableCellAlignment) -> Self {
        assert!(col < self.row.cells.len());
        assert_eq!(
            self.row.cells[col].align, expected,
            "{}: Cell {} alignment mismatch",
            self.context, col
        );
        self
    }

    pub fn cell_is_header(self, col: usize, expected: bool) -> Self {
        assert!(col < self.row.cells.len());
        assert_eq!(
            self.row.cells[col].header, expected,
            "{}: Cell {} header flag mismatch",
            self.context, col
        );
        self
    }

    pub fn cell_has_block_content(self, col: usize, expected: bool) -> Self {
        assert!(col < self.row.cells.len());
        let actual = self.row.cells[col].has_block_content();
        assert_eq!(
            actual, expected,
            "{}: Cell {} has_block_content mismatch",
            self.context, col
        );
        self
    }

    pub fn cell_child_count(self, col: usize, expected: usize) -> Self {
        assert!(col < self.row.cells.len());
        let actual = self.row.cells[col].children.len();
        assert_eq!(
            actual, expected,
            "{}: Cell {} child count mismatch",
            self.context, col
        );
        self
    }
}
