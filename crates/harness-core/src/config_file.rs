//! Existing configuration snapshots for native installation. Publication checks
//! both the original object identity and exact bytes inside a short transaction.
//! This is one-file atomicity; a multi-step installer still needs its journal.
#![cfg(windows)]

use crate::registration_native::{FileGuard, LinkIdentity, MAX_REGULAR_BYTES};
use serde::{Deserialize, Serialize};
use std::{fmt, io, path::Path, path::PathBuf};

pub struct ConfigSnapshot {
    path: PathBuf,
    identity: LinkIdentity,
    bytes: Vec<u8>,
}

/// An explicit proposed change. Its private record can be journaled by native
/// registration; neither Debug nor the ordinary reports expose config bytes.
pub struct ConfigChange {
    pub(crate) record: ConfigRecord,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigRecord {
    pub(crate) path: PathBuf,
    identity: LinkIdentity,
    before: Vec<u8>,
    after: Vec<u8>,
}

impl ConfigRecord {
    pub(crate) fn baseline_sha256(&self) -> String {
        crate::build_identity::hash_bytes(&self.before)
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.before.len() > MAX_REGULAR_BYTES || self.after.len() > MAX_REGULAR_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Configuration exceeds 16 MiB.",
            ));
        }
        Ok(())
    }

    fn read_current(&self) -> io::Result<(FileGuard, Vec<u8>)> {
        self.validate()?;
        let (guard, bytes) = FileGuard::read_regular(&self.path)?;
        if guard.object_identity()? != self.identity {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Configuration object identity changed; preserving it.",
            ));
        }
        Ok((guard, bytes))
    }

    pub(crate) fn check_before(&self) -> io::Result<()> {
        self.hold_before().map(drop)
    }

    pub(crate) fn hold_before(&self) -> io::Result<FileGuard> {
        let (guard, bytes) = self.read_current()?;
        if bytes != self.before {
            return Err(changed());
        }
        Ok(guard)
    }

    pub(crate) fn set_candidate(&mut self, bytes: Vec<u8>) -> io::Result<()> {
        if bytes.len() > MAX_REGULAR_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Configuration exceeds 16 MiB.",
            ));
        }
        self.after = bytes;
        Ok(())
    }

    pub(crate) fn check_published(&self) -> io::Result<()> {
        self.hold_published().map(drop)
    }

    pub(crate) fn hold_published(&self) -> io::Result<FileGuard> {
        let (guard, bytes) = self.read_current()?;
        if bytes != self.after {
            return Err(changed());
        }
        Ok(guard)
    }

    pub(crate) fn published_identity(&self) -> &LinkIdentity {
        &self.identity
    }

    pub(crate) fn published_bytes(&self) -> &[u8] {
        &self.after
    }

    pub(crate) fn check_undo(&self) -> io::Result<()> {
        let (_guard, bytes) = self.read_current()?;
        if bytes != self.before && bytes != self.after {
            return Err(changed());
        }
        Ok(())
    }

    pub(crate) fn same_candidate(&self, other: &Self) -> bool {
        self.path == other.path && self.identity == other.identity && self.after == other.after
    }

    pub(crate) fn same_object(&self, other: &Self) -> bool {
        self.identity == other.identity
    }

    pub(crate) fn publish(&self) -> io::Result<ConfigSnapshot> {
        FileGuard::replace_regular(&self.path, &self.identity, &self.before, &self.after)?;
        Ok(ConfigSnapshot {
            path: self.path.clone(),
            identity: self.identity.clone(),
            bytes: self.after.clone(),
        })
    }

    pub(crate) fn undo(&self) -> io::Result<bool> {
        let (guard, bytes) = self.read_current()?;
        if bytes == self.before {
            return Ok(false);
        }
        if bytes != self.after {
            return Err(changed());
        }
        drop(guard);
        FileGuard::replace_regular(&self.path, &self.identity, &self.after, &self.before)?;
        Ok(true)
    }
}

fn changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Configuration bytes changed; preserving configuration and journal.",
    )
}

impl fmt::Debug for ConfigRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigRecord")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for ConfigChange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.record.fmt(formatter)
    }
}

impl ConfigChange {
    pub fn publish(&self) -> io::Result<ConfigSnapshot> {
        self.record.publish()
    }
}

impl ConfigSnapshot {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn guard(&self) -> io::Result<FileGuard> {
        let guard = FileGuard::open_regular(&self.path, &self.bytes)?;
        if guard.object_identity()? != self.identity {
            return Err(io::Error::other("configuration object identity changed"));
        }
        Ok(guard)
    }

    pub(crate) fn fingerprint(&self) -> (LinkIdentity, String) {
        (
            self.identity.clone(),
            crate::build_identity::hash_bytes(&self.bytes),
        )
    }

    /// Captures an existing, single-link ordinary file on supported local NTFS.
    /// No handle remains open during expensive candidate preparation.
    pub fn read(path: &Path) -> io::Result<Self> {
        let (guard, bytes) = FileGuard::read_regular(path)?;
        Ok(Self {
            path: path.to_owned(),
            identity: guard.object_identity()?,
            bytes,
        })
    }

    pub fn contents(&self) -> &[u8] {
        &self.bytes
    }

    /// Rechecks the captured ordinary object and bytes without publishing.
    /// This is an observation at call time, not a lock across external work.
    pub fn verify_unchanged(&self) -> io::Result<()> {
        self.guard().map(|_| ())
    }

    pub fn plan_replace(&self, bytes: &[u8]) -> io::Result<ConfigChange> {
        let record = ConfigRecord {
            path: self.path.clone(),
            identity: self.identity.clone(),
            before: self.bytes.clone(),
            after: bytes.to_owned(),
        };
        record.validate()?;
        Ok(ConfigChange { record })
    }

    /// Publishes all bytes or preserves the original on failure. Refuses a
    /// replacement object even if its contents equal the original snapshot.
    /// The returned snapshot can guard a subsequent change or explicit rollback.
    pub fn replace(&self, bytes: &[u8]) -> io::Result<Self> {
        self.plan_replace(bytes)?.publish()
    }
}

impl fmt::Debug for ConfigSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigSnapshot")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}
