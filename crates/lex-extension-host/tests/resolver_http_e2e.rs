//! End-to-end validation of the resolver machinery using a hand-
//! rolled HTTP fetcher against a local mock server. The point isn't
//! to test HTTP — the real [`lex_extension_host::HttpsFetcher`] has
//! its own e2e test in `https_fetcher_e2e.rs`. This test proves the
//! generic machinery (trait + registry + cache + dispatch + cache-key
//! by `(uri, rev)`) works for a non-stub fetcher: URI parse → registry
//! dispatch → cache miss → fetch → cache hit on second resolve.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use lex_extension_host::{
    resolve_namespace_with, FetchError, Fetcher, FetcherRegistry, ParsedUri, ResolverCache,
};

/// Test fetcher: hand-rolled minimal HTTP client. `prefix` is a
/// `http://127.0.0.1:<port>` URL stub the fetcher prepends to the
/// URI body. The full URL is `{prefix}{uri.body}` (so a URI like
/// `mockhttp:/foo/bar.yaml` becomes a GET to `{prefix}/foo/bar.yaml`).
///
/// Writes the response body to `<dest>/schema.yaml`. Single file,
/// fixed filename — enough to prove the round-trip; real fetchers
/// extract a directory tree.
struct TestHttpFetcher {
    prefix: String,
    request_count: Arc<AtomicUsize>,
}

impl Fetcher for TestHttpFetcher {
    fn fetch(&self, uri: &ParsedUri, dest: &Path) -> Result<(), FetchError> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        let url = format!("{}{}", self.prefix, uri.body);
        let body = http_get(&url).map_err(|e| FetchError::Network {
            message: format!("GET {url}: {e}"),
        })?;
        std::fs::write(dest.join("schema.yaml"), body)?;
        Ok(())
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["mockhttp"]
    }
}

/// Hand-rolled minimal HTTP/1.1 GET. Accepts `http://host:port/path`,
/// returns the response body. Headers + status line are discarded;
/// the test mock server always returns 200 OK with a predictable
/// body, so we don't need a real parser.
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

    // Skip status + headers (find \r\n\r\n).
    let separator = b"\r\n\r\n";
    let split = response
        .windows(separator.len())
        .position(|w| w == separator)
        .ok_or_else(|| std::io::Error::other("malformed response (no header/body separator)"))?;
    Ok(response[split + separator.len()..].to_vec())
}

/// Spawn a local HTTP mock server that responds to every GET with
/// `200 OK` + the supplied body. Returns just the address it's
/// listening on; the listener thread is detached and runs until the
/// test binary exits. We accept the thread "leak" because the test
/// binary's lifetime is short (single test, sub-second wall time)
/// and a shutdown mechanism (atomic flag, request limit, channel
/// close) is more complexity than the test needs at this scale —
/// future expansion of the integration suite can wire one up if a
/// single binary ends up spawning many of these.
fn spawn_mock_server(body: &'static [u8], request_count: Arc<AtomicUsize>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind localhost");
    let addr = listener.local_addr().expect("local addr");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            request_count.fetch_add(1, Ordering::SeqCst);
            // Read until \r\n\r\n so the client's request is
            // drained before we respond. Without this, on some OSes
            // the client sees Connection Reset before reading the
            // response.
            let mut reader = BufReader::new(s.try_clone().unwrap());
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

const FIXTURE_SCHEMA: &[u8] = b"schema_version: 1\nlabel: mock.x\n";

#[test]
fn resolver_machinery_round_trips_a_real_http_fetcher() {
    // Mock server returns a fixed YAML body for any GET. The
    // server_calls counter tracks how many requests the *server*
    // sees — separate from the fetcher's own counter — so we can
    // confirm the cache short-circuits the network on the second
    // resolve.
    let server_calls = Arc::new(AtomicUsize::new(0));
    let addr = spawn_mock_server(FIXTURE_SCHEMA, Arc::clone(&server_calls));

    let prefix = format!("http://{addr}");
    let fetcher_calls = Arc::new(AtomicUsize::new(0));
    let fetcher = Arc::new(TestHttpFetcher {
        prefix,
        request_count: Arc::clone(&fetcher_calls),
    });

    let mut registry = FetcherRegistry::new();
    registry.register(fetcher);

    let cache_root = tempfile::tempdir().expect("tempdir");
    let cache = ResolverCache::new(cache_root.path()).expect("cache root");

    let workspace = tempfile::tempdir().expect("workspace");

    // First resolve: cache miss → HTTP fetch → file in cache dir.
    let resolved1 = resolve_namespace_with(
        "mockhttp:/labels/acme.yaml",
        workspace.path(),
        &registry,
        &cache,
    )
    .expect("first resolve");
    assert!(
        resolved1.schema_dir.starts_with(cache_root.path()),
        "cache dir should be under cache root, got: {}",
        resolved1.schema_dir.display()
    );
    assert_eq!(
        std::fs::read(resolved1.schema_dir.join("schema.yaml")).unwrap(),
        FIXTURE_SCHEMA,
        "fetched body should match server's response"
    );
    assert_eq!(
        fetcher_calls.load(Ordering::SeqCst),
        1,
        "fetcher should have been invoked once on cache miss"
    );
    assert_eq!(
        server_calls.load(Ordering::SeqCst),
        1,
        "server should have seen one request"
    );

    // Second resolve, same URI: cache hit → no fetch.
    let resolved2 = resolve_namespace_with(
        "mockhttp:/labels/acme.yaml",
        workspace.path(),
        &registry,
        &cache,
    )
    .expect("second resolve");
    assert_eq!(
        resolved1.schema_dir, resolved2.schema_dir,
        "second resolve should return the same cache dir"
    );
    assert_eq!(
        fetcher_calls.load(Ordering::SeqCst),
        1,
        "fetcher should NOT have been called again on cache hit"
    );
    assert_eq!(
        server_calls.load(Ordering::SeqCst),
        1,
        "server should NOT have seen a second request"
    );

    // Different rev → different cache entry → fetch again.
    let resolved3 = resolve_namespace_with(
        "mockhttp:/labels/acme.yaml#v2",
        workspace.path(),
        &registry,
        &cache,
    )
    .expect("third resolve");
    assert_ne!(
        resolved1.schema_dir, resolved3.schema_dir,
        "different rev should hash to a different cache dir"
    );
    assert_eq!(
        fetcher_calls.load(Ordering::SeqCst),
        2,
        "different rev should trigger a new fetch"
    );
}
