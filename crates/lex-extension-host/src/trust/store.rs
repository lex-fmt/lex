//! Persistent trust store for LSP-mode handler decisions.
//!
//! Modeled on VS Code's workspace-trust pattern: trust decisions are
//! workspace-scoped and stored in `<workspace>/.lex/trust.json`.
//! Each entry pins to a `(namespace, command_string)` tuple so that
//! changing the schema's `handler.command` (e.g., a version bump
//! that adds a new flag) triggers a fresh prompt instead of silently
//! reusing the old approval.
//!
//! # File format
//!
//! ```json
//! {
//!   "version": 1,
//!   "entries": [
//!     {
//!       "namespace": "acme",
//!       "command_string": "acme-handler --workspace=/foo",
//!       "decision": "trusted"
//!     },
//!     {
//!       "namespace": "evil",
//!       "command_string": "evil-binary",
//!       "decision": {"denied": "user rejected"}
//!     }
//!   ]
//! }
//! ```
//!
//! `version: 1` is the schema-format version. Future changes that
//! aren't backwards-readable bump it and the loader returns
//! [`TrustStoreError::UnsupportedVersion`].

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::decision::TrustDecision;

/// Errors raised by the trust store. Read paths are best-effort —
/// missing files yield an empty store (callers see `None` from
/// `get`); only malformed-but-present files fail loudly.
#[derive(Debug)]
pub enum TrustStoreError {
    /// Reading or writing `.lex/trust.json` failed at the OS level.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The file body was not valid JSON or didn't deserialise into
    /// the expected shape.
    Parse { path: PathBuf, message: String },
    /// `version` field was set to a value newer than the loader
    /// understands. The store stays empty and the caller is told
    /// which version was unexpected so the user can either upgrade
    /// the host or delete the file.
    UnsupportedVersion { path: PathBuf, version: u32 },
}

impl std::fmt::Display for TrustStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustStoreError::Io { path, source } => {
                write!(f, "{}: trust store io error: {source}", path.display())
            }
            TrustStoreError::Parse { path, message } => {
                write!(f, "{}: trust store parse error: {message}", path.display())
            }
            TrustStoreError::UnsupportedVersion { path, version } => write!(
                f,
                "{}: trust store version {version} is newer than this host supports (1)",
                path.display()
            ),
        }
    }
}

impl std::error::Error for TrustStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TrustStoreError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// What the store keys on. The `(namespace, command_string)` tuple
/// gives pin granularity — a different `command_string` means a new
/// prompt, even if the namespace is the same.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrustKey {
    pub namespace: String,
    pub command_string: String,
}

/// In-memory + on-disk trust store. Construct via [`TrustStore::open`]
/// pointing at a workspace root; the store reads
/// `<workspace>/.lex/trust.json` on construction (missing file →
/// empty store) and writes back on every [`set`](Self::set) call.
#[derive(Debug)]
pub struct TrustStore {
    path: PathBuf,
    entries: HashMap<TrustKey, TrustDecision>,
}

impl TrustStore {
    /// Open (or create) the trust store for `workspace`. The actual
    /// JSON file lives at `<workspace>/.lex/trust.json`. Missing
    /// file or missing `.lex/` directory produces an empty store —
    /// hosts can `set` into it and the directory is created on first
    /// flush.
    pub fn open(workspace: impl AsRef<Path>) -> Result<Self, TrustStoreError> {
        let path = workspace.as_ref().join(".lex").join("trust.json");
        let entries = match fs::read_to_string(&path) {
            Ok(body) => parse_disk_format(&body, &path)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(source) => {
                return Err(TrustStoreError::Io {
                    path: path.clone(),
                    source,
                });
            }
        };
        Ok(Self { path, entries })
    }

    /// Look up a pinned decision. `None` means the gate must prompt;
    /// `Some` short-circuits the prompt.
    pub fn get(&self, key: &TrustKey) -> Option<&TrustDecision> {
        self.entries.get(key)
    }

