//! Content-keyed cache for resolved namespaces.
//!
//! Each `(canonical_uri, rev)` pair maps to a stable directory under
//! the cache root. First resolve fetches into the directory; later
//! resolves with the same key either reuse it (immutable rev or
//! within TTL for mutable refs) or re-fetch (mutable rev past TTL).
//!
//! Layout:
//!
//! ```text
//! <root>/                                      ← $XDG_CACHE_HOME/lex/labels
//!   <hash>/                                    ← sha256 of "<uri>#<rev>" (lowercased hex)
//!     .lex-fetched-at                          ← unix ts of last fetch
//!     <schema files>                           ← whatever the fetcher wrote
//! ```
//!
//! Key derivation uses SHA-256 of the canonical URI + rev so the same
//! lex.toml resolves to the same cache directory across machines —
//! the proposal's reproducibility property (§4.4) holds as long as
//! upstream hasn't moved a tag.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use super::fetcher::Fetcher;
use super::uri::ParsedUri;
use super::ResolveError;

/// Default TTL for mutable refs (branches, missing `rev`). Per
/// proposal §4.4: 24 hours, after which the resolver re-fetches.
pub const DEFAULT_MUTABLE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Filename of the timestamp marker the cache writes after a
/// successful fetch. Plain text (decimal unix-seconds) so it can be
/// inspected with `cat`.
const TIMESTAMP_FILENAME: &str = ".lex-fetched-at";

/// Resolver cache. Stateless aside from the root path + TTL — every
/// lookup re-reads the filesystem, so multiple processes sharing the
/// same root see each other's writes immediately (modulo the usual
/// caveats around concurrent fetches into the same directory; not a
/// concern at v1 since fetches run serially through `boot_registry`).
#[derive(Debug, Clone)]
pub struct ResolverCache {
    root: PathBuf,
    mutable_ttl: Duration,
}

impl ResolverCache {
    /// Create a cache rooted at `root` with the default 24-hour TTL.
    /// Creates the directory if it doesn't exist (a missing cache
    /// directory is normal on first run; not an error).
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            mutable_ttl: DEFAULT_MUTABLE_TTL,
        })
    }

    /// Create the per-user cache at `$XDG_CACHE_HOME/lex/labels`,
    /// falling back to `$HOME/.cache/lex/labels` per XDG conventions.
    pub fn user_default() -> std::io::Result<Self> {
        Self::new(Self::default_root())
    }

    /// Compute the default cache root without touching the
    /// filesystem. Exposed so [`super::resolve_namespace`] can
    /// surface the path in its [`ResolveError::CacheIo`] when
    /// [`Self::user_default`] fails.
    pub fn default_root() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("lex").join("labels");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                return PathBuf::from(home)
                    .join(".cache")
                    .join("lex")
                    .join("labels");
            }
        }
        // Last-resort fallback: a per-process temp dir. Better than
        // panicking; surfaces as "we'll re-fetch every time" which
        // is degraded but not broken.
        std::env::temp_dir().join(format!("lex-labels-{}", std::process::id()))
    }

    /// Override the mutable-rev TTL. Tests use this to force quick
    /// expiry without sleeping for 24 hours.
    pub fn with_mutable_ttl(mut self, ttl: Duration) -> Self {
        self.mutable_ttl = ttl;
        self
    }

    /// The cache root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Cache directory for a URI. Deterministic; doesn't touch the
    /// filesystem.
    pub fn entry_path(&self, uri: &ParsedUri) -> PathBuf {
        self.root.join(hash_key(uri))
    }

    /// Resolve `uri` against the cache, fetching via `fetcher` on a
    /// cache miss or expired mutable entry. The caller-facing entry
    /// point used by [`super::resolve_namespace_with`].
    ///
    /// Returns the cache directory containing the resolved schema
    /// (or the `subdir` thereof, if the URI requested one — the
    /// fetcher is responsible for honouring `subdir`).
    pub fn fetch_or_reuse(
        &self,
        uri: &ParsedUri,
        fetcher: &dyn Fetcher,
    ) -> Result<PathBuf, ResolveError> {
        let entry = self.entry_path(uri);

        // Cache hit path. Two requirements:
        //
        //   1. The completion marker (`TIMESTAMP_FILENAME`) exists
        //      and contains a parseable timestamp. The marker is
        //      written *only* after a fetcher returns Ok — its
        //      presence proves the fetch ran to completion, so a
        //      directory whose fetcher crashed mid-write isn't
        //      mistaken for a complete entry.
        //   2. For immutable refs: any complete entry is reusable
        //      forever. For mutable refs: the marker's timestamp
        //      must also be within `mutable_ttl`.
        //
        // Requiring the marker even for immutable revs is the
        // partial-fetch defence — without it, a crash mid-fetch
        // could leave a half-populated directory that the next
        // resolve would happily reuse as-is for the immutable rev.
        if entry.is_dir() {
            if let Some(fetched_at) = read_completion_marker(&entry) {
                let immutable = fetcher.is_immutable_rev(uri.rev.as_deref());
                if immutable || self.is_within_ttl(fetched_at) {
                    return Ok(entry);
                }
            }
        }

        // Miss, stale, or incomplete-from-prior-fetch — fetch fresh.
        // Wipe the entry first so a partial-write from a previous
        // failed fetch doesn't get reused via a future code change
        // that loosens the marker check.
        if entry.exists() {
            std::fs::remove_dir_all(&entry).map_err(|source| ResolveError::CacheIo {
                path: entry.clone(),
                source,
            })?;
        }
        std::fs::create_dir_all(&entry).map_err(|source| ResolveError::CacheIo {
            path: entry.clone(),
            source,
        })?;

        fetcher.fetch(uri, &entry).map_err(|source| {
            // Partial-fetch cleanup: a fetcher that wrote some
            // files then errored leaves a directory without a
            // completion marker. The next resolve would re-fetch
            // anyway (no marker → not a hit), but leaving a
            // partial entry on disk is wasteful and confusing
            // when users inspect the cache. Best-effort wipe;
            // ignore errors since we're already returning one.
            let _ = std::fs::remove_dir_all(&entry);
            ResolveError::Fetch {
                uri: uri.original.clone(),
                source,
            }
        })?;

        // Fetch succeeded → drop the completion marker. Failure to
        // write the marker is non-fatal but degrades subsequent
        // resolves to re-fetch on every call (entry won't be
        // recognised as complete). Logging this requires plumbing
        // a logger into the resolver; out of scope for now.
        let _ = self.write_timestamp(&entry);

        Ok(entry)
    }

    /// Check whether `fetched_at` is within the mutable-rev TTL.
    fn is_within_ttl(&self, fetched_at: u64) -> bool {
        let Ok(now) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) else {
            return false;
        };
        now.as_secs().saturating_sub(fetched_at) < self.mutable_ttl.as_secs()
    }

    fn write_timestamp(&self, entry: &Path) -> std::io::Result<()> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        std::fs::write(entry.join(TIMESTAMP_FILENAME), now.to_string())
    }
}

