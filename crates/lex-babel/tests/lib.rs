// Single consolidated test binary for lex-babel. Each submodule used to be (or
// co-existed as) its own `tests/*.rs` binary; consolidating avoids redundant
// linking.

#[cfg(test)]
mod common;

// Shared Skeleton reducer (`canon`) + Faithfulness comparator, used by both
// `format_invariants` and the markdown conversion-faithfulness tests.
#[cfg(test)]
mod skeleton;

#[cfg(test)]
mod html;

#[cfg(test)]
mod markdown;

#[cfg(test)]
mod pdf;

#[cfg(test)]
mod rfc_xml;

#[cfg(test)]
mod round_trip_proptest;

#[cfg(test)]
mod ir_round_trip_proptest;

#[cfg(test)]
mod format_invariants;

#[cfg(test)]
mod lex_separation;
