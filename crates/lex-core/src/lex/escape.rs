//! Centralized escape/unescape logic for Lex content
//!
//! Inline Escaping Rules:
//!   - Backslash before non-alphanumeric: escapes the character (backslash removed)
//!   - Backslash before alphanumeric: backslash preserved (for paths like C:\Users)
//!   - Double backslash (\\): produces a single backslash
//!   - Trailing backslash at end of input: preserved
//!
//! Quoted Parameter Value Escaping Rules:
//!   - `\"` inside a quoted value: literal quote (backslash removed)
//!   - `\\` inside a quoted value: literal backslash
//!   - Only `"` and `\` can be escaped; other backslashes are literal
//!
//! Structural Scanner Rules (for split/find on structural delimiters like `|`, `,`, `;`):
//!   - `\<sep>` is treated as a literal character (not a split point);
//!     the escaping backslash is stripped in the returned segment text.
//!   - `\\<sep>` counts as an escaped backslash followed by a structural `<sep>`
//!     (even number of backslashes → `<sep>` is structural).
//!   - Optionally, content inside balanced `literal_delim` pairs (e.g. backticks)
//!     is passed through verbatim: no split, no backslash stripping.
//!
//! Verbatim blocks and labels have no character-level escaping.
//!
//! # Module layout
//!
//! The logic is split across three cohesive submodules, re-exported here so the
//! `lex_core::lex::escape::*` paths are unchanged:
//!
//! - [`inline`] — character-level inline escaping (`escape_inline`,
//!   `unescape_inline`, …).
//! - [`structural`] — structural `LexMarker` detection and quoted-parameter-value
//!   escaping (`find_structural_lex_markers`, `is_quote_escaped`, …).
//! - [`split`] — escape-aware structural splitting and finding
//!   (`split_respecting_escape`, `find_respecting_escape`, …).

mod inline;
mod split;
mod structural;

#[cfg(test)]
mod tests;

pub use inline::{escape_inline, unescape_inline, unescape_inline_char, EscapeAction};
pub use split::{
    find_respecting_escape, find_respecting_escape_and_literals, split_respecting_escape,
    split_respecting_escape_and_literals, split_respecting_escape_with_ranges,
};
pub use structural::{
    escape_quoted, find_structural_lex_marker_pairs, find_structural_lex_markers, is_quote_escaped,
    is_structural_at, unescape_quoted,
};
