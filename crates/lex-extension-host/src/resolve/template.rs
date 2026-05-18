//! URL-template expansion for forge shorthands.
//!
//! A URL template is a pure function over [`ParsedUri`] that turns a
//! forge-shorthand URI into a transport URI. Templates have no
//! [`super::Fetcher`] impl — they don't do any IO; they just rewrite
//! the URI so the dispatch layer hands the expanded form to the right
//! transport fetcher.
//!
//! Two templates ship today:
//!
//! - `github:owner/repo[#rev][?subdir=…]` — expands to a GitHub
//!   tarball-API URL (`https:` transport, default) or a git clone URL
//!   (`git:` transport, when `via = "git"` — knob plumbing pending,
//!   see [#10.2 of the stores spec]). Tracked at lex#562 alongside the
//!   underlying https/git fetchers.
//! - `gitlab:owner/repo[#rev][?subdir=…]` — same shape, gitlab.com.
//!
//! New templates (bitbucket, gitea, codeberg, sourcehut) drop in here
//! as additional `*_template` functions plus a match arm in [`expand`].

use super::uri::{ParsedUri, UriParseError};

/// Expand any URL-template URI into its underlying transport URI.
/// Non-template URIs (`path:`, `https:`, `git:`, `git+ssh:`) pass
/// through unchanged.
///
/// The `original` field of the returned [`ParsedUri`] is preserved
/// from the input, so error messages can refer to what the user
/// actually wrote (`github:acme/lex-labels`) rather than the expanded
/// form (`https://api.github.com/...`). The `subdir` is also
/// preserved across the expansion.
pub(super) fn expand(uri: ParsedUri) -> Result<ParsedUri, UriParseError> {
    match uri.scheme.as_str() {
        "github" => github_template(uri),
        "gitlab" => gitlab_template(uri),
        _ => Ok(uri),
    }
}

/// Expand `github:owner/repo[#rev]` into the GitHub tarball-API
/// `https:` URI. Default ref is `HEAD` (the repo's default branch).
///
/// The `via` knob (https vs. git) is not yet plumbed through
/// `lex-config`; templates currently default to https. When `via` is
/// wired up, a `via = "git"` case will produce a
/// `git:https://github.com/owner/repo.git` URI instead.
fn github_template(uri: ParsedUri) -> Result<ParsedUri, UriParseError> {
    let owner_repo = uri.body.trim_start_matches('/');
    let rev = uri.rev.as_deref().unwrap_or("HEAD");
    let expanded_str = format!("https://api.github.com/repos/{owner_repo}/tarball/{rev}");

    let mut expanded = ParsedUri::parse(&expanded_str)?;
    expanded.original = uri.original;
    expanded.subdir = uri.subdir;
    Ok(expanded)
}

/// Expand `gitlab:owner/repo[#rev]` into the GitLab archive-API
/// `https:` URI. GitLab's archive endpoint is path-shaped rather than
/// API-shaped: `https://gitlab.com/<owner>/<repo>/-/archive/<ref>/<repo>-<ref>.tar.gz`.
/// Default ref is `HEAD`.
fn gitlab_template(uri: ParsedUri) -> Result<ParsedUri, UriParseError> {
    let owner_repo = uri.body.trim_start_matches('/');
    let repo = owner_repo
        .rsplit_once('/')
        .map(|(_, r)| r)
        .unwrap_or(owner_repo);
    let rev = uri.rev.as_deref().unwrap_or("HEAD");
    let expanded_str =
        format!("https://gitlab.com/{owner_repo}/-/archive/{rev}/{repo}-{rev}.tar.gz");

    let mut expanded = ParsedUri::parse(&expanded_str)?;
    expanded.original = uri.original;
    expanded.subdir = uri.subdir;
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_no_rev_expands_to_head_tarball() {
        let input = ParsedUri::parse("github:acme/lex-labels").unwrap();
        let out = expand(input).unwrap();
        assert_eq!(out.scheme, "https");
        assert_eq!(
            out.body,
            "//api.github.com/repos/acme/lex-labels/tarball/HEAD"
        );
        assert_eq!(out.original, "github:acme/lex-labels");
    }

    #[test]
    fn github_with_rev_expands_to_tagged_tarball() {
        let input = ParsedUri::parse("github:acme/lex-labels#v1.2.0").unwrap();
        let out = expand(input).unwrap();
        assert_eq!(out.scheme, "https");
        assert_eq!(
            out.body,
            "//api.github.com/repos/acme/lex-labels/tarball/v1.2.0"
        );
    }

    #[test]
    fn github_preserves_subdir_across_expansion() {
        let input = ParsedUri::parse("github:acme/lex-labels?subdir=schemas").unwrap();
        let out = expand(input).unwrap();
        assert_eq!(out.scheme, "https");
        assert_eq!(out.subdir.as_deref(), Some("schemas"));
    }

    #[test]
    fn github_preserves_original_uri_for_diagnostics() {
        let input = ParsedUri::parse("github:acme/lex-labels#main").unwrap();
        let out = expand(input).unwrap();
        assert_eq!(out.original, "github:acme/lex-labels#main");
    }

    #[test]
    fn gitlab_no_rev_expands_to_head_archive() {
        let input = ParsedUri::parse("gitlab:foolco/lex-labels").unwrap();
        let out = expand(input).unwrap();
        assert_eq!(out.scheme, "https");
        assert_eq!(
            out.body,
            "//gitlab.com/foolco/lex-labels/-/archive/HEAD/lex-labels-HEAD.tar.gz"
        );
    }

    #[test]
    fn gitlab_with_rev_expands_to_tagged_archive() {
        let input = ParsedUri::parse("gitlab:foolco/lex-labels#v2.1.0").unwrap();
        let out = expand(input).unwrap();
        assert_eq!(out.scheme, "https");
        assert_eq!(
            out.body,
            "//gitlab.com/foolco/lex-labels/-/archive/v2.1.0/lex-labels-v2.1.0.tar.gz"
        );
    }

    #[test]
    fn non_template_uri_passes_through_unchanged() {
        for input_str in [
            "path:./local",
            "https://example.com/foo.tar.gz",
            "git+ssh://git@host/repo.git#main",
        ] {
            let input = ParsedUri::parse(input_str).unwrap();
            let out = expand(input.clone()).unwrap();
            assert_eq!(out, input, "non-template URI was rewritten: {input_str}");
        }
    }
}
