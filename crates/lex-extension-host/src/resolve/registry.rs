//! Scheme → [`Fetcher`] registry.
//!
//! Schemes are static strings owned by their [`Fetcher`] impl
//! ([`Fetcher::schemes`] returns `&'static [&'static str]`), so the
//! registry's keys are also `&'static str` — no allocation per
//! lookup.
//!
//! Typical usage: construct via [`default_fetcher_registry`] for the
//! standard two-transport stub set ([`HttpsFetcher`], [`GitFetcher`]),
//! or build a custom registry with [`FetcherRegistry::new`] +
//! [`FetcherRegistry::register`] when a host wants its own fetchers
//! (in-process mocks for tests, custom internal schemes, etc.).
//!
//! Forge-shorthand schemes (`github:`, `gitlab:`) are *not* in the
//! registry: they're URL templates that expand into a transport URI
//! upstream of registry dispatch (see [`super::template`]). The
//! registry only carries real transports.

use std::collections::HashMap;
use std::sync::Arc;

use super::fetcher::{Fetcher, GitFetcher, HttpsFetcher};

/// Maps URI schemes to [`Fetcher`] implementations. Clone is cheap
/// (one `Arc` clone per registered fetcher).
#[derive(Clone, Default)]
pub struct FetcherRegistry {
    fetchers: HashMap<&'static str, Arc<dyn Fetcher>>,
}

impl FetcherRegistry {
    /// Empty registry. Use [`Self::register`] to add fetchers, or
    /// [`default_fetcher_registry`] for the standard set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a fetcher to the registry, claiming every scheme in its
    /// [`Fetcher::schemes`] list. A later registration for the same
    /// scheme overwrites the earlier entry — fine for tests
    /// overriding a default fetcher; the host doesn't currently rely
    /// on the no-overwrite invariant.
    pub fn register(&mut self, fetcher: Arc<dyn Fetcher>) {
        for scheme in fetcher.schemes() {
            self.fetchers.insert(*scheme, Arc::clone(&fetcher));
        }
    }

    /// Look up a fetcher by URI scheme. Returns `None` when no
    /// fetcher claims this scheme; the caller surfaces that as a
    /// [`super::ResolveError::UnknownScheme`].
    pub fn get(&self, scheme: &str) -> Option<Arc<dyn Fetcher>> {
        self.fetchers.get(scheme).map(Arc::clone)
    }

    /// True if any fetcher in this registry claims `scheme`.
    pub fn contains(&self, scheme: &str) -> bool {
        self.fetchers.contains_key(scheme)
    }
}

impl std::fmt::Debug for FetcherRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetcherRegistry")
            .field(
                "schemes",
                &self.fetchers.keys().copied().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Construct a registry with the standard transport-fetcher stub set:
/// two [`Fetcher`] implementations covering three URI schemes —
/// [`HttpsFetcher`] (claims `https:`) and [`GitFetcher`] (claims both
/// `git:` and `git+ssh:`, since both URL forms feed the same
/// `git clone`). Each fetcher returns [`super::FetchError::Unimplemented`]
/// from `fetch` — replace with a real implementation per lex#562 to
/// make the transport actually work.
///
/// `path:` is NOT in the registry: it's special-cased at the
/// [`super::resolve_namespace_with`] level (no cache, no fetcher,
/// resolved directly against the workspace root).
///
/// `github:` and `gitlab:` are NOT in the registry either: they're
/// URL templates that expand to transport URIs upstream of dispatch
/// (see [`super::template`]). Adding more forge shorthands
/// (bitbucket, gitea, codeberg, sourcehut) is template work, not
/// fetcher work — the registry stays at two entries.
pub fn default_fetcher_registry() -> FetcherRegistry {
    let mut r = FetcherRegistry::new();
    r.register(Arc::new(HttpsFetcher));
    r.register(Arc::new(GitFetcher));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_transport_schemes_only() {
        let r = default_fetcher_registry();
        // Real transports.
        for s in ["https", "git", "git+ssh"] {
            assert!(r.contains(s), "default registry missing `{s}:`");
        }
        // `path:` is intentionally NOT in the registry — special-cased
        // upstream of dispatch.
        assert!(!r.contains("path"));
        // `github:` and `gitlab:` are URL templates, not transports —
        // they expand into one of the registered transports before
        // dispatch reaches the registry.
        assert!(!r.contains("github"));
        assert!(!r.contains("gitlab"));
    }

    #[test]
    fn register_then_get() {
        struct Custom;
        impl Fetcher for Custom {
            fn fetch(
                &self,
                _uri: &super::super::uri::ParsedUri,
                _dest: &std::path::Path,
            ) -> Result<(), super::super::FetchError> {
                unreachable!("test fetcher: fetch shouldn't be called")
            }
            fn schemes(&self) -> &'static [&'static str] {
                &["custom"]
            }
        }
        let mut r = FetcherRegistry::new();
        r.register(Arc::new(Custom));
        assert!(r.contains("custom"));
        let _f = r.get("custom").expect("custom fetcher present");
    }

    #[test]
    fn register_overwrites_on_scheme_collision() {
        struct A;
        impl Fetcher for A {
            fn fetch(
                &self,
                _uri: &super::super::uri::ParsedUri,
                _dest: &std::path::Path,
            ) -> Result<(), super::super::FetchError> {
                Err(super::super::FetchError::Other {
                    message: "A".into(),
                })
            }
            fn schemes(&self) -> &'static [&'static str] {
                &["shared"]
            }
        }
        struct B;
        impl Fetcher for B {
            fn fetch(
                &self,
                _uri: &super::super::uri::ParsedUri,
                _dest: &std::path::Path,
            ) -> Result<(), super::super::FetchError> {
                Err(super::super::FetchError::Other {
                    message: "B".into(),
                })
            }
            fn schemes(&self) -> &'static [&'static str] {
                &["shared"]
            }
        }
        let mut r = FetcherRegistry::new();
        r.register(Arc::new(A));
        r.register(Arc::new(B));
        let f = r.get("shared").unwrap();
        let dummy = super::super::uri::ParsedUri::parse("shared:x").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let err = f.fetch(&dummy, tmp.path()).unwrap_err();
        match err {
            super::super::FetchError::Other { message } => assert_eq!(message, "B"),
            other => panic!("expected Other(B), got: {other}"),
        }
    }
}
