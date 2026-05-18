//! End-to-end validation of the `gitlab:` URL template through the
//! resolver pipeline.
//!
//! The point isn't to test gitlab.com (the real public archive
//! endpoint is exercised opportunistically by ignored tests in
//! [`https_fetcher_e2e`]) — it's to prove that the URL-template
//! expansion + registry dispatch + cache layers handle gitlab
//! shorthands end-to-end:
//!
//! - `gitlab:owner/repo` expands to the archive URL and round-trips
//!   through the cache like any other `https:` fetch.
//! - Slashed refs (`feature/foo`) keep the `/` in the URL path but
//!   substitute `-` in the filename component — the integration
//!   counterpart to the `gitlab_slashed_rev_dashes_the_filename` unit
//!   test in [`super::template`].
//! - The cache short-circuits a second resolve of the same URI.
//!
//! The `via=git` branch is covered by the unit tests in
//! [`super::template`] (`gitlab_via_git_*`); the URL shape is what
//! matters there, and the underlying [`super::fetcher::GitFetcher`]
//! is exercised by its own e2e suite (`git_fetcher_e2e.rs`). A
//! parallel integration test here would duplicate that coverage
//! without adding signal.
//!
//! Mock-server style is the same as
//! [`resolver_http_e2e`]: hand-rolled HTTP/1.1 GET, fixed `200 OK +
//! body` response, listener thread detached until the test binary
//! exits.
//!
//! ## How the stub fetcher intercepts gitlab.com
//!
//! The `gitlab:` template expands to a URL pointing at
//! `https://gitlab.com/…`. To exercise the full pipeline without
//! hitting the real gitlab.com (and to assert what URL would actually
//! be requested), we register a stub fetcher that claims the `https`
//! scheme — it rewrites the `gitlab.com` authority to a local mock
//! server's `127.0.0.1:<port>` and records the request path it would
//! have seen. This avoids touching the network while still proving
//! the template-to-transport hop dispatches correctly.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use lex_extension_host::{
    resolve_namespace_with, FetchError, Fetcher, FetcherRegistry, ParsedUri, ResolverCache,
};

/// Stub fetcher claiming the `https:` scheme so it intercepts the
/// gitlab template's expansion. Rewrites the `gitlab.com` authority
/// to a local mock server and records the URL path the request
/// targeted (for path-shape assertions). Writes the response body to
/// `<dest>/schema.yaml` so [`ResolverCache`] sees a populated cache
/// dir.
struct StubGitlabHttpsFetcher {
    /// `http://127.0.0.1:<port>` — replaces `https://gitlab.com` in
    /// the expanded URL.
    mock_prefix: String,
    /// Counts how many times `fetch` was invoked. The cache should
    /// short-circuit additional calls when the same URI is resolved
    /// twice.
    fetch_count: Arc<AtomicUsize>,
    /// Captures the URL path (everything after the host) that the
    /// fetcher would have requested. The test reads this to assert
    /// the URL shape the template produced.
    last_path: Arc<Mutex<Option<String>>>,
}

impl Fetcher for StubGitlabHttpsFetcher {
    fn fetch(&self, uri: &ParsedUri, dest: &Path) -> Result<(), FetchError> {
        self.fetch_count.fetch_add(1, Ordering::SeqCst);

        // The expanded URI from the gitlab template has scheme="https"
        // and body="//gitlab.com/<owner_repo>/-/archive/<ref>/<...>.tar.gz".
        // Strip the `//gitlab.com` authority so we can fire the GET
        // at the local mock server instead.
        let path = uri
            .body
            .strip_prefix("//gitlab.com")
            .ok_or_else(|| FetchError::Network {
                message: format!(
                    "stub expected gitlab.com authority in body, got: {}",
                    uri.body
                ),
            })?;

        *self.last_path.lock().unwrap() = Some(path.to_string());

        let url = format!("{}{}", self.mock_prefix, path);
        let body = http_get(&url).map_err(|e| FetchError::Network {
            message: format!("GET {url}: {e}"),
        })?;
        std::fs::write(dest.join("schema.yaml"), body)?;
        Ok(())
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["https"]
    }
}