    /// Pin a decision. Both `Trusted` and `Denied` are persisted —
    /// `Pending` is not a stored state. Writes to disk immediately
    /// so a crash in the middle of a session doesn't drop the
    /// approval the user just gave.
    pub fn set(&mut self, key: TrustKey, decision: TrustDecision) -> Result<(), TrustStoreError> {
        // Reject Pending — it's a programmer error to persist.
        if matches!(decision, TrustDecision::Pending) {
            return Ok(());
        }
        self.entries.insert(key, decision);
        self.flush()
    }

    /// Drop all pinned decisions. Used by editor commands like
    /// "Reset Lex extension trust for this workspace".
    pub fn clear(&mut self) -> Result<(), TrustStoreError> {
        self.entries.clear();
        self.flush()
    }

    /// Iterate the (key, decision) pairs in arbitrary order. The
    /// editor UI uses this to render "currently trusted namespaces".
    pub fn iter(&self) -> impl Iterator<Item = (&TrustKey, &TrustDecision)> {
        self.entries.iter()
    }

    /// Number of pinned decisions. Tests use this; the editor UI
    /// might too.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn flush(&self) -> Result<(), TrustStoreError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| TrustStoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let body = serialise_disk_format(&self.entries);
        fs::write(&self.path, body).map_err(|source| TrustStoreError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

/// On-disk JSON shape. Versioned with a top-level `version` field so
/// we can evolve the format without losing old stores.
#[derive(Debug, Serialize, Deserialize)]
struct OnDiskFile {
    version: u32,
    entries: Vec<OnDiskEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OnDiskEntry {
    namespace: String,
    command_string: String,
    decision: OnDiskDecision,
}

/// On-disk decision shape. Mirrors [`TrustDecision`] minus
/// `Pending` (which we never persist).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum OnDiskDecision {
    Trusted,
    Denied { reason: String },
}

fn parse_disk_format(
    body: &str,
    path: &Path,
) -> Result<HashMap<TrustKey, TrustDecision>, TrustStoreError> {
    let parsed: OnDiskFile = serde_json::from_str(body).map_err(|err| TrustStoreError::Parse {
        path: path.to_path_buf(),
        message: err.to_string(),
    })?;
    if parsed.version != 1 {
        return Err(TrustStoreError::UnsupportedVersion {
            path: path.to_path_buf(),
            version: parsed.version,
        });
    }
    let mut out = HashMap::with_capacity(parsed.entries.len());
    for entry in parsed.entries {
        let key = TrustKey {
            namespace: entry.namespace,
            command_string: entry.command_string,
        };
        let decision = match entry.decision {
            OnDiskDecision::Trusted => TrustDecision::Trusted,
            OnDiskDecision::Denied { reason } => TrustDecision::Denied { reason },
        };
        out.insert(key, decision);
    }
    Ok(out)
}

fn serialise_disk_format(entries: &HashMap<TrustKey, TrustDecision>) -> String {
    let mut on_disk: Vec<OnDiskEntry> = entries
        .iter()
        .filter_map(|(k, v)| {
            let decision = match v {
                TrustDecision::Trusted => OnDiskDecision::Trusted,
                TrustDecision::Denied { reason } => OnDiskDecision::Denied {
                    reason: reason.clone(),
                },
                TrustDecision::Pending => return None,
            };
            Some(OnDiskEntry {
                namespace: k.namespace.clone(),
                command_string: k.command_string.clone(),
                decision,
            })
        })
        .collect();
    // Stable order for deterministic file content (helps debugging
    // and version-control diffs of `.lex/trust.json`).
    on_disk.sort_by(|a, b| {
        a.namespace
            .cmp(&b.namespace)
            .then(a.command_string.cmp(&b.command_string))
    });
    let file = OnDiskFile {
        version: 1,
        entries: on_disk,
    };
    serde_json::to_string_pretty(&file).expect("OnDiskFile serialises")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ns: &str, cmd: &str) -> TrustKey {
        TrustKey {
            namespace: ns.into(),
            command_string: cmd.into(),
        }
    }

    #[test]
    fn missing_file_yields_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = TrustStore::open(dir.path()).expect("open empty");
        assert!(store.is_empty());
    }

