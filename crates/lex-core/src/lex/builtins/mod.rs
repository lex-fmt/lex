//! Built-in `LexHandler` implementations for the `lex.*` namespace.
//!
//! Built-ins flow through the same `lex_extension::LexHandler` trait and
//! `lex_extension_host::Registry` dispatch fabric as third-party namespaces.
//! Their only privilege is being compiled-in: at host startup, the CLI
//! and LSP call this module's `register_into(&Registry, ...)` helper to
//! attach the bundled `lex.*` schemas and handlers.
//!
//! # Status: skeleton only
//!
//! This module is the placeholder landed in PR 3a (lex-fmt/lex#519). The
//! first concrete built-in, `LexIncludeHandler`, lands in PR 3c
//! (lex-fmt/lex#532) once the wire codec from PR 3b
//! (lex-fmt/lex#531) exists. The resolve pass wires through this
//! module's `register_into` helper in PR 3d (lex-fmt/lex#533).
//!
//! Future built-ins (`lex.toc`, …) follow the same shape: one impl per
//! label, registered through this module's helper at host startup.
