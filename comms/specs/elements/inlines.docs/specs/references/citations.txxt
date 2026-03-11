:: title :: Citation References Specification
:: author :: Arthur Debert
:: pub-date :: 2025-01-12

Complete specification for citation references - academic and bibliographic citations that integrate with external citation management systems in lex documents.

:: note :: This specification describes the specific characteristics of citation reference elements. For the general reference token pattern and architecture, see [../references-general.lex].

1. Purpose

    Citation references provide academic and bibliographic citation capabilities that integrate with external citation management tools. They use the `[@key]` pattern to reference entries in external bibliography files, supporting multiple citation formats, page numbers, and various academic citation styles. The system delegates complex citation formatting to external tools while maintaining simple, readable syntax.

2. Citation Types

    2.1. Author Citations

        Basic author references:
            [@john]                 # Single author
            [@john,smith]           # Multiple authors (comma-separated)
        :: author-citations

    2.2. Author with Page Citations

        Author references with specific page information:
            [@john, p.45]           # Single page
            [@john, p.45,46]        # Multiple pages (comma-separated)
            [@john, p.45-203]       # Page range
        :: author-page-citations

    2.3. Page-Only References

        Standalone page references (no author):
            [p.43]                  # Single page
            [p.43,44]               # Multiple pages
            [p.43-100]              # Page range
            [pp.43]                 # Alternative page format
            [pp 44]                 # Space instead of period
        :: page-references

        Page reference variations:
            [p43,p44]               # With p prefix per page
            [p.43,p.44]             # Mixed formats allowed
        :: page-variations

3. Grammar

    3.1. Citation Grammar

        The authoritative grammar for citations is defined in the main syntax reference.

        A citation span consists of one or more keys and an optional page locator.
            <citation-span> = <left-bracket> <at-sign> <citation-keys> <citation-locator>? <right-bracket>

        Citation keys are comma-separated identifiers:
            <citation-key> = <identifier>
            <citation-keys> = <citation-key> (, <citation-key>)*

        The page locator specifies page numbers or ranges:
            <page-number> = <digit>+
            <page-range> = <page-number> (- <page-number>)?
            <page-list> = <page-range> (, <page-range>)*
            <page-locator> = p <period>? <page-list> | pp <period>? <page-list>
            <citation-locator> = , <whitespace>* <page-locator>
        :: grammar

4. AST Structure

    Citation-specific AST representation:

    Author Citation AST:
        ├── Citation
        │   ├── citation_type: AuthorCitation
        │   ├── keys: Vec<String>
        │   ├── page_info: Option<PageInfo>
        │   └── tokens: TokenSequence
    :: author-ast

    Page Citation AST:
        ├── Citation
        │   ├── citation_type: PageCitation
        │   ├── page_info: PageInfo
        │   └── tokens: TokenSequence
    :: page-ast

    Page Info Structure:
        ├── PageInfo
        │   ├── format: PageFormat  # p or pp
        │   ├── ranges: Vec<PageRange>
        │   └── raw_text: String
    :: page-info

5. Processing Rules

    5.1. Recognition Priority

        Citation recognition in reference processing order:
        1. Author citations: Pattern starts with `[@`
        2. Page citations: Pattern starts with `[p` or `[pp`
        3. Session references: Pattern starts with `[#`
        4. General references: All other `[content]` patterns

    5.2. Author Citation Processing

        Author citation parsing:
        1. Extract content after `[@`
        2. Split on comma to separate authors from page info
        3. Parse author keys (comma-separated)
        4. Parse optional page locator after comma
        5. Validate citation keys match identifier pattern

    5.3. Page Citation Processing

        Page citation parsing:
        1. Extract content after `[p` or `[pp`
        2. Parse page format indicator
        3. Extract page numbers and ranges
        4. Handle various separator formats (comma, space, period)
        5. Validate numeric page references

6. Integration with Bibliography System

    6.1. External Bibliography Declaration

        Document-level bibliography annotation:
            :: bibliography :: references.bib
        :: bibliography-declaration

        Purpose: Links document to external BibTeX or similar bibliography file

    6.2. Citation Key Resolution

        Citation processing workflow:
        1. Parse citation references in document
        2. Collect all citation keys
        3. Look up keys in declared bibliography file
        4. Delegate formatting to external citation processor
        5. Generate final formatted citations and bibliography

    6.3. Bibliography Integration

        External tool responsibilities:
        - Load and parse bibliography file (BibTeX, etc.)
        - Resolve citation keys to bibliography entries
        - Apply citation style (APA, MLA, Chicago, etc.)
        - Generate in-text citations and bibliography
        - Handle missing or invalid citation keys



7. Edge Cases and Validation

    7.1. Invalid Citation Keys

        Malformed citation references:
            [@]                     # Empty citation key - invalid
            [@123invalid]           # Invalid identifier - treated as literal
            [@valid, invalid-page]  # Invalid page format - ignore page part
        :: invalid-citations

    7.2. Page Number Edge Cases

        Page reference boundary conditions:
            [p.]                    # No page number - invalid
            [p.123-]               # Incomplete range - treat as single page
            [p.45,]                # Trailing comma - ignore
            [pp 45 46]             # Space-separated - parse as range
        :: page-edge-cases

    7.3. Bibliography Resolution

        Unresolved citation handling:
        - Missing bibliography file → Preserve citations as text
        - Unknown citation key → Mark as unresolved, preserve original
        - Invalid page format → Use author-only citation
        - Tool responsibility to handle gracefully
8. Implementation Notes

    8.1. Parser Requirements

        Citation parsing considerations:
        - Distinguish citations from other bracket references
        - Handle multiple author keys correctly
        - Parse page locators with various formats
        - Preserve original content for error recovery

    8.2. External Tool Integration

        Bibliography system integration:
        - Standard BibTeX file support
        - CSL (Citation Style Language) compatibility
        - Integration with Zotero, Mendeley, etc.
        - Configurable citation styles

    8.3. Error Handling

        Robust citation processing:
        - Graceful handling of missing bibliography
        - Preserve unresolved citations as readable text
        - Clear error messages for debugging
        - Never break document processing

:: note :: Citation references provide powerful academic and bibliographic capabilities while maintaining lex's principle of delegating complex formatting to external tools. The simple bracket syntax ensures readability while supporting comprehensive citation workflows.