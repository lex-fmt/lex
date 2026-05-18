//! URI parsing for namespace declarations.
//!
//! The accepted forms (proposal §4.2):
//!
//! - `path:<body>` — local path. `body` is everything after the
//!   colon. No `#` or `?` allowed.
//! - `<scheme>:<body>[#rev][?subdir=…]` — github/gitlab tap form.
//!   Examples: `github:acme/lex-labels`, `gitlab:foo/bar#v2.1.0`,
//!   `github:acme/lex-labels?subdir=v2`.
//! - `<scheme>://<authority><path>[#rev][?subdir=…]` — https /
//!   git+ssh. Examples: `https://example.com/labels.tar.gz#v1`,
//!   `git+ssh://git@host/path.git#main`.
//!
//! The parser doesn't validate scheme-specific shapes (it doesn't
//! check that github's `body` is `owner/repo`, doesn't check that
//! `https://` URLs have a hostname). Those validations live in the
//! per-scheme fetcher, which knows what its scheme actually needs.
//! What this parser guarantees is the syntactic split into
//! `(scheme, body, rev, subdir)` plus the few cross-cutting rules
//! the host needs to enforce upstream of dispatch.
//!
//! The recognised query keys are `subdir` and `via`. Multiple keys
//! may appear separated by `&` (e.g. `?subdir=labels&via=git`). Other
//! keys are a parse error. Multiple `?` is a parse error. Multiple
//! `#` is a parse error. These keep silently-swallowed user mistakes
//! out of the contract.
//!
//! The `via` value is opaque to the parser — templates in
//! [`super::template`] interpret it (`"https"` vs `"git"`), keeping
//! the parser layer free of forge-policy concerns.

/// Parsed URI components.
///
/// Constructed via [`ParsedUri::parse`]. The fields are public so
/// fetchers can read them directly — there's no useful invariant to
/// preserve via accessors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUri {
    /// The original input string (for diagnostics + canonical hash).
    pub original: String,
    /// The scheme — `"path"`, `"github"`, `"gitlab"`, `"https"`,
    /// `"git+ssh"`. Lowercase.
    pub scheme: String,
    /// Everything between `<scheme>:` and the first `#` or `?`.
    /// Includes the `//<authority>` portion for `https:` / `git+ssh:`
    /// — fetchers split that out themselves.
    pub body: String,
    /// The `#rev` fragment, if any. Empty fragment (`uri#`) parses
    /// to `Some("")` — caller's choice whether to treat that as an
    /// error.
    pub rev: Option<String>,
    /// The `?subdir=…` query value, if any. Empty value parses to
    /// `Some("")`.
    pub subdir: Option<String>,
    /// The `?via=…` query value, if any. Empty value parses to
    /// `Some("")`. The parser does not validate the value — templates
    /// in [`super::template`] interpret `"https"` vs `"git"`; other
    /// transports treat a `via` carried into them as a no-op.
    pub via: Option<String>,
}

/// Errors from [`ParsedUri::parse`].
#[derive(Debug)]
#[non_exhaustive]
pub enum UriParseError {
    /// No `:` separator — can't extract a scheme.
    NoScheme,
    /// Empty scheme (leading `:`).
    EmptyScheme,
    /// Multiple `#` fragments.
    MultipleFragments,
    /// Multiple `?` queries.
    MultipleQueries,
    /// `?` before `#` — fragment must come before query in our
    /// canonicalisation (matches lex-config's `canonical_uri` output).
    QueryBeforeFragment,
    /// Query string contains a key other than `subdir` / `via`. We
    /// intentionally don't accept arbitrary query strings — anything
    /// else is a typo or an unsupported feature.
    UnknownQueryKey { key: String },
    /// A query key (`subdir` or `via`) appeared twice in the same
    /// query string. We reject this so a config-file user doesn't
    /// silently get the second value of an accidental duplicate.
    DuplicateQueryKey { key: String },
    /// Query parameter without a value (`?subdir` instead of
    /// `?subdir=…`).
    QueryParamMissingValue,
    /// `via=<value>` carried a value the forge template doesn't
    /// recognise. Templates accept `via=https` (the default) and
    /// `via=git` (route through the git transport); any other value is
    /// reported here. This variant is raised by per-template logic in
    /// [`super::template`], not the parser itself — the parser is
    /// content-agnostic about the `via` value, since each template
    /// decides which transports it can route through.
    UnsupportedVia { value: String },
}

impl std::fmt::Display for UriParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UriParseError::NoScheme => write!(f, "missing scheme (expected `<scheme>:<body>`)"),
            UriParseError::EmptyScheme => write!(f, "scheme is empty"),
            UriParseError::MultipleFragments => write!(f, "multiple `#` fragments"),
            UriParseError::MultipleQueries => write!(f, "multiple `?` query strings"),
            UriParseError::QueryBeforeFragment => write!(
                f,
                "`?` appears before `#` — `#rev` must come first, then `?subdir=…`"
            ),
            UriParseError::UnknownQueryKey { key } => write!(
                f,
                "unknown query parameter `{key}` (recognised keys: `subdir`, `via`)"
            ),
            UriParseError::DuplicateQueryKey { key } => {
                write!(f, "query parameter `{key}` appears more than once")
            }
            UriParseError::QueryParamMissingValue => write!(f, "query parameter has no `=` value"),
            UriParseError::UnsupportedVia { value } => write!(
                f,
                "unsupported `via={value}` (recognised values: `https`, `git`)"
            ),
        }
    }
}

impl std::error::Error for UriParseError {}

impl ParsedUri {
    /// Parse a namespace URI string. See module docs for the accepted
    /// forms.
    pub fn parse(input: &str) -> Result<Self, UriParseError> {
        let colon = input.find(':').ok_or(UriParseError::NoScheme)?;
        if colon == 0 {
            return Err(UriParseError::EmptyScheme);
        }
        let scheme = input[..colon].to_ascii_lowercase();
        let rest = &input[colon + 1..];

        // Extract fragment + query. Fragment must come before query
        // when both are present — `<body>#<rev>?subdir=<v>`. This
        // matches `lex-config::NamespaceSpec::canonical_uri` output,
        // which is the only producer of URI strings the resolver
        // receives.
        let hash = rest.find('#');
        let question = rest.find('?');

        if let (Some(h), Some(q)) = (hash, question) {
            if q < h {
                return Err(UriParseError::QueryBeforeFragment);
            }
        }

        // Body is everything up to the first `#` or `?`.
        let body_end = match (hash, question) {
            (Some(h), Some(q)) => h.min(q),
            (Some(h), None) => h,
            (None, Some(q)) => q,
            (None, None) => rest.len(),
        };
        let body = rest[..body_end].to_string();

        // Fragment is between `#` and the next `?` (or end).
        let rev = if let Some(h) = hash {
            // Reject `<body>#<a>#<b>` — multiple fragments.
            let after_hash = &rest[h + 1..];
            if after_hash.contains('#') {
                return Err(UriParseError::MultipleFragments);
            }
            let frag_end = after_hash.find('?').unwrap_or(after_hash.len());
            Some(after_hash[..frag_end].to_string())
        } else {
            None
        };

        // Query is everything after the first `?` (which must be
        // after the `#` if present). Multi-`?` is an error.
        let (subdir, via) = if let Some(q) = question {
            let after_q = &rest[q + 1..];
            if after_q.contains('?') {
                return Err(UriParseError::MultipleQueries);
            }
            parse_query(after_q)?
        } else {
            (None, None)
        };

        Ok(Self {
            original: input.to_string(),
            scheme,
            body,
            rev,
            subdir,
            via,
        })
    }

    /// True when this URI carried a `#` fragment. Distinct from
    /// `rev.is_some()` only in that it would still be true for an
    /// empty `#` (i.e. `uri#`). The path-scheme handler uses this to
    /// reject `path:dir#` even when the rev is empty.
    pub fn has_fragment(&self) -> bool {
        self.rev.is_some()
    }

    /// True when this URI carried a `?` query. Same shape as
    /// [`Self::has_fragment`] — true regardless of which recognised
    /// key (`subdir` or `via`) appeared, and for the empty-query
    /// marker (`uri?` → `subdir = Some("")`).
    pub fn has_query(&self) -> bool {
        self.subdir.is_some() || self.via.is_some()
    }
}

/// Parse the query string after `?`. Recognised keys are
/// `subdir=<value>` and `via=<value>`; multi-key queries separate
/// pairs with `&`. Any other key is a parse error.
///
/// The `subdir` slot returns `Some(String)` whenever the URI carries
/// a `?`, even for `uri?` with an empty query string (returns
/// `Some(String::new())`). That symmetry with the fragment parser —
/// where `uri#` yields `Some("")` not `None` — is load-bearing:
/// callers (notably the `path:` resolver) inspect `has_query()` to
/// decide whether the remote-only `?` knob was used; collapsing `?`
/// to `None` would silently slip `path:dir?` past that check.
///
/// The `via` slot is `None` when the URI doesn't carry a `via=` pair;
/// it doesn't share the `subdir`-style "empty query → empty value"
/// rule because `via` is forge-policy and isn't consulted by
/// `has_query()`.
fn parse_query(q: &str) -> Result<(Option<String>, Option<String>), UriParseError> {
    if q.is_empty() {
        // `uri?` with empty query string — represent as
        // `subdir = Some("")` so `has_query()` correctly reports
        // "URI carried a `?`". `via` stays `None` (it isn't part of
        // the syntactic-presence contract).
        return Ok((Some(String::new()), None));
    }
    let mut subdir = None;
    let mut via = None;
    for pair in q.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(UriParseError::QueryParamMissingValue);
        };
        match key {
            "subdir" => {
                if subdir.is_some() {
                    return Err(UriParseError::DuplicateQueryKey {
                        key: key.to_string(),
                    });
                }
                subdir = Some(value.to_string());
            }
            "via" => {
                if via.is_some() {
                    return Err(UriParseError::DuplicateQueryKey {
                        key: key.to_string(),
                    });
                }
                via = Some(value.to_string());
            }
            _ => {
                return Err(UriParseError::UnknownQueryKey {
                    key: key.to_string(),
                });
            }
        }
    }
    Ok((subdir, via))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_path_uri() {
        let p = ParsedUri::parse("path:acme-labels").unwrap();
        assert_eq!(p.scheme, "path");
        assert_eq!(p.body, "acme-labels");
        assert_eq!(p.rev, None);
        assert_eq!(p.subdir, None);
    }

    #[test]
    fn parses_github_with_rev_and_subdir() {
        let p = ParsedUri::parse("github:acme/repo#v1.2.0?subdir=labels").unwrap();
        assert_eq!(p.scheme, "github");
        assert_eq!(p.body, "acme/repo");
        assert_eq!(p.rev.as_deref(), Some("v1.2.0"));
        assert_eq!(p.subdir.as_deref(), Some("labels"));
    }

    #[test]
    fn parses_https_with_authority() {
        let p = ParsedUri::parse("https://example.com/path.tar.gz#main").unwrap();
        assert_eq!(p.scheme, "https");
        assert_eq!(p.body, "//example.com/path.tar.gz");
        assert_eq!(p.rev.as_deref(), Some("main"));
    }

    #[test]
    fn parses_git_ssh_uri() {
        let p = ParsedUri::parse("git+ssh://git@host/path.git#v1").unwrap();
        assert_eq!(p.scheme, "git+ssh");
        assert_eq!(p.body, "//git@host/path.git");
        assert_eq!(p.rev.as_deref(), Some("v1"));
    }

    #[test]
    fn lowercases_scheme() {
        let p = ParsedUri::parse("GITHUB:acme/repo").unwrap();
        assert_eq!(p.scheme, "github");
    }

    #[test]
    fn rejects_no_scheme() {
        let err = ParsedUri::parse("acme-labels").unwrap_err();
        assert!(matches!(err, UriParseError::NoScheme));
    }

    #[test]
    fn rejects_empty_scheme() {
        let err = ParsedUri::parse(":acme/repo").unwrap_err();
        assert!(matches!(err, UriParseError::EmptyScheme));
    }

    #[test]
    fn rejects_multiple_fragments() {
        let err = ParsedUri::parse("github:acme/repo#a#b").unwrap_err();
        assert!(matches!(err, UriParseError::MultipleFragments));
    }

    #[test]
    fn rejects_query_before_fragment() {
        let err = ParsedUri::parse("github:acme/repo?subdir=x#rev").unwrap_err();
        assert!(matches!(err, UriParseError::QueryBeforeFragment));
    }

    #[test]
    fn rejects_unknown_query_key() {
        let err = ParsedUri::parse("github:acme/repo?otherkey=x").unwrap_err();
        assert!(matches!(err, UriParseError::UnknownQueryKey { .. }));
    }

    #[test]
    fn rejects_query_without_value() {
        let err = ParsedUri::parse("github:acme/repo?subdir").unwrap_err();
        assert!(matches!(err, UriParseError::QueryParamMissingValue));
    }

    #[test]
    fn accepts_subdir_and_via_together() {
        let p = ParsedUri::parse("github:acme/repo#v1?subdir=labels&via=git").unwrap();
        assert_eq!(p.rev.as_deref(), Some("v1"));
        assert_eq!(p.subdir.as_deref(), Some("labels"));
        assert_eq!(p.via.as_deref(), Some("git"));
    }

    #[test]
    fn accepts_via_alone() {
        let p = ParsedUri::parse("github:acme/repo?via=git").unwrap();
        assert_eq!(p.via.as_deref(), Some("git"));
        assert_eq!(p.subdir, None);
        // `?via=git` is a recognised key, not the empty-query syntactic
        // marker — so `has_query()` is `true` (URI carried a `?`) but
        // subdir didn't get the empty-string sentinel.
        assert!(p.has_query());
    }

    #[test]
    fn rejects_multi_query_with_unknown_key() {
        let err = ParsedUri::parse("github:acme/repo#v1?subdir=a&otherkey=b").unwrap_err();
        assert!(matches!(err, UriParseError::UnknownQueryKey { key } if key == "otherkey"));
    }

    #[test]
    fn rejects_duplicate_subdir_query_key() {
        let err = ParsedUri::parse("github:acme/repo?subdir=a&subdir=b").unwrap_err();
        assert!(matches!(err, UriParseError::DuplicateQueryKey { key } if key == "subdir"));
    }

    #[test]
    fn rejects_duplicate_via_query_key() {
        let err = ParsedUri::parse("github:acme/repo?via=git&via=https").unwrap_err();
        assert!(matches!(err, UriParseError::DuplicateQueryKey { key } if key == "via"));
    }

    #[test]
    fn empty_query_after_question_is_some_empty_not_none() {
        // `uri#rev?` — the trailing `?` parses to `Some("")` so
        // `has_query()` correctly reports the syntactic presence of
        // the query separator. The `path:` resolver relies on this
        // to reject `path:dir?` (see
        // `path_dir_with_empty_question_is_rejected_by_path_resolver`).
        let p = ParsedUri::parse("github:acme/repo#v1?").unwrap();
        assert_eq!(p.rev.as_deref(), Some("v1"));
        assert_eq!(p.subdir.as_deref(), Some(""));
        assert!(p.has_query());
    }

    #[test]
    fn fragment_without_rev_value_is_some_empty() {
        // `uri#` with empty fragment — parses to Some(""). The
        // path-scheme handler treats this as "URI has fragment" and
        // rejects appropriately; remote schemes will too if the
        // empty rev confuses them.
        let p = ParsedUri::parse("github:acme/repo#").unwrap();
        assert_eq!(p.rev.as_deref(), Some(""));
        assert!(p.has_fragment());
    }
}
