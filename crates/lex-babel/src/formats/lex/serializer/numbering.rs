//! List-marker index rendering for the Lex serializer.
//!
//! Turns a `(DecorationStyle, upper_case, index)` triple into the marker text
//! the formatter writes — decimal, alphabetical, or Roman. Used by
//! `LexSerializer`'s list-item emission to normalize sequence markers.

use lex_core::lex::ast::elements::sequence_marker::DecorationStyle;

/// Render a single list level's index according to its decoration style.
pub(super) fn format_marker_index(
    style: DecorationStyle,
    upper_case: bool,
    index: usize,
) -> String {
    match style {
        DecorationStyle::Plain => "-".to_string(),
        DecorationStyle::Numerical => index.to_string(),
        DecorationStyle::Alphabetical => {
            if upper_case {
                to_alpha_upper(index)
            } else {
                to_alpha_lower(index)
            }
        }
        DecorationStyle::Roman => to_roman_upper(index),
    }
}

fn to_alpha_lower(n: usize) -> String {
    if (1..=26).contains(&n) {
        char::from_u32((n as u32) + 96).unwrap().to_string()
    } else {
        n.to_string()
    }
}
fn to_alpha_upper(n: usize) -> String {
    if (1..=26).contains(&n) {
        char::from_u32((n as u32) + 64).unwrap().to_string()
    } else {
        n.to_string()
    }
}

fn to_roman_upper(n: usize) -> String {
    // Convert to Roman numerals (uppercase) for common values
    // Falls back to decimal for values > 20
    match n {
        1 => "I".to_string(),
        2 => "II".to_string(),
        3 => "III".to_string(),
        4 => "IV".to_string(),
        5 => "V".to_string(),
        6 => "VI".to_string(),
        7 => "VII".to_string(),
        8 => "VIII".to_string(),
        9 => "IX".to_string(),
        10 => "X".to_string(),
        11 => "XI".to_string(),
        12 => "XII".to_string(),
        13 => "XIII".to_string(),
        14 => "XIV".to_string(),
        15 => "XV".to_string(),
        16 => "XVI".to_string(),
        17 => "XVII".to_string(),
        18 => "XVIII".to_string(),
        19 => "XIX".to_string(),
        20 => "XX".to_string(),
        _ => n.to_string(), // Fallback to decimal for larger numbers
    }
}