/// Read the completion marker as a Unix timestamp. Returns `None` if
/// the file is missing OR its contents don't parse as a `u64`
/// (treated as "no marker"; forces a re-fetch). The marker is the
/// indicator that a previous fetch ran to completion; immutable-rev
/// reuse and mutable-rev freshness checks both depend on it.
fn read_completion_marker(entry: &Path) -> Option<u64> {
    let stamp = entry.join(TIMESTAMP_FILENAME);
    let content = std::fs::read_to_string(&stamp).ok()?;
    content.trim().parse::<u64>().ok()
}

/// SHA-256 of the URI + rev, lowercased hex. Stable across processes
/// and machines: same `lex.toml` resolves to the same cache directory
/// everywhere. Inputs include both the body and the rev so a tag
/// change doesn't collide with the previous tag's cached content.
fn hash_key(uri: &ParsedUri) -> String {
    let mut h = Sha256::new();
    h.update(uri.scheme.as_bytes());
    h.update(b":");
    h.update(uri.body.as_bytes());
    if let Some(rev) = &uri.rev {
        h.update(b"#");
        h.update(rev.as_bytes());
    }
    if let Some(subdir) = &uri.subdir {
        h.update(b"?subdir=");
        h.update(subdir.as_bytes());
    }
    hex_encode(&h.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(uri: &str) -> ParsedUri {
        ParsedUri::parse(uri).unwrap()
    }

    #[test]
    fn hash_key_is_deterministic() {
        let a = hash_key(&parse("github:acme/repo#v1"));
        let b = hash_key(&parse("github:acme/repo#v1"));
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn hash_key_distinguishes_rev() {
        let a = hash_key(&parse("github:acme/repo#v1"));
        let b = hash_key(&parse("github:acme/repo#v2"));
        assert_ne!(a, b);
    }

    #[test]
    fn hash_key_distinguishes_scheme() {
        let a = hash_key(&parse("github:acme/repo"));
        let b = hash_key(&parse("gitlab:acme/repo"));
        assert_ne!(a, b);
    }

    #[test]
    fn entry_path_is_stable_across_cache_instances() {
        let tmp = tempfile::tempdir().unwrap();
        let cache1 = ResolverCache::new(tmp.path()).unwrap();
        let cache2 = ResolverCache::new(tmp.path()).unwrap();
        let uri = parse("github:acme/repo#v1");
        assert_eq!(cache1.entry_path(&uri), cache2.entry_path(&uri));
    }

    #[test]
    fn default_root_uses_xdg_cache_home() {
        let prev_xdg = std::env::var("XDG_CACHE_HOME").ok();
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("XDG_CACHE_HOME", "/tmp/xdg-test");
        let r = ResolverCache::default_root();
        assert_eq!(r, PathBuf::from("/tmp/xdg-test/lex/labels"));
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        // Restore HOME just in case other tests rely on it.
        if let Some(h) = prev_home {
            std::env::set_var("HOME", h);
        }
    }

    /// Mock fetcher: writes a known file into dest. Used by the
    /// freshness tests to drive the cache without involving real
    /// network IO.
    struct MockFetcher;

    impl Fetcher for MockFetcher {
        fn fetch(&self, _uri: &ParsedUri, dest: &Path) -> Result<(), super::super::FetchError> {
            std::fs::write(dest.join("schema.yaml"), b"schema_version: 1\nlabel: x.y\n")?;
            Ok(())
        }
        fn schemes(&self) -> &'static [&'static str] {
            &["mock"]
        }
    }

    /// Mock fetcher that counts how many times `fetch` was called.
    /// Used to verify cache hits don't re-fetch.
    #[derive(Default)]
    struct CountingFetcher {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl Fetcher for CountingFetcher {
        fn fetch(&self, _uri: &ParsedUri, dest: &Path) -> Result<(), super::super::FetchError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::fs::write(dest.join("schema.yaml"), b"x")?;
            Ok(())
        }
        fn schemes(&self) -> &'static [&'static str] {
            &["mock"]
        }
    }

    #[test]
    fn fetch_or_reuse_writes_to_cache_on_miss() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = ResolverCache::new(tmp.path()).unwrap();
        let uri = parse("mock:something#v1");
        let dir = cache.fetch_or_reuse(&uri, &MockFetcher).unwrap();
        assert!(dir.starts_with(tmp.path()));
        assert!(dir.join("schema.yaml").is_file());
        assert!(dir.join(TIMESTAMP_FILENAME).is_file());
    }

    #[test]
    fn fetch_or_reuse_reuses_immutable_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = ResolverCache::new(tmp.path()).unwrap();
        let uri = parse("mock:something#v1");
        let counter = CountingFetcher::default();
        // First call fetches.
        cache.fetch_or_reuse(&uri, &counter).unwrap();
        // Second call should NOT fetch — but our CountingFetcher
        // reports mutable rev, so freshness check applies. Wrap it
        // in an immutable-reporting fetcher to exercise the
        // is_immutable_rev branch.
        let immutable = ImmutableCountingFetcher::default();
        immutable
            .inner
            .calls
            .store(0, std::sync::atomic::Ordering::SeqCst);
        // Pre-populate cache via the inner counting fetcher first.
        cache.fetch_or_reuse(&uri, &immutable).unwrap();
        let after_first = immutable
            .inner
            .calls
            .load(std::sync::atomic::Ordering::SeqCst);
        cache.fetch_or_reuse(&uri, &immutable).unwrap();
        let after_second = immutable
            .inner
            .calls
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            after_first, after_second,
            "second call should be a cache hit (immutable rev), got {after_first} → {after_second}"
        );
    }

    /// Wraps `CountingFetcher` to report `is_immutable_rev == true`.
    #[derive(Default)]
    struct ImmutableCountingFetcher {
        inner: CountingFetcher,
    }

    impl Fetcher for ImmutableCountingFetcher {
        fn fetch(&self, uri: &ParsedUri, dest: &Path) -> Result<(), super::super::FetchError> {
            self.inner.fetch(uri, dest)
        }
        fn schemes(&self) -> &'static [&'static str] {
            self.inner.schemes()
        }
        fn is_immutable_rev(&self, _rev: Option<&str>) -> bool {
            true
        }
    }

    #[test]
    fn fetch_or_reuse_reuses_mutable_entry_within_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = ResolverCache::new(tmp.path()).unwrap();
        let uri = parse("mock:something#main");
        let counter = CountingFetcher::default();
        cache.fetch_or_reuse(&uri, &counter).unwrap();
        cache.fetch_or_reuse(&uri, &counter).unwrap();
        assert_eq!(
            counter.calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "second call within TTL should reuse the cached entry"
        );
    }

    #[test]
    fn fetch_or_reuse_refetches_mutable_entry_past_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        // Zero-duration TTL — every call past the first is stale.
        let cache = ResolverCache::new(tmp.path())
            .unwrap()
            .with_mutable_ttl(Duration::from_secs(0));
        let uri = parse("mock:something#main");
        let counter = CountingFetcher::default();
        cache.fetch_or_reuse(&uri, &counter).unwrap();
        // sleep(0) — the saturating_sub still reads 0 < 0 == false
        // immediately, so the entry is stale on the very next call.
        cache.fetch_or_reuse(&uri, &counter).unwrap();
        assert_eq!(
            counter.calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "second call past TTL should re-fetch"
        );
    }

    #[test]
    fn fetch_or_reuse_propagates_fetch_errors() {
        struct FailingFetcher;
        impl Fetcher for FailingFetcher {
            fn fetch(
                &self,
                _uri: &ParsedUri,
                _dest: &Path,
            ) -> Result<(), super::super::FetchError> {
                Err(super::super::FetchError::Network {
                    message: "simulated".into(),
                })
            }
            fn schemes(&self) -> &'static [&'static str] {
                &["mock"]
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let cache = ResolverCache::new(tmp.path()).unwrap();
        let uri = parse("mock:fail");
        let err = cache.fetch_or_reuse(&uri, &FailingFetcher).unwrap_err();
        match err {
            ResolveError::Fetch {
                source: super::super::FetchError::Network { .. },
                ..
            } => {}
            other => panic!("expected Fetch::Network error, got: {other}"),
        }
    }

    /// Regression: a directory left behind by a previous failed
    /// fetch (no completion marker) must not be mistaken for a
    /// complete cache entry, even when the rev is immutable.
    /// Without this, an immutable-rev fetcher that crashed
    /// mid-write would leave a partial directory that the next
    /// resolve happily reused forever, since the immutable path
    /// previously bypassed all completeness checks.
    #[test]
    fn fetch_or_reuse_does_not_reuse_partial_entry_for_immutable_rev() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = ResolverCache::new(tmp.path()).unwrap();
        let uri = parse("mock:something#v1");

        // Hand-craft a partial directory: exists, has some content,
        // but lacks the completion marker (simulating a crashed
        // mid-write fetch).
        let entry = cache.entry_path(&uri);
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("partial-thing.yaml"), b"only half written").unwrap();
        assert!(!entry.join(TIMESTAMP_FILENAME).exists());

        // Now resolve with an immutable-reporting fetcher. The
        // partial entry must be wiped and re-fetched, not reused.
        struct ImmutableMockFetcher {
            called: std::sync::atomic::AtomicUsize,
        }
        impl Fetcher for ImmutableMockFetcher {
            fn fetch(&self, _uri: &ParsedUri, dest: &Path) -> Result<(), super::super::FetchError> {
                self.called
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::fs::write(dest.join("schema.yaml"), b"complete").unwrap();
                Ok(())
            }
            fn schemes(&self) -> &'static [&'static str] {
                &["mock"]
            }
            fn is_immutable_rev(&self, _rev: Option<&str>) -> bool {
                true
            }
        }
        let fetcher = ImmutableMockFetcher {
            called: std::sync::atomic::AtomicUsize::new(0),
        };
        let dir = cache.fetch_or_reuse(&uri, &fetcher).unwrap();
        assert_eq!(
            fetcher.called.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "partial entry must be wiped and re-fetched, not reused"
        );
        // Partial file is gone; only the completed fetch's output remains.
        assert!(!dir.join("partial-thing.yaml").exists());
        assert_eq!(std::fs::read(dir.join("schema.yaml")).unwrap(), b"complete");
        assert!(dir.join(TIMESTAMP_FILENAME).is_file());
    }

    /// Regression for the same defence-in-depth: a fetcher that
    /// writes some content and then errors out must leave a clean
    /// cache (no partial directory). Subsequent resolves see a
    /// cache miss (no entry) rather than a partial entry.
    #[test]
    fn fetch_or_reuse_wipes_partial_writes_when_fetcher_errors() {
        struct PartialThenFailFetcher;
        impl Fetcher for PartialThenFailFetcher {
            fn fetch(&self, _uri: &ParsedUri, dest: &Path) -> Result<(), super::super::FetchError> {
                // Write some content, then error.
                std::fs::write(dest.join("half.yaml"), b"x").unwrap();
                Err(super::super::FetchError::Network {
                    message: "interrupted".into(),
                })
            }
            fn schemes(&self) -> &'static [&'static str] {
                &["mock"]
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let cache = ResolverCache::new(tmp.path()).unwrap();
        let uri = parse("mock:fail#partial");
        let entry = cache.entry_path(&uri);
        let _err = cache
            .fetch_or_reuse(&uri, &PartialThenFailFetcher)
            .unwrap_err();
        // The partial write must have been cleaned up.
        assert!(
            !entry.exists(),
            "partial entry should have been removed; still at {}",
            entry.display()
        );
    }
}
