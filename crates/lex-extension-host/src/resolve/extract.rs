//! Archive extraction for the HTTPS fetcher.
//!
//! The HTTPS fetcher hands archive bytes (gzipped tarball or zip) to
//! [`extract_archive_into`], which writes the extracted tree into a
//! caller-provided destination directory. Two concerns this module
//! owns and the fetcher does not:
//!
//! - **Format dispatch.** Tar.gz vs. zip by signal (Content-Type and
//!   URL/path hint).
//! - **Path-traversal defence.** Both tar and zip allow archive
//!   members to declare paths like `../../etc/passwd`; we reject any
//!   member whose resolved path escapes `dest`. (Zip-slip / tar-slip.)
//!
//! The HTTPS fetcher stays thin (just GET + dispatch into this
//! module), so this code can be exercised in isolation against
//! in-memory archives without needing a TLS mock server.

use std::fs;
use std::io::{self, Read, Seek};
use std::path::{Component, Path, PathBuf};

/// The two archive shapes this module can extract. The HTTPS fetcher
/// resolves the format from the HTTP `Content-Type` header with a
/// URL-extension fallback (see [`detect_format`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ArchiveFormat {
    /// Gzipped tarball (`application/gzip`, `application/x-gzip`,
    /// `application/x-tar` with gzip content, or `.tar.gz` / `.tgz`).
    TarGz,
    /// Zip archive (`application/zip` or `.zip`).
    Zip,
}

/// Errors from archive extraction. The HTTPS fetcher wraps these into
/// [`super::FetchError::Extract`] for the host's diagnostic surface.
#[derive(Debug)]
pub(super) enum ExtractError {
    /// Archive bytes couldn't be parsed (corrupt header, truncated,
    /// wrong format for the declared type).
    Corrupt { message: String },
    /// An archive member's path escapes the destination directory
    /// (zip-slip / tar-slip).
    PathEscape { member: String },
    /// An archive member has an absolute path (`/etc/passwd`); we
    /// reject these the same way we reject path-escape, but with a
    /// distinct error so diagnostics can be specific.
    AbsoluteMember { member: String },
    /// The `subdir` filter matched no entries — almost always a
    /// misconfigured `subdir = "..."` in `lex.toml`.
    SubdirNotFound { subdir: String },
    /// IO failure on the destination side (out of disk, permission
    /// denied, …).
    Io(io::Error),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::Corrupt { message } => write!(f, "corrupt archive: {message}"),
            ExtractError::PathEscape { member } => write!(
                f,
                "archive member `{member}` resolves outside the destination directory (zip-slip / tar-slip)"
            ),
            ExtractError::AbsoluteMember { member } => write!(
                f,
                "archive member `{member}` has an absolute path; refusing for safety"
            ),
            ExtractError::SubdirNotFound { subdir } => write!(
                f,
                "archive contains no entries under subdir `{subdir}`"
            ),
            ExtractError::Io(e) => write!(f, "extraction io error: {e}"),
        }
    }
}

impl std::error::Error for ExtractError {}

impl From<io::Error> for ExtractError {
    fn from(e: io::Error) -> Self {
        ExtractError::Io(e)
    }
}

/// Detect the archive format from the HTTP `Content-Type` header
/// (preferred) with a URL-path extension fallback.
///
/// Returns [`ArchiveFormat::TarGz`] as the last-resort default — most
/// public tarball APIs return `application/octet-stream` or no useful
/// type at all, and tar.gz is by far the more common archive on the
/// wire for forge tarballs.
pub(super) fn detect_format(content_type: Option<&str>, url_path: &str) -> ArchiveFormat {
    if let Some(ct) = content_type {
        let ct = ct.to_ascii_lowercase();
        // Strip any `; charset=…` suffix; only the main type matters.
        let ct = ct.split(';').next().unwrap_or(&ct).trim();
        match ct {
            "application/zip" | "application/x-zip-compressed" => return ArchiveFormat::Zip,
            "application/gzip"
            | "application/x-gzip"
            | "application/x-tar"
            | "application/x-compressed-tar" => return ArchiveFormat::TarGz,
            _ => {} // Fall through to URL-extension sniffing.
        }
    }

    if url_path.to_ascii_lowercase().ends_with(".zip") {
        ArchiveFormat::Zip
    } else {
        // tar.gz / .tgz / unknown all default to tar.gz — that's by
        // far the more common archive on the wire for forge tarballs.
        ArchiveFormat::TarGz
    }
}

