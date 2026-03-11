// This file is required to make `cargo test` discover tests in subdirectories.

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
