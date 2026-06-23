//! Pluggable source-text loaders.
//!
//! The resolver never touches the filesystem directly: all I/O goes
//! through the [`Loader`] trait, which keeps the resolver pure and usable
//! in WASM, sandboxes, and unit tests. Of all of lex-core, only
//! [`FsLoader`] references `std::fs`. [`MemoryLoader`] (gated behind the
//! `test-support` cargo feature) backs tests with an in-memory map.

use super::errors::LoadError;
use std::path::{Path, PathBuf};

/// A pluggable source-text loader.
///
/// Implementations decide where bytes come from (filesystem, in-memory map,
/// virtual filesystem, content-addressed store, …). lex-core never references
/// `std::fs` directly through this trait; that keeps the resolver pure and
/// usable in WASM, sandboxes, and unit tests.
pub trait Loader {
    /// Load the source text for `path` and return both the contents and a
    /// canonical identity for the loaded resource. The path is what the
    /// resolver decided on after applying the rules in §4 of the proposal.
    ///
    /// `LoadedFile::canonical_path` is the loader's authoritative identity
    /// for the resource. For [`FsLoader`] this is the filesystem-canonical
    /// path (symlinks resolved, case-folded if the underlying FS is
    /// case-insensitive); for [`MemoryLoader`] it's the lookup key (since
    /// memory loaders have no symlinks). The resolver uses this for cycle
    /// detection and for stamping `Range.origin_path` on the loaded tree.
    fn load(&self, path: &Path) -> Result<LoadedFile, LoadError>;
}

/// Result of a successful [`Loader::load`].
#[derive(Debug, Clone)]
pub struct LoadedFile {
    /// The file's source text.
    pub source: String,
    /// The loader's authoritative identity for the resource. See
    /// [`Loader::load`] for how loaders decide this.
    pub canonical_path: PathBuf,
}

// ============================================================================
// Filesystem-backed loader
// ============================================================================

/// [`Loader`] that reads files from the filesystem with `std::fs::read_to_string`.
///
/// This is the production loader used by the CLI; the LSP wraps it with a
/// file-watch invalidation layer in PR 8. lex-core's *resolver* code does not
/// reference `std::fs` — `FsLoader` is the one place where it does, isolated
/// behind the [`Loader`] trait so the rest of the crate stays sandbox- and
/// WASM-friendly.
///
/// `FsLoader` is constructed with the resolution root and rechecks every
/// load against it post-`fs::canonicalize`, so a symlink pointing outside
/// the root is rejected even though the lexical-only check in
/// [`resolve_path`](super::resolve_path) cannot see it. Also rejects
/// non-regular files (devices, FIFOs, directories) before reading, so the
/// loader can't be tricked into blocking on `/dev/zero` or allocating
/// against an open device.
///
/// Errors map:
/// - canonicalization fails (file missing, permission denied at a parent,
///   broken symlink, …) → [`LoadError::NotFound`]
/// - canonical path doesn't sit under canonical root → [`LoadError::OutsideRoot`]
/// - target is not a regular file → [`LoadError::Io`] with a clear message
/// - any other I/O error during read → [`LoadError::Io`]
pub struct FsLoader {
    /// Filesystem-canonical resolution root. Constructed once at
    /// `FsLoader::new`; if canonicalization fails (e.g., the configured
    /// root doesn't exist on disk), we fall back to the input verbatim
    /// and the bounds check will simply never pass — visible to the user
    /// as a `LoadError::OutsideRoot` instead of silently disabling the
    /// security check.
    canonical_root: PathBuf,
    /// Per-file size cap (bytes). Loads of larger files surface as
    /// `LoadError::TooLarge` before any bytes are read into memory.
    /// Default [`FsLoader::DEFAULT_MAX_FILE_SIZE`].
    max_file_size: u64,
}

impl FsLoader {
    /// Default per-file size cap: 10 MiB. Generous for realistic Lex
    /// source documents (text only) and tight enough to bound memory
    /// allocation per include against an adversarial 1 GB file.
    pub const DEFAULT_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

    /// Construct a loader rooted at `root` with default size limits.
    /// The loader stores `root`'s fs-canonical form (with symlinks
    /// resolved); subsequent loads validate that the requested path's
    /// canonical form lives under it.
    pub fn new(root: PathBuf) -> Self {
        let canonical_root = std::fs::canonicalize(&root).unwrap_or(root);
        Self {
            canonical_root,
            max_file_size: Self::DEFAULT_MAX_FILE_SIZE,
        }
    }