/// Extract an archive from `reader` (in `format` shape) into `dest`.
/// If `subdir` is supplied, only files under that path within the
/// archive are extracted, and they're written into `dest` directly
/// (no leading `subdir/` prefix — the schema loader scans `dest`
/// flat).
///
/// `reader` is taken as `Read + Seek` so the caller can pass either a
/// `Cursor<&[u8]>` (test path, in-memory archive fixture) or a
/// `std::fs::File` over a temp file (production path: the HTTPS
/// fetcher streams the response body to a tempfile to avoid buffering
/// up to 256 MiB in memory). `tar` doesn't need Seek; `zip` does;
/// the unified bound keeps the signature uniform.
///
/// `dest` must exist and be a directory; the caller (the cache layer)
/// is responsible for creating it.
pub(super) fn extract_archive_into<R: Read + Seek>(
    mut reader: R,
    format: ArchiveFormat,
    dest: &Path,
    subdir: Option<&str>,
) -> Result<(), ExtractError> {
    let dest = dest
        .canonicalize()
        .map_err(|e| ExtractError::Io(io::Error::new(e.kind(), format!("dest {dest:?}: {e}"))))?;
    let subdir_normalized = subdir.map(normalize_subdir);

    // `matched_any` is `true` upfront when no subdir filter is set
    // (every entry counts) and starts `false` when a subdir filter is
    // set — only entries that survive both the subdir filter AND the
    // entry-type filter (i.e. a real file we'd actually write) flip
    // it. The earlier formulation flipped it on the subdir filter
    // alone, which meant a tarball whose only subdir-matched entries
    // were symlinks would silently produce an empty dest instead of
    // surfacing SubdirNotFound.
    let mut matched_any = subdir_normalized.is_none();

    match format {
        ArchiveFormat::TarGz => {
            let decoder = flate2::read::GzDecoder::new(reader);
            let mut archive = tar::Archive::new(decoder);
            for entry in archive.entries().map_err(|e| ExtractError::Corrupt {
                message: e.to_string(),
            })? {
                let mut entry = entry.map_err(|e| ExtractError::Corrupt {
                    message: e.to_string(),
                })?;
                let raw_path = entry
                    .path()
                    .map_err(|e| ExtractError::Corrupt {
                        message: e.to_string(),
                    })?
                    .into_owned();
                let entry_type = entry.header().entry_type();

                let Some(member_rel) = relativize_member(&raw_path, subdir_normalized.as_deref())?
                else {
                    continue; // outside subdir filter
                };

                // Refuse symlinks: schema dirs are pure data; allowing
                // tarball symlinks expands the trust surface (a
                // malicious tarball could create a symlink at
                // `secret.yaml` pointing at `/etc/passwd` and the
                // schema loader would dutifully read it). Flip
                // `matched_any` AFTER this filter so SubdirNotFound
                // still fires when the only subdir-matched entries
                // are symlinks.
                if entry_type.is_symlink() || entry_type.is_hard_link() {
                    continue;
                }

                let dest_path = safe_join(&dest, &member_rel)?;

                if entry_type.is_dir() {
                    matched_any = true;
                    fs::create_dir_all(&dest_path)?;
                    continue;
                }
                matched_any = true;
                if let Some(parent) = dest_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut out = fs::File::create(&dest_path)?;
                io::copy(&mut entry, &mut out)?;
            }
        }
        ArchiveFormat::Zip => {
            reader.rewind()?;
            let mut archive = zip::ZipArchive::new(reader).map_err(|e| ExtractError::Corrupt {
                message: e.to_string(),
            })?;
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).map_err(|e| ExtractError::Corrupt {
                    message: e.to_string(),
                })?;
                let raw_path = match entry.enclosed_name() {
                    Some(p) => p.to_path_buf(),
                    None => {
                        // `enclosed_name` returns None for entries
                        // that path-escape. Surface as the explicit
                        // PathEscape error rather than silently
                        // skipping — the user's archive is malformed.
                        return Err(ExtractError::PathEscape {
                            member: entry.name().to_string(),
                        });
                    }
                };

                let Some(member_rel) = relativize_member(&raw_path, subdir_normalized.as_deref())?
                else {
                    continue;
                };

                // Zip can represent symlinks via the entry's unix
                // mode bits (`S_IFLNK == 0o120000`). Skip these for
                // the same reason we skip tarball symlinks: schema
                // dirs are pure data, allowing zip-shipped symlinks
                // expands the trust surface (`unix_mode` reads the
                // file's mode from the archive's "external
                // attributes" field, which zip optionally carries).
                if let Some(mode) = entry.unix_mode() {
                    const S_IFMT: u32 = 0o170000;
                    const S_IFLNK: u32 = 0o120000;
                    if mode & S_IFMT == S_IFLNK {
                        continue;
                    }
                }

                let dest_path = safe_join(&dest, &member_rel)?;

                if entry.is_dir() {
                    matched_any = true;
                    fs::create_dir_all(&dest_path)?;
                    continue;
                }
                matched_any = true;
                if let Some(parent) = dest_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut out = fs::File::create(&dest_path)?;
                io::copy(&mut entry, &mut out)?;
            }
        }
    }

    if !matched_any {
        return Err(ExtractError::SubdirNotFound {
            subdir: subdir.unwrap_or("").to_string(),
        });
    }
    Ok(())
}

