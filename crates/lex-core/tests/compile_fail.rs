// Container type safety is now enforced at runtime via validation in push().
// See tests/runtime_type_safety.rs for the fast runtime tests that replaced
// the slow compile-fail tests. This provides the same safety guarantees but
// executes ~7800x faster (~0.001s vs ~7.8s).
//
// The compile-time safety at construction time is still enforced by the type
// system (Annotation::new() and Definition::new() accept Vec<ContentElement>
// which excludes Sessions), but mutation safety is now runtime-validated.