/// Hand-rolled minimal HTTP/1.1 GET — mirror of the helper in
/// `resolver_http_e2e.rs`. The mock server always returns 200 OK with
/// a fixed body, so we don't need a real parser.
fn http_get(url: &str) -> std::io::Result<Vec<u8>> {
    let stripped = url
        .strip_prefix("http://")
        .ok_or_else(|| std::io::Error::other("only http:// supported"))?;
    let (authority, path) = stripped.split_once('/').unwrap_or((stripped, ""));
    let addr: SocketAddr = authority
        .parse()
        .map_err(|e| std::io::Error::other(format!("bad addr `{authority}`: {e}")))?;
    let mut stream = TcpStream::connect(addr)?;
    let request =
        format!("GET /{path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n",);
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;

    let separator = b"\r\n\r\n";
    let split = response
        .windows(separator.len())
        .position(|w| w == separator)
        .ok_or_else(|| std::io::Error::other("malformed response (no header/body separator)"))?;
    Ok(response[split + separator.len()..].to_vec())
}

/// Spawn a local HTTP mock server that responds to every GET with
/// `200 OK` + the supplied body. Records each request's URL path
/// (the GET request line's target) so the test can verify what the
/// fetcher actually requested. The listener thread is detached and
/// runs until the test binary exits — fine at this scale.
fn spawn_mock_server(
    body: &'static [u8],
    request_count: Arc<AtomicUsize>,
    request_paths: Arc<Mutex<Vec<String>>>,
) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind localhost");
    let addr = listener.local_addr().expect("local addr");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            request_count.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(s.try_clone().unwrap());
            let mut request_line = String::new();
            let _ = reader.read_line(&mut request_line);
            // Capture the path from the request line:
            // `GET /foo/bar HTTP/1.1\r\n`.
            if let Some(path) = request_line.split_whitespace().nth(1) {
                request_paths.lock().unwrap().push(path.to_string());
            }
            // Drain remaining headers.
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok() {
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                line.clear();
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(response.as_bytes());
            let _ = s.write_all(body);
            let _ = s.flush();
        }
    });
    addr
}

const FIXTURE_SCHEMA: &[u8] = b"schema_version: 1\nlabel: gitlab-mock.x\n";

/// Build a registry containing the stub gitlab https fetcher
/// pointed at `mock_addr`. Returns the fetcher's invocation counter
/// and its last-seen path slot so the test can assert against them.
fn build_registry(
    mock_addr: SocketAddr,
) -> (
    FetcherRegistry,
    Arc<AtomicUsize>,
    Arc<Mutex<Option<String>>>,
) {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let last_path = Arc::new(Mutex::new(None));
    let fetcher = Arc::new(StubGitlabHttpsFetcher {
        mock_prefix: format!("http://{mock_addr}"),
        fetch_count: Arc::clone(&fetch_count),
        last_path: Arc::clone(&last_path),
    });
    let mut registry = FetcherRegistry::new();
    registry.register(fetcher);
    (registry, fetch_count, last_path)
}

#[test]
fn gitlab_table_form_resolves_via_https_default() {
    // `gitlab:foolco/lex-labels` with no `via` knob defaults to the
    // archive (https) path. The stub fetcher returns a populated
    // schema dir; the resolver wires it into the cache and hands us
    // back the cache path.
    let server_calls = Arc::new(AtomicUsize::new(0));
    let server_paths: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let addr = spawn_mock_server(
        FIXTURE_SCHEMA,
        Arc::clone(&server_calls),
        Arc::clone(&server_paths),
    );

    let (registry, fetcher_calls, last_path) = build_registry(addr);
    let cache_root = tempfile::tempdir().expect("cache tempdir");
    let cache = ResolverCache::new(cache_root.path()).expect("init cache");
    let workspace = tempfile::tempdir().expect("workspace tempdir");

    let resolved = resolve_namespace_with(
        "gitlab:foolco/lex-labels",
        workspace.path(),
        &registry,
        &cache,
    )
    .expect("gitlab template should resolve via the stub https fetcher");

    assert!(
        resolved.schema_dir.starts_with(cache_root.path()),
        "cache dir should be under cache root, got: {}",
        resolved.schema_dir.display()
    );
    assert_eq!(
        std::fs::read(resolved.schema_dir.join("schema.yaml")).unwrap(),
        FIXTURE_SCHEMA,
        "fetched body should match server's response"
    );
    assert_eq!(
        fetcher_calls.load(Ordering::SeqCst),
        1,
        "fetcher should have been invoked once on cache miss"
    );

    // URL-shape sanity: the template's default expansion targets
    // `/<owner>/<repo>/-/archive/HEAD/<repo>-HEAD.tar.gz`.
    let path = last_path
        .lock()
        .unwrap()
        .clone()
        .expect("fetcher should have recorded a path");
    assert_eq!(
        path, "/foolco/lex-labels/-/archive/HEAD/lex-labels-HEAD.tar.gz",
        "default gitlab expansion should target the HEAD archive"
    );
}