// ---- helpers ----

/// Strip a leading slash from a `subdir = "..."` config value and
/// normalize away trailing slashes, so the prefix-match below behaves
/// the same for `subdir = "labels"`, `subdir = "/labels"`,
/// `subdir = "labels/"`, etc.
fn normalize_subdir(s: &str) -> String {
    s.trim_matches('/').to_string()
}

/// Given an archive member's path and an optional subdir filter,
/// return the member's path *relative to dest* (i.e. with the subdir
/// prefix stripped if applicable). Returns `Ok(None)` when a subdir
/// is set and the member is outside it.
///
/// Many forge tarballs (GitHub's especially) wrap the repo in a
/// single top-level `<owner>-<repo>-<sha>/` directory. Callers
/// pointing at a schema dir within that wrapper set `subdir = "..."`
/// to the desired path component; the match below finds the first
/// occurrence of that token in the member's path and keeps everything
/// after it, so the wrapper is implicitly stripped. Without a subdir
/// the wrapper is preserved verbatim in `dest`.
fn relativize_member(
    raw_path: &Path,
    subdir: Option<&str>,
) -> Result<Option<PathBuf>, ExtractError> {
    let member_str = raw_path.to_string_lossy().to_string();

    // Absolute paths are rejected outright — tar/zip can encode them
    // and the only correct response is to refuse.
    if raw_path.is_absolute() {
        return Err(ExtractError::AbsoluteMember { member: member_str });
    }

    // Any component-level path escape (`..`) is rejected before we
    // even try to join. The later `safe_join` is belt-and-suspenders.
    for component in raw_path.components() {
        match component {
            Component::ParentDir => {
                return Err(ExtractError::PathEscape { member: member_str });
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(ExtractError::AbsoluteMember { member: member_str });
            }
            _ => {}
        }
    }

    let Some(subdir) = subdir else {
        return Ok(Some(raw_path.to_path_buf()));
    };
    if subdir.is_empty() {
        return Ok(Some(raw_path.to_path_buf()));
    }

    // Match members under `<archive-root>/.../<subdir>/...`.
    // `subdir` may be either a single component (`labels`) or a
    // nested path (`src/labels`). Use a component-windowed match so
    // both cases work and we don't fold `/` into the matcher
    // implicitly. Archive paths often include a leading wrapper
    // directory (GitHub tarballs do this), so we match the first
    // occurrence of the subdir token sequence, not a fixed-prefix
    // match.
    let path_components: Vec<Component<'_>> = raw_path
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .collect();
    let subdir_path = Path::new(subdir);
    let subdir_components: Vec<Component<'_>> = subdir_path
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .collect();

    if subdir_components.is_empty() {
        return Ok(Some(raw_path.to_path_buf()));
    }

    if let Some(idx) = path_components
        .windows(subdir_components.len())
        .position(|window| window == subdir_components.as_slice())
    {
        let rel: PathBuf = path_components[idx + subdir_components.len()..]
            .iter()
            .collect();
        if rel.as_os_str().is_empty() {
            // The entry IS the subdir itself (a directory marker);
            // skip it — its children will be extracted.
            return Ok(None);
        }
        return Ok(Some(rel));
    }
    Ok(None)
}

