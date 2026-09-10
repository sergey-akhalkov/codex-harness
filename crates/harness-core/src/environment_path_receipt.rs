//! A durable witness for a PATH operation, committed in the same KTM transaction.
//! The installer must serialize operations and retain its immutable intent until
//! recovery/finish has handled these receipts. Equal values alone prove no write.
#![cfg_attr(not(test), allow(dead_code))]

use super::*;
use crate::{
    build_identity::hash_bytes,
    config_file::ConfigSnapshot,
    installation_state::normal,
    registration_native::{FileGuard, LinkIdentity, StagedFile},
};
use std::path::PathBuf;

const SCHEMA: u32 = 1;
const MAX_RECEIPT: usize = 65536;
const APPLIED: &str = "path-applied.json";
const UNDONE: &str = "path-undone.json";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Witness {
    path: PathBuf,
    identity: LinkIdentity,
    sha256: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    intent: Witness,
    registry: String,
    change_sha256: String,
}

// No Debug: the private receipt identifies the current user and machine paths.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: u32,
    path: PathBuf,
    identity: LinkIdentity,
    scope: Scope,
    applied: Option<Witness>,
    checksum: String,
}

impl Document {
    fn checksum(&self) -> io::Result<String> {
        Ok(hash_bytes(&serde_json::to_vec(&(
            self.schema,
            &self.path,
            &self.identity,
            &self.scope,
            &self.applied,
        ))?))
    }
}

struct Receipt {
    _guard: FileGuard,
    witness: Witness,
}

fn registration_scope(
    path: &Path,
    fingerprint: (LinkIdentity, String),
    change: &UserPathChange,
) -> io::Result<Scope> {
    let mut current_user = null_mut();
    status(unsafe { RegOpenCurrentUser(KEY_QUERY_VALUE, &mut current_user) })?;
    let root = Key(current_user);
    Ok(Scope {
        intent: Witness {
            path: normal(path)?,
            identity: fingerprint.0,
            sha256: fingerprint.1,
        },
        registry: format!("{}\\{}", root.name()?, registration_subkey()),
        change_sha256: hash_bytes(&serde_json::to_vec(change)?),
    })
}

/// Only for a verified durable registration commitment. Its embedded original
/// intent establishes the scope even after journal.json has been retired. This
/// function grants no PATH write and never invents a missing receipt.
pub(crate) fn committed_registration_receipts(
    path: &Path,
    bytes: &[u8],
    identity: &LinkIdentity,
    change: &UserPathChange,
) -> io::Result<Vec<FileGuard>> {
    let fingerprint = (identity.clone(), hash_bytes(bytes));
    let prefix = hash_bytes(&serde_json::to_vec(&fingerprint)?);
    let scope = registration_scope(path, fingerprint, change)?;
    let parent = path.parent().ok_or_else(conflict)?;
    let applied = open(
        &parent.join(format!("path-{prefix}-applied.json")),
        &scope,
        None,
    )?;
    let undone = open(
        &parent.join(format!("path-{prefix}-undone.json")),
        &scope,
        applied.as_ref().map(|r| &r.witness),
    )?;
    if undone.is_some() {
        return Err(conflict());
    }
    Ok(applied.into_iter().map(|receipt| receipt._guard).collect())
}

fn conflict() -> io::Error {
    invalid("PATH receipt conflicts with the recorded operation; preserving recovery state")
}