    #[test]
    fn set_persists_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = TrustStore::open(dir.path()).unwrap();
            store
                .set(key("acme", "acme-handler"), TrustDecision::Trusted)
                .unwrap();
            store
                .set(
                    key("evil", "evil-binary"),
                    TrustDecision::Denied {
                        reason: "rejected".into(),
                    },
                )
                .unwrap();
        }
        let store = TrustStore::open(dir.path()).expect("reopen");
        assert_eq!(store.len(), 2);
        assert_eq!(
            store.get(&key("acme", "acme-handler")),
            Some(&TrustDecision::Trusted)
        );
        match store.get(&key("evil", "evil-binary")) {
            Some(TrustDecision::Denied { reason }) => assert_eq!(reason, "rejected"),
            other => panic!("expected Denied, got: {other:?}"),
        }
    }

    #[test]
    fn pending_is_not_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::open(dir.path()).unwrap();
        store
            .set(key("acme", "acme-handler"), TrustDecision::Pending)
            .unwrap();
        assert!(store.is_empty());
        // And the file shouldn't carry it back either.
        let store = TrustStore::open(dir.path()).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn clear_wipes_all_entries_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = TrustStore::open(dir.path()).unwrap();
            store.set(key("acme", "x"), TrustDecision::Trusted).unwrap();
            store.clear().unwrap();
            assert!(store.is_empty());
        }
        let store = TrustStore::open(dir.path()).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn iter_yields_every_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::open(dir.path()).unwrap();
        store.set(key("a", "1"), TrustDecision::Trusted).unwrap();
        store
            .set(
                key("b", "2"),
                TrustDecision::Denied {
                    reason: "no".into(),
                },
            )
            .unwrap();
        let mut seen: Vec<String> = store.iter().map(|(k, _)| k.namespace.clone()).collect();
        seen.sort();
        assert_eq!(seen, vec!["a", "b"]);
    }

    #[test]
    fn unsupported_version_yields_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let trust_path = dir.path().join(".lex/trust.json");
        std::fs::create_dir_all(trust_path.parent().unwrap()).unwrap();
        std::fs::write(&trust_path, r#"{"version": 99, "entries": []}"#).unwrap();
        let err = TrustStore::open(dir.path()).unwrap_err();
        match err {
            TrustStoreError::UnsupportedVersion { version, .. } => assert_eq!(version, 99),
            other => panic!("expected UnsupportedVersion, got: {other}"),
        }
    }

    #[test]
    fn malformed_json_yields_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let trust_path = dir.path().join(".lex/trust.json");
        std::fs::create_dir_all(trust_path.parent().unwrap()).unwrap();
        std::fs::write(&trust_path, "{not valid json").unwrap();
        let err = TrustStore::open(dir.path()).unwrap_err();
        assert!(matches!(err, TrustStoreError::Parse { .. }));
    }

    #[test]
    fn disk_format_is_pretty_and_sorted() {
        // The on-disk file should be human-readable and stable
        // (deterministic order) so version-control diffs of
        // `.lex/trust.json` are clean.
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = TrustStore::open(dir.path()).unwrap();
            store.set(key("zeta", "z"), TrustDecision::Trusted).unwrap();
            store
                .set(key("alpha", "a"), TrustDecision::Trusted)
                .unwrap();
        }
        let body = std::fs::read_to_string(dir.path().join(".lex/trust.json")).unwrap();
        // Pretty-printed → contains newlines and indentation.
        assert!(body.contains('\n'));
        // Alpha comes before zeta in the file.
        assert!(body.find("alpha").unwrap() < body.find("zeta").unwrap());
        // Schema version present.
        assert!(body.contains("\"version\": 1"));
    }
}
