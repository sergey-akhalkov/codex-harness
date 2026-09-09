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
pub(crate) struct PathReceipts<'a> {
    intent: &'a ConfigSnapshot,
    change: &'a UserPathChange,
}

impl<'a> PathReceipts<'a> {
    pub(crate) fn new(intent: &'a ConfigSnapshot, change: &'a UserPathChange) -> Self {
        Self { intent, change }
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
        let applied_path = parent.join(APPLIED);
        let undone_path = parent.join(UNDONE);
        if intent_path == applied_path || intent_path == undone_path {
            return Err(conflict());
        }
        let (identity, sha256) = self.intent.fingerprint();
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