fn open(path: &Path, scope: &Scope, applied: Option<&Witness>) -> io::Result<Option<Receipt>> {
    let (guard, bytes) = match FileGuard::read_regular(path) {
        Ok(value) => value,
        Err(error) if matches!(error.raw_os_error(), Some(2 | 3)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if bytes.len() > MAX_RECEIPT {
        return Err(conflict());
    }
    let document: Document = serde_json::from_slice(&bytes).map_err(|_| conflict())?;
    let identity = guard.object_identity()?;
    if document.schema != SCHEMA
        || document.path != path
        || document.identity != identity
        || &document.scope != scope
        || document.applied.as_ref() != applied
        || document.checksum != document.checksum()?
    {
        return Err(conflict());
    }
    Ok(Some(Receipt {
        _guard: guard,
        witness: Witness {
            path: path.to_owned(),
            identity,
            sha256: hash_bytes(&bytes),
        },
    }))
}

/// The file snapshot must be the installer's persisted immutable operation
/// intent, including the exact serialized UserPathChange. It remains guarded
/// while a receipt is read or published. Receipt cleanup belongs to that
/// lifecycle, after its final recovery decision; this module never deletes it.
enum Intent<'a> {
    Snapshot(&'a ConfigSnapshot),
    Held {
        path: &'a Path,
        bytes: &'a [u8],
        guard: &'a FileGuard,
    },
}

impl Intent<'_> {
    fn path(&self) -> &Path {
        match self {
            Self::Snapshot(s) => s.path(),
            Self::Held { path, .. } => path,
        }
    }
    fn guard(&self) -> io::Result<Option<FileGuard>> {
        match self {
            Self::Snapshot(s) => s.guard().map(Some),
            Self::Held { .. } => Ok(None),
        }
    }
    fn fingerprint(&self) -> io::Result<(LinkIdentity, String)> {
        match self {
            Self::Snapshot(s) => Ok(s.fingerprint()),
            Self::Held { bytes, guard, .. } => Ok((guard.object_identity()?, hash_bytes(bytes))),
        }
    }
}

pub(crate) struct PathReceipts<'a> {
    intent: Intent<'a>,
    change: &'a UserPathChange,
    registration: bool,
}

impl<'a> PathReceipts<'a> {
    pub(crate) fn new(intent: &'a ConfigSnapshot, change: &'a UserPathChange) -> Self {
        Self {
            intent: Intent::Snapshot(intent),
            change,
            registration: false,
        }
    }

    /// The caller retains the exact journal guard and the bytes read/written
    /// under that guard; a second native delete-capable open would conflict.
    pub(crate) fn held_registration(
        path: &'a Path,
        bytes: &'a [u8],
        guard: &'a FileGuard,
        change: &'a UserPathChange,
    ) -> Self {
        Self {
            intent: Intent::Held { path, bytes, guard },
            change,
            registration: true,
        }
    }

    fn paths(&self, parent: &Path) -> io::Result<(PathBuf, PathBuf)> {
        Ok(if self.registration {
            // Retained private receipts cannot collide with the next operation
            // that reuses journal.json. Its exact bytes and ID remain in Scope.
            let prefix = hash_bytes(&serde_json::to_vec(&self.intent.fingerprint()?)?);
            (
                parent.join(format!("path-{prefix}-applied.json")),
                parent.join(format!("path-{prefix}-undone.json")),
            )
        } else {
            (parent.join(APPLIED), parent.join(UNDONE))
        })
    }

    pub(crate) fn apply_registration(&self) -> io::Result<bool> {
        self.exchange(&registration_subkey(), false, |_| Ok(()))
    }