/// Join `base` and `rel`, then verify the result is still under
/// `base`. Belt-and-suspenders on top of [`relativize_member`]'s
/// component check — `safe_join` catches anything that slipped
/// through (symlink loops, edge cases in the component analysis).
fn safe_join(base: &Path, rel: &Path) -> Result<PathBuf, ExtractError> {
    let joined = base.join(rel);
    // We don't canonicalize `joined` because the path may not exist
    // yet (we're about to create it). Instead we walk the components
    // and refuse `..` outright.
    for component in rel.components() {
        if matches!(component, Component::ParentDir) {
            return Err(ExtractError::PathEscape {
                member: rel.to_string_lossy().to_string(),
            });
        }
    }
    if !joined.starts_with(base) {
        return Err(ExtractError::PathEscape {
            member: rel.to_string_lossy().to_string(),
        });
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- detect_format ----

    #[test]
    fn detect_format_prefers_content_type_when_present() {
        assert_eq!(
            detect_format(Some("application/zip"), "foo.tar.gz"),
            ArchiveFormat::Zip
        );
        assert_eq!(
            detect_format(Some("application/gzip"), "foo.zip"),
            ArchiveFormat::TarGz
        );
    }

    #[test]
    fn detect_format_strips_charset_suffix() {
        assert_eq!(
            detect_format(Some("application/zip; charset=binary"), ""),
            ArchiveFormat::Zip
        );
    }

    #[test]
    fn detect_format_falls_back_to_extension() {
        assert_eq!(detect_format(None, "foo.zip"), ArchiveFormat::Zip);
        assert_eq!(detect_format(None, "foo.tar.gz"), ArchiveFormat::TarGz);
        assert_eq!(detect_format(None, "foo.tgz"), ArchiveFormat::TarGz);
    }

    #[test]
    fn detect_format_defaults_to_targz_for_unknown_content_type() {
        assert_eq!(
            detect_format(Some("application/octet-stream"), "weird"),
            ArchiveFormat::TarGz
        );
    }

    // ---- tarball extraction ----

    fn build_tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let gz = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = tar::Builder::new(gz);
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *contents).unwrap();
        }
        let gz = builder.into_inner().unwrap();
        let mut buf = gz.finish().unwrap();
        buf.flush().unwrap();
        buf
    }

    #[test]
    fn extract_targz_writes_flat_files() {
        let bytes = build_tar_gz(&[("foo.yaml", b"foo: bar"), ("sub/baz.yaml", b"baz: qux")]);
        let dest = tempfile::tempdir().unwrap();
        extract_archive_into(
            io::Cursor::new(&bytes),
            ArchiveFormat::TarGz,
            dest.path(),
            None,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.path().join("foo.yaml")).unwrap(),
            "foo: bar"
        );
        assert_eq!(
            std::fs::read_to_string(dest.path().join("sub/baz.yaml")).unwrap(),
            "baz: qux"
        );
    }

    #[test]
    fn extract_targz_honors_subdir_filter() {
        let bytes = build_tar_gz(&[
            ("repo-abc/README.md", b"readme"),
            ("repo-abc/labels/foo.yaml", b"foo"),
            ("repo-abc/labels/bar.yaml", b"bar"),
            ("repo-abc/other/baz.yaml", b"baz"),
        ]);
        let dest = tempfile::tempdir().unwrap();
        extract_archive_into(
            io::Cursor::new(&bytes),
            ArchiveFormat::TarGz,
            dest.path(),
            Some("labels"),
        )
        .unwrap();
        assert!(dest.path().join("foo.yaml").exists());
        assert!(dest.path().join("bar.yaml").exists());
        assert!(!dest.path().join("README.md").exists());
        assert!(!dest.path().join("baz.yaml").exists());
    }

    #[test]
    fn extract_targz_subdir_not_found_errors_cleanly() {
        let bytes = build_tar_gz(&[("repo/foo.yaml", b"foo")]);
        let dest = tempfile::tempdir().unwrap();
        let err = extract_archive_into(
            io::Cursor::new(&bytes),
            ArchiveFormat::TarGz,
            dest.path(),
            Some("missing"),
        )
        .unwrap_err();
        match err {
            ExtractError::SubdirNotFound { subdir } => assert_eq!(subdir, "missing"),
            other => panic!("expected SubdirNotFound, got: {other}"),
        }
    }

    // Direct unit tests for the path-resolution primitives. We can't
    // easily exercise path-traversal through the tarball builder
    // (the `tar` crate's high-level writer refuses to build entries
    // with `..` paths), so we test the defensive primitives directly
    // — they're the load-bearing check in either case.

    #[test]
    fn relativize_member_rejects_parent_dir_components() {
        let err = relativize_member(Path::new("../escape.yaml"), None).unwrap_err();
        assert!(matches!(err, ExtractError::PathEscape { .. }));
    }

    #[test]
    fn relativize_member_rejects_parent_dir_in_middle() {
        let err = relativize_member(Path::new("safe/../escape.yaml"), None).unwrap_err();
        assert!(matches!(err, ExtractError::PathEscape { .. }));
    }

    #[test]
    fn relativize_member_rejects_absolute_paths() {
        let err = relativize_member(Path::new("/etc/passwd"), None).unwrap_err();
        assert!(matches!(err, ExtractError::AbsoluteMember { .. }));
    }

    #[test]
    fn safe_join_refuses_escape() {
        let base = std::env::temp_dir();
        let err = safe_join(&base, Path::new("../outside.yaml")).unwrap_err();
        assert!(matches!(err, ExtractError::PathEscape { .. }));
    }

    #[test]
    fn safe_join_accepts_normal_relative_path() {
        let base = std::env::temp_dir();
        let joined = safe_join(&base, Path::new("a/b/c.yaml")).unwrap();
        assert!(joined.starts_with(&base));
    }

    #[test]
    fn extract_targz_skips_symlinks() {
        // Build a tar with a symlink entry pointing outside dest. The
        // extractor should silently skip it rather than dereferencing
        // or refusing.
        use std::io::Write;
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o644);
        builder
            .append_link(&mut header, "secret.yaml", "/etc/passwd")
            .unwrap();
        // Add one real file so the extraction has something to
        // extract.
        let mut h2 = tar::Header::new_gnu();
        h2.set_size(5);
        h2.set_mode(0o644);
        h2.set_cksum();
        builder
            .append_data(&mut h2, "real.yaml", &b"hello"[..])
            .unwrap();
        let gz = builder.into_inner().unwrap();
        let mut bytes = gz.finish().unwrap();
        bytes.flush().unwrap();

        let dest = tempfile::tempdir().unwrap();
        extract_archive_into(
            io::Cursor::new(&bytes),
            ArchiveFormat::TarGz,
            dest.path(),
            None,
        )
        .unwrap();
        assert!(!dest.path().join("secret.yaml").exists());
        assert!(dest.path().join("real.yaml").exists());
    }

    // ---- zip extraction ----

    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let cursor = io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (path, contents) in entries {
            writer.start_file(*path, opts).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn extract_zip_writes_flat_files() {
        let bytes = build_zip(&[("foo.yaml", b"foo: bar")]);
        let dest = tempfile::tempdir().unwrap();
        extract_archive_into(
            io::Cursor::new(&bytes),
            ArchiveFormat::Zip,
            dest.path(),
            None,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.path().join("foo.yaml")).unwrap(),
            "foo: bar"
        );
    }

    #[test]
    fn extract_zip_honors_subdir_filter() {
        let bytes = build_zip(&[
            ("repo/README", b"readme"),
            ("repo/labels/a.yaml", b"a"),
            ("repo/labels/b.yaml", b"b"),
        ]);
        let dest = tempfile::tempdir().unwrap();
        extract_archive_into(
            io::Cursor::new(&bytes),
            ArchiveFormat::Zip,
            dest.path(),
            Some("labels"),
        )
        .unwrap();
        assert!(dest.path().join("a.yaml").exists());
        assert!(dest.path().join("b.yaml").exists());
        assert!(!dest.path().join("README").exists());
    }

    // ---- nested subdir matching ----

    #[test]
    fn extract_targz_honors_nested_subdir_filter() {
        // `subdir = "src/labels"` — nested path. The component-windowed
        // match should find the two-component sequence inside paths
        // like `repo/src/labels/foo.yaml` and strip everything up to
        // and including it.
        let bytes = build_tar_gz(&[
            ("repo-abc/README", b"readme"),
            ("repo-abc/src/labels/a.yaml", b"a"),
            ("repo-abc/src/labels/nested/b.yaml", b"b"),
            ("repo-abc/src/other/c.yaml", b"c"),
            ("repo-abc/labels/d.yaml", b"d"), // bare `labels` shouldn't match
        ]);
        let dest = tempfile::tempdir().unwrap();
        extract_archive_into(
            io::Cursor::new(&bytes),
            ArchiveFormat::TarGz,
            dest.path(),
            Some("src/labels"),
        )
        .unwrap();
        assert!(dest.path().join("a.yaml").exists());
        assert!(dest.path().join("nested/b.yaml").exists());
        assert!(!dest.path().join("c.yaml").exists());
        assert!(!dest.path().join("d.yaml").exists());
        assert!(!dest.path().join("README").exists());
    }
}