    /// Override the default per-file size cap (bytes). Use to widen the
    /// limit for projects with genuinely large source files, or tighten
    /// it for stricter sandboxes (e.g., LSPs serving untrusted content).
    pub fn with_max_file_size(mut self, max_file_size: u64) -> Self {
        self.max_file_size = max_file_size;
        self
    }
}

impl Loader for FsLoader {
    fn load(&self, path: &Path) -> Result<LoadedFile, LoadError> {
        // 1. Canonicalize. Resolves symlinks and `..` segments against the
        //    real filesystem. NotFound / broken-symlink / permission errors
        //    all surface here.
        let canonical_path = std::fs::canonicalize(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => LoadError::NotFound {
                path: path.to_path_buf(),
            },
            _ => LoadError::Io {
                path: path.to_path_buf(),
                message: e.to_string(),
            },
        })?;

        // 2. Bounds check against the *canonical* root. This is the
        //    actual security gate against symlink traversal — the lexical
        //    check in resolve_path can't see through symlinks.
        if !canonical_path.starts_with(&self.canonical_root) {
            return Err(LoadError::OutsideRoot {
                path: canonical_path,
                root: self.canonical_root.clone(),
            });
        }

        // 3. Reject non-regular files. Without this, an attacker (with
        //    write access to the repo) could symlink an include target to
        //    `/dev/zero` or a FIFO and block / OOM the reader. The
        //    is_file() metadata call is a cheap sanity check.
        let meta = std::fs::metadata(&canonical_path).map_err(|e| LoadError::Io {
            path: canonical_path.clone(),
            message: e.to_string(),
        })?;
        if !meta.is_file() {
            return Err(LoadError::Io {
                path: canonical_path,
                message: "include target is not a regular file".to_string(),
            });
        }

        // 4. Size cap. Bounds memory allocation per include against an
        //    adversarial 1 GB file before any bytes hit the heap.
        let size = meta.len();
        if size > self.max_file_size {
            return Err(LoadError::TooLarge {
                path: canonical_path,
                size,
                limit: self.max_file_size,
            });
        }

        // 5. Read. By this point we know the path is a regular file under
        //    the canonical root and within the size cap; anything that
        //    fails here is a real I/O error worth surfacing.
        let source = std::fs::read_to_string(&canonical_path).map_err(|e| LoadError::Io {
            path: canonical_path.clone(),
            message: e.to_string(),
        })?;

        Ok(LoadedFile {
            source,
            canonical_path,
        })
    }
}

// ============================================================================
// Test fixtures (test-support feature + cfg(test))
// ============================================================================

/// In-memory [`Loader`] backed by a `HashMap<PathBuf, String>`.
#[cfg(any(test, feature = "test-support"))]
pub struct MemoryLoader {
    files: std::collections::HashMap<PathBuf, String>,
}

#[cfg(any(test, feature = "test-support"))]
impl MemoryLoader {
    /// Create an empty loader. Add files with [`MemoryLoader::insert`].
    pub fn new() -> Self {
        Self {
            files: std::collections::HashMap::new(),
        }
    }

    /// Register a file at `path` with the given source text.
    pub fn insert<P: Into<PathBuf>, S: Into<String>>(&mut self, path: P, contents: S) -> &mut Self {
        self.files.insert(path.into(), contents.into());
        self
    }

    /// Convenience constructor: build a loader from any iterator of
    /// `(path, contents)` pairs.
    pub fn from_pairs<I, P, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (P, S)>,
        P: Into<PathBuf>,
        S: Into<String>,
    {
        let mut loader = Self::new();
        for (path, contents) in pairs {
            loader.insert(path, contents);
        }
        loader
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Default for MemoryLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Loader for MemoryLoader {
    fn load(&self, path: &Path) -> Result<LoadedFile, LoadError> {
        // Memory loaders have no symlinks; the lookup key *is* the
        // canonical identity. Cycle detection in the resolver compares
        // `LoadedFile::canonical_path` values; for tests this matches the
        // lexically-normalized paths the resolver already produces.
        let source = self
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| LoadError::NotFound {
                path: path.to_path_buf(),
            })?;
        Ok(LoadedFile {
            source,
            canonical_path: path.to_path_buf(),
        })
    }
}