    pub(crate) fn undo_registration(&self) -> io::Result<bool> {
        let applied = self
            .paths(self.intent.path().parent().ok_or_else(conflict)?)?
            .0;
        match std::fs::symlink_metadata(applied) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // No receipt means this operation never wrote PATH. Checking
                // both absence and the original value grants no write authority.
                self.observe_registration(false)?;
                Ok(false)
            }
            Err(error) => Err(error),
            Ok(_) => self.exchange(&registration_subkey(), true, |_| Ok(())),
        }
    }

    pub(crate) fn observe_registration(&self, published: bool) -> io::Result<()> {
        self.observe_registration_state(Some(published))
    }

    pub(crate) fn observe_undo_registration(&self) -> io::Result<()> {
        self.observe_registration_state(None)
    }

    fn observe_registration_state(&self, published: Option<bool>) -> io::Result<()> {
        let (_intent, applied, undone) = self.hold_registration_receipts()?;
        if published == Some(true) && (applied.is_none() || undone.is_some())
            || published == Some(false) && (applied.is_some() || undone.is_some())
            || undone.is_some() && applied.is_none()
        {
            return Err(conflict());
        }
        let expected = if published.unwrap_or(applied.is_some() && undone.is_none()) {
            &self.change.after
        } else {
            &self.change.before
        };
        if &UserPathSnapshot::read_at(&registration_subkey())?.value != expected {
            return Err(conflict());
        }
        Ok(())
    }

    /// Exact receipt objects after a finished inverse. The caller must retire
    /// them atomically with their held intent; deleting either alone loses the
    /// evidence needed by a retry after interruption.
    pub(crate) fn cleanup_undo_registration(&self) -> io::Result<Vec<FileGuard>> {
        let (_intent, applied, undone) = self.hold_registration_receipts()?;
        if applied.is_some() != undone.is_some()
            || UserPathSnapshot::read_at(&registration_subkey())?.value != self.change.before
        {
            return Err(conflict());
        }
        Ok([applied, undone]
            .into_iter()
            .flatten()
            .map(|receipt| receipt._guard)
            .collect())
    }

    fn hold_registration_receipts(
        &self,
    ) -> io::Result<(Option<FileGuard>, Option<Receipt>, Option<Receipt>)> {
        let _intent = self.intent.guard()?;
        let parent = self.intent.path().parent().ok_or_else(conflict)?;
        let (applied_path, undone_path) = self.paths(parent)?;
        let scope =
            registration_scope(self.intent.path(), self.intent.fingerprint()?, self.change)?;
        let applied = open(&applied_path, &scope, None)?;
        let undone = open(&undone_path, &scope, applied.as_ref().map(|r| &r.witness))?;
        Ok((_intent, applied, undone))
    }

    #[cfg_attr(test, allow(dead_code))] // Unit tests use an owned registry leaf.
    pub(crate) fn publish(&self) -> io::Result<bool> {
        self.exchange(ENVIRONMENT, false, |_| Ok(()))
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn rollback(&self) -> io::Result<bool> {
        self.exchange(ENVIRONMENT, true, |_| Ok(()))
    }

    pub(super) fn exchange(
        &self,
        subkey: &str,
        undo: bool,
        mut checkpoint: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<bool> {
        let _intent = self.intent.guard()?;
        let intent_path = normal(self.intent.path())?;
        let parent = intent_path.parent().ok_or_else(conflict)?;
        let (applied_path, undone_path) = self.paths(parent)?;
        if intent_path == applied_path || intent_path == undone_path {
            return Err(conflict());
        }
        let (identity, sha256) = self.intent.fingerprint()?;
        let mut current_user = null_mut();
        status(unsafe { RegOpenCurrentUser(KEY_QUERY_VALUE, &mut current_user) })?;
        let root = Key(current_user);
        let scope = Scope {
            intent: Witness {
                path: intent_path,
                identity,
                sha256,
            },
            registry: format!("{}\\{subkey}", root.name()?),
            change_sha256: hash_bytes(&serde_json::to_vec(self.change)?),
        };
        drop(root);
        let applied = open(&applied_path, &scope, None)?;
        let undone = open(&undone_path, &scope, applied.as_ref().map(|r| &r.witness))?;
        if !undo && undone.is_some() || undo && applied.is_none() {
            return Err(conflict());
        }
        if (!undo && applied.is_some()) || (undo && undone.is_some()) {
            let desired = if undo {
                &self.change.before
            } else {
                &self.change.after
            };
            if &UserPathSnapshot::read_at(subkey)?.value != desired {
                return Err(conflict());
            }
            return Ok(false);
        }
        let path = if undo { undone_path } else { applied_path };
        let mut staged = StagedFile::create(&path, b"")?;
        let changed = self
            .change
            .stage_in(subkey, undo, true, staged.transaction(), |phase| {
                checkpoint(phase)
            })?;
        let mut document = Document {
            schema: SCHEMA,
            path,
            identity: staged.identity(),
            scope,
            applied: if undo {
                applied.as_ref().map(|r| r.witness.clone())
            } else {
                None
            },
            checksum: String::new(),
        };
        document.checksum = document.checksum()?;
        let bytes = serde_json::to_vec(&document)?;
        if bytes.len() > MAX_RECEIPT {
            return Err(conflict());
        }
        staged.set_bytes(&bytes)?;
        checkpoint("receipt-staged")?;
        staged.commit()?;
        checkpoint("receipt-committed")?;
        Ok(changed)
    }
}

pub(super) fn registration_subkey() -> String {
    #[cfg(test)]
    if let Some(key) = REGISTRATION_TEST_KEY.with(|key| key.borrow().clone()) {
        return key;
    }
    ENVIRONMENT.to_owned()
}

#[cfg(test)]
thread_local! {
    pub(super) static REGISTRATION_TEST_KEY: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}
