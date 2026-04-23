// Single consolidated test binary for lex-babel. Each submodule used to be (or
// co-existed as) its own `tests/*.rs` binary; consolidating avoids redundant
// linking.

#[cfg(test)]
mod common;

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
