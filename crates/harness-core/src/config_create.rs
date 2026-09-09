//! Journal-only creation of absent configuration files. Existing objects are
//! never adopted, overwritten or removed by content equality alone.
#![cfg(windows)]

use crate::registration_native::{self as native, FileGuard, LinkIdentity, MAX_REGULAR_BYTES};
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct ConfigCreation {
    pub(crate) path: PathBuf,
    bytes: Vec<u8>,
}

impl ConfigCreation {
    /// Prepares data only. Registration checks absence again before staging,
    /// then publishes exclusively after its creation proof is durable.
    pub fn new(path: &Path, bytes: &[u8]) -> io::Result<Self> {
        validate_size(bytes)?;
        Ok(Self {
            path: path.to_owned(),
            bytes: bytes.to_owned(),
        })
    }

    pub(crate) fn check_absent(&self) -> io::Result<()> {
        match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
            Ok(_) => Err(conflict()),
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl fmt::Debug for ConfigCreation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigCreation")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreatedConfig {
    pub(crate) path: PathBuf,
    pub(crate) stage: PathBuf,
    identity: LinkIdentity,
    bytes: Vec<u8>,
}

impl CreatedConfig {
    pub(crate) fn new(plan: &ConfigCreation, stage: PathBuf, identity: LinkIdentity) -> Self {
        Self {
            path: plan.path.clone(),
            stage,
            identity,
            bytes: plan.bytes.clone(),
        }
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        validate_size(&self.bytes)?;
        if self.path == self.stage || self.path.parent() != self.stage.parent() {
            return Err(conflict());
        }
        Ok(())
    }

    pub(crate) fn same_candidate(&self, plan: &ConfigCreation) -> bool {
        self.path == plan.path && self.bytes == plan.bytes
    }

    fn guard(&self, path: &Path) -> io::Result<FileGuard> {
        let guard = FileGuard::open_regular(path, &self.bytes)?;
        if guard.object_identity()? != self.identity {
            return Err(conflict());
        }
        Ok(guard)
    }

    pub(crate) fn publish(&self) -> io::Result<()> {
        native::publish_regular(&self.stage, &self.path, &self.bytes, &self.identity)
    }

    pub(crate) fn check_published(&self) -> io::Result<()> {
        self.hold_published().map(drop)
    }

    pub(crate) fn hold_published(&self) -> io::Result<FileGuard> {
        self.guard(&self.path)
    }

    pub(crate) fn published_identity(&self) -> &LinkIdentity {
        &self.identity
    }

    pub(crate) fn published_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn check_undo(&self) -> io::Result<()> {
        for path in [&self.path, &self.stage] {
            match self.guard(path) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub(crate) fn undo(&self) -> io::Result<bool> {
        let mut removed_destination = false;
        for path in [&self.path, &self.stage] {
            match self.guard(path) {
                Ok(guard) => {
                    guard.remove()?;
                    removed_destination |= path == &self.path;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(removed_destination)
    }
}

impl fmt::Debug for CreatedConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreatedConfig")
            .field("path", &self.path)
            .field("stage", &self.stage)
            .finish_non_exhaustive()
    }
}

fn validate_size(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_REGULAR_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Configuration exceeds 16 MiB.",
        ));
    }
    Ok(())
}

fn conflict() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Configuration creation ownership conflict; preserving files and journal.",
    )
}
