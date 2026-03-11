Lex-Babel Format Implementation Guide

This document provides the standard workflow for implementing format interoperability in lex-babel.
It covers project setup, implementation approach, testing strategy, and best practices.

For brevity, we list spec files from the root spec directory (specs/v1).

Read for reference:
    lex-babel/src/format.rs           # Format trait definition
    lex-babel/src/lib.rs              # Architecture overview
    lex-babel/src/ir/                 # Intermediate Representation
    lex-babel/src/common/           # Nested ↔ Flat conversion algorithms

The rationale is to start with export (Lex → Format), since we start with a well-understood AST
(lex-parser's), making the first task easier. Import (Format → Lex) comes second, leveraging
the export implementation for testing.

0. Setup and Dependencies

  0.1. Add format-specific dependencies to Cargo.toml:

    Example for Markdown:
      [dependencies]
      lex-parser = { path = "../lex-parser" }
      comrak = "0.29"  # Markdown parser/serializer

    For other formats, choose appropriate well-maintained crates:
      - HTML: Use a robust HTML parser like `html5ever` or `scraper`
      - Pandoc: Use `pandoc_ast` or shell out to pandoc binary
      - LaTeX: Consider `tex-parser` or similar

  0.2. File structure to create:

    src/formats/<format>/
      ├── mod.rs           # Module documentation, Format trait impl, element mapping table
      ├── serializer.rs    # Lex AST → Format (export)
      ├── parser.rs        # Format → Lex AST (import)
      └── mapping.rs       # Shared conversion utilities (optional, add if needed)

  0.3. Register format in src/formats/mod.rs:

    pub mod <format>;
    pub use <format>::<Format>Format;

  0.4. Register in src/registry.rs (if not auto-discovered)

  0.5. Add test module:

    tests/
      ├── lib.rs           # Add `mod <format>;`
      └── <format>/
          ├── mod.rs       # Test suite entry point
          ├── export.rs    # Export tests (Lex → Format)
          ├── import.rs    # Import tests (Format → Lex)
          └── fixtures/    # Test files (.lex and .<ext> pairs)

  0.6. Element Mapping Table (maintain in src/formats/<format>/mod.rs as doc comment):

    Create a complete mapping table with these columns:

    | Lex Element | Format Equivalent | Export Notes | Import Notes |

    Include all IR node types:
    - Document, Heading, Paragraph, List, ListItem, Definition, Verbatim, Annotation
    - InlineContent: Text, Bold, Italic, Code, Math, Reference

    Document lossy conversions explicitly.
    See src/formats/markdown/mod.rs for a complete example.

  0.6.1. Special Case: Links and Anchors

    Lex does not have link anchors (clickable text). It only has references like [url].
    When converting to/from formats that DO support links with anchors (HTML, Markdown, etc.),
    follow this standard behavior:

    EXPORT (Lex → Format with links):
      - The word BEFORE the reference becomes the link anchor
      - If the reference is the first word in an element, use the word AFTER it
      - The reference itself becomes the link href/target

      Example:
        Lex:      "welcome to the bahamas [bahamas.gov]"
        Markdown: "welcome to the [bahamas](bahamas.gov)"
        HTML:     "welcome to the <a href=\"bahamas.gov\">bahamas</a>"

      Example (reference at start):
        Lex:      "[wikipedia.org] Wikipedia is useful"
        Markdown: "[Wikipedia](wikipedia.org) is useful"

    IMPORT (Format with links → Lex):
      - The anchor text becomes plain text in the content
      - The reference follows it in Lex format

      Example:
        HTML:     "<a href=\"bahamas.gov\">bahamas</a>"
        Lex:      "bahamas [bahamas.gov]"

    IMPLEMENTATION:
      - Generic helpers are provided in src/common/links.rs
      - Use extract_anchor_for_reference() during export
      - Use insert_reference_with_anchor() during import
      - These work at the IR InlineContent level, so they're format-agnostic

  0.7. Reference Documents for Testing:

    Identify a canonical reference document for the format:
    - Markdown: CommonMark specification examples
    - HTML: HTML5 test suite snippets
    - LaTeX: Common LaTeX document patterns

    Create: tests/fixtures/<format>-reference.<ext> with representative examples
    - Markdown currently snapshots:
        * tests/fixtures/markdown-reference-commonmark.md (CommonMark spec README)
        * tests/fixtures/markdown-reference-comrak.md (Comrak README)
      These are imported in lex-babel/tests/markdown/import.rs and asserted via insta snapshots.
      Verify round-trips from the CLI with:
        cargo run --bin lex -- lex-babel/tests/fixtures/markdown-reference-commonmark.md --to tag

1. What to test: as a general rule we want to have:

  Getting the right source files for testing and iterating is key. 


  1.1. Isolated unitests , one per element (in some cases 2 or 3 might be useful). These serve as a simplified way to get to know the other ast and verify correctness.
  1.2 Test the trickiest bit: the document structure, being Lex hierarchical while most formats are flat. For this, we will use the trifecta documents (./docs/spec/v1/trifecta), starting rom the simplest case, then an intermediate (files 010, 020, and 060). 
  1.3 Ensambles: documents that mix many elemenents. We always test:
    1.3.1 Kitchensink: specs/v1/benchmark/010-kitchensink.lex botn as import and export since it's a reasonable concide document that covers all Lex format featus.
    1.3.3 Other format reference document: most format's tooling / library we will use have a reference file  , we must find such a file and use it for each format.

  That is: 1-3 files per elment (according to complexity), 3 trifecta structure files, the full kitchensink document and a reference file for that format itself.


2. Implementation Approach

  General pipeline for all formats:

    Export: Lex AST → IR → Events → Format AST → String
    Import: String → Format AST → Events → IR → Lex AST

  2.1. Export (Lex → Format) Implementation:

    Step 1: Convert Lex AST to IR
      - Use existing `lex_babel::to_ir(doc)` function
      - This gives you IR::Document with IR nodes
      - IMPORTANT: Annotations are attached as metadata to AST nodes, not stored in the content tree
      - You must extract annotations from each element using element.annotations()
      - See lex-parser/src/lex/assembling/stages/attach_annotations.rs for how they're attached
      - See lex-babel/src/ir/from_lex.rs:extract_attached_annotations() for extraction example

    Step 2: Convert IR to flat event stream
      - Use `common::nested_to_flat::tree_to_events(&ir_doc_node)`
      - This flattens the hierarchical structure into start/end events

    Step 3: Convert events to Format AST (format-specific library)
      - Walk events and build format-specific nodes
      - Handle format constraints (e.g., heading depth limits)
      - You will need state machines to manage conversion (see section 2.3 below)

    Step 4: Serialize Format AST to string
      - Use format library's serializer
      - Configure output options as needed
      - Post-process to clean up library artifacts (e.g., unwanted HTML comments)
      - Some libraries inject elements you may not want in the final output

    Example skeleton for serializer.rs:

      use crate::ir::events::Event;
      use crate::error::FormatError;

      pub fn serialize_to_<format>(doc: &lex_parser::lex::ast::Document) -> Result<String, FormatError> {
          // Step 1: Lex AST → IR
          let ir_doc = crate::to_ir(doc);

          // Step 2: IR → Events
          let events = crate::common::nested_to_flat::tree_to_events(&ir_doc);

          // Step 3: Events → Format AST (format-specific)
          let format_ast = events_to_<format>_ast(&events)?;

          // Step 4: Format AST → String (using format library)
          let output = <format_library>::serialize(format_ast)?;
          Ok(output)
      }

  2.2. Import (Format → Lex) Implementation:

    Step 1: Parse format string to Format AST
      - Use format library's parser
      - Handle parse errors gracefully

    Step 2: Convert Format AST to IR events
      - Walk format AST nodes recursively
      - Emit Event::Start*, Event::Inline, Event::End* appropriately
      - Maintain proper nesting with start/end pairs

    Step 3: Convert events to IR tree
      - Use `common::flat_to_nested::events_to_tree(&events)`
      - This reconstructs the hierarchical structure
      - Validates proper nesting

    Step 4: Convert IR to Lex AST
      - Use existing `ir::to_lex::to_lex_document(&ir_doc)` (implement if missing)
      - Map IR nodes to Lex AST elements

    For format-specific implementation examples, see:
      - src/formats/markdown/mod.rs (once implemented)
      - src/formats/tag/mod.rs (simpler example, export only)

  2.3. Common State Machines for Event ↔ Format AST Conversion:

    When converting between event streams and format-specific ASTs, you will need
    several state machines to manage the conversion process. These patterns recur
    across all format implementations:

    2.3.1. Parent Stack (for building trees from events)
      - Maintains the current nesting level during AST construction
      - Push containers onto stack when entering (StartHeading, StartList, etc.)
      - Pop when exiting (EndHeading, EndList, etc.)
      - Current parent is always stack.last_mut()
      - Example: src/formats/markdown/serializer.rs:66-68

    2.3.2. Heading Hierarchy (handled automatically by flat_to_nested)
      - The generic flat_to_nested converter automatically closes parent headings
      - Format parsers just emit StartHeading(level) - no EndHeading needed
      - When flat_to_nested sees a new heading, it auto-closes any at same or deeper level
      - At document end, it auto-closes all remaining open headings
      - This is built into src/common/flat_to_nested.rs - all formats get it for free
      - Example: src/formats/markdown/parser.rs just emits StartHeading

    2.3.3. Content Accumulation (for multi-event elements)
      - Collect content across multiple inline events before finalizing
      - Common for verbatim blocks (accumulate lines) and definitions
      - Use temporary buffers: in_verbatim flag + verbatim_content string
      - Example: src/formats/markdown/serializer.rs:70-72, 199-221

    2.3.4. Multi-Element Pattern Detection (for complex common)
      - Some Lex elements map to patterns across multiple format elements
      - Example: Lex Definition → Markdown "**Term**: description" + siblings
      - Requires lookahead/peekable iterators to consume sibling nodes
      - Track boundaries (headings, other definitions) to know when to stop
      - Example: src/formats/markdown/parser.rs:360-413

    2.3.5. Context-Sensitive Rendering (handling format quirks)
      - List items may need auto-wrapped paragraphs (Markdown tight lists)
      - Headings can only contain inline content, not blocks
      - Use flags: current_heading, in_list_item, list_item_paragraph
      - Example: src/formats/markdown/serializer.rs:77-82, 114-130, 244-260

    These state machines interact and must be carefully coordinated. When in doubt,
    study the markdown implementation as a reference for managing these patterns.

3. How To Test, for each direction (import and export)

  The backbone of testing will be comprised of unit tests testing Lex ↔ Format AST conversions.
  The base format for tests follows, and is to be followed by isolated document tests and the trifecta tests.

    3.1. Unit test pattern (AST level):

      use lex_parser::lex::transforms::standard::STRING_TO_AST;

      #[test]
      fn test_paragraph_export() {
          // Load Lex source from spec file
          let lex_src = std::fs::read_to_string("../../specs/v1/elements/paragraph.docs/paragraph-01-flat-oneline.lex").unwrap();
          let lex_doc = STRING_TO_AST.run(lex_src).unwrap();

          // Optionally test IR conversion
          let ir = lex_babel::to_ir(&lex_doc);
          let events = tree_to_events(&DocNode::Document(ir));

          // Assert on event structure
          assert!(matches!(events[1], Event::StartParagraph));
          assert!(matches!(events[2], Event::Inline(_)));

          // Full string conversion
          let output = <Format>Format.serialize(&lex_doc).unwrap();
          assert!(output.contains("expected content"));
      }

    3.2. Integration test pattern (string level):

      #[test]
      fn test_kitchensink_roundtrip() {
          let lex_src = std::fs::read_to_string("tests/fixtures/kitchensink.lex").unwrap();
          let lex_doc = LexFormat.parse(&lex_src).unwrap();

          // Export to format
          let format_output = <Format>Format.serialize(&lex_doc).unwrap();

          // Import back to Lex
          let lex_doc2 = <Format>Format.parse(&format_output).unwrap();

          // Compare (may be lossy, so use snapshot testing)
          // Use `insta` crate for snapshot testing
          insta::assert_snapshot!(format_output);
      }

  Note: Use lex-parser's STRING_TO_AST.run() or similar parsing utilities to load
  isolated elements from spec files for testing.

    3.3. Snapshot Testing Best Practices:

      Snapshot tests are superior to assertion-based tests for format conversion because
      they capture the exact output and make regressions immediately visible.

      Setup:
        $ cargo install cargo-insta

      Workflow:
        1. Write test using insta::assert_snapshot!(output)
        2. Run test - it will fail on first run (no snapshot exists)
        3. Review generated snapshot in snapshots/ directory
        4. If correct: cargo insta accept
        5. If incorrect: fix code and rerun

      Best practices:
        - Review snapshot diffs carefully before accepting changes
        - Document lossy conversions in snapshots (e.g., annotations not reimported)
        - Use snapshot names that match test function names for clarity
        - Commit snapshots to git alongside code
        - Snapshots catch regressions better than hand-written assertions
        - Use insta::assert_snapshot!(output, @"expected") for inline snapshots

      Example from markdown implementation:
        - See lex-babel/tests/markdown/export.rs for snapshot test usage
        - See lex-babel/tests/markdown/snapshots/ for snapshot files


4. Testing Order

  1. Always start with Lex export to format (easier since you control the input AST).

  2. Do the isolated elements, one by one, testing and committing every element that is working.

     General suggested order (adapt to format):
     - Paragraph (simplest, no nesting)
     - Heading (maps to Session)
     - Bold, Italic, Code inlines
     - Lists (introduces nesting)
     - Code blocks / Verbatim
     - Definitions (may need creative mapping)
     - Annotations (often lossy conversion)

  3. Do the trifecta documents, one by one. It's key to get each right in order (start from smaller file numbers).
     Trying to get the high number files working first won't work, you need to walk up the complexity ladder.

     Trifecta test files (in /specs/v1/trifecta/):
     - 010-paragraphs-sessions-flat-single.lex (simplest)
     - 020-paragraphs-sessions-flat-multiple.lex
     - 060-trifecta-nesting.lex (most complex nesting)

  4. Do the benchmark files, again, one by one, starting with the kitchensink.

     Benchmark files:
     - specs/v1/benchmark/010-kitchensink.lex (export)
     - tests/fixtures/<format>-reference.<ext> (import)

  5. Additional cases that are relevant for that format, be it in isolated element form or full ensembles.

  Once you start the work on the import, by using the same documents, you can leverage the export
  tests: you are now sure of one AST form (Lex from export tests), and the other format's AST
  (the library for the format is understood to be correct), which makes testing conversion much easier.

5. Remember:

  1. While format specific transformations exist, there is an entire class of mapping challenges that
     will be true across formats. The classic and most complex one is the mapping from nested documents
     (Lex) to flat documents (Markdown, HTML, Pandoc, LaTeX). This is why we have
     lex-babel/src/common/mod.rs, that is base source code that handles these. If you find new
     common that affect multiple formats, add to the common library for reuse.

  2. Keep as cargo document, in the formats module file (i.e. lex-babel/src/formats/<format>/mod.rs)
     a table of ported elements on import and export columns.

  3. As soon as feasible add your format to lex-cli so you can quickly test the work on the command line.
     You can add both text output (<format>) and AST output (<format>-ast) to make intermediate steps easier.

6. Tips:

  Useful commands during development:

    $ cargo run --bin lex inspect <path> ast-treeviz  # Visualize Lex AST
    $ cargo run --bin lex convert <path> to <path>    # Test conversion
    $ cargo test -- --nocapture                       # Run tests with output
    $ cargo test <format>                             # Run format-specific tests