#[test]
fn gitlab_slashed_rev_resolves_with_dashed_filename() {
    // Integration counterpart to the `gitlab_slashed_rev_dashes_the_filename`
    // unit test: a ref like `feature/foo` keeps the `/` in the URL
    // path (`/-/archive/feature/foo/…`) but uses `-` in the archive
    // filename (`lex-labels-feature-foo.tar.gz`). Without the
    // substitution the URL would have a stray path separator inside
    // the filename component.
    let server_calls = Arc::new(AtomicUsize::new(0));
    let server_paths: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let addr = spawn_mock_server(
        FIXTURE_SCHEMA,
        Arc::clone(&server_calls),
        Arc::clone(&server_paths),
    );

    let (registry, _fetcher_calls, last_path) = build_registry(addr);
    let cache_root = tempfile::tempdir().expect("cache tempdir");
    let cache = ResolverCache::new(cache_root.path()).expect("init cache");
    let workspace = tempfile::tempdir().expect("workspace tempdir");

    resolve_namespace_with(
        "gitlab:foolco/lex-labels#feature/foo",
        workspace.path(),
        &registry,
        &cache,
    )
    .expect("gitlab template should resolve a slashed rev");

    // Assert the URL the fetcher tried to GET. Two invariants:
    //  - the archive path keeps the `/` in `feature/foo` (GitLab
    //    accepts multi-segment paths).
    //  - the filename uses `-` in place of `/`
    //    (`lex-labels-feature-foo.tar.gz`).
    let path = last_path
        .lock()
        .unwrap()
        .clone()
        .expect("fetcher should have recorded a path");
    assert!(
        path.contains("/-/archive/feature/foo/"),
        "archive path should keep slashed ref intact, got: {path}"
    );
    assert!(
        path.ends_with("lex-labels-feature-foo.tar.gz"),
        "archive filename should dash slashed ref, got: {path}"
    );

    // Cross-check against the server side too: the request actually
    // landed on the wire with the same shape.
    let recorded = server_paths.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1, "expected one request, got: {recorded:?}");
    assert_eq!(
        recorded[0], path,
        "server-side recorded path should match fetcher-side recorded path"
    );
}

#[test]
fn gitlab_cache_reuse_on_second_resolve() {
    // Resolve the same `gitlab:` URI twice. The first call is a
    // cache miss → fetcher runs. The second call should be a cache
    // hit → fetcher does NOT run. This proves the cache key
    // ((expanded URI, rev) tuple) survives template expansion intact.
    let server_calls = Arc::new(AtomicUsize::new(0));
    let server_paths: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let addr = spawn_mock_server(
        FIXTURE_SCHEMA,
        Arc::clone(&server_calls),
        Arc::clone(&server_paths),
    );

    let (registry, fetcher_calls, _last_path) = build_registry(addr);
    let cache_root = tempfile::tempdir().expect("cache tempdir");
    let cache = ResolverCache::new(cache_root.path()).expect("init cache");
    let workspace = tempfile::tempdir().expect("workspace tempdir");

    let resolved1 = resolve_namespace_with(
        "gitlab:foolco/lex-labels#v2.1.0",
        workspace.path(),
        &registry,
        &cache,
    )
    .expect("first resolve");
    assert_eq!(
        fetcher_calls.load(Ordering::SeqCst),
        1,
        "fetcher should fire once on cache miss"
    );

    let resolved2 = resolve_namespace_with(
        "gitlab:foolco/lex-labels#v2.1.0",
        workspace.path(),
        &registry,
        &cache,
    )
    .expect("second resolve");
    assert_eq!(
        resolved1.schema_dir, resolved2.schema_dir,
        "second resolve should hit the same cache dir"
    );
    assert_eq!(
        fetcher_calls.load(Ordering::SeqCst),
        1,
        "fetcher should NOT have fired again on cache hit"
    );
}
