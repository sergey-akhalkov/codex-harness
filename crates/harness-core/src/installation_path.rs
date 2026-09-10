//! The installer chooses one PATH scope; the journal stores its distinct
//! durable-registry or process-owned recovery contract.
#![cfg(windows)]
use crate::{
    environment_path::{UserPathChange, UserPathSnapshot},
    installation_state::PathScope,
    process_path::{ProcessPathChange, ProcessPathSnapshot},
};
use std::{io, path::Path};

#[derive(Clone, Debug)]
pub(crate) enum PathChange {
    User(UserPathChange),
    Process(ProcessPathChange),
}

impl From<UserPathChange> for PathChange {
    fn from(value: UserPathChange) -> Self {
        Self::User(value)
    }
}

impl PathChange {
    pub(crate) fn prepend(scope: PathScope, bin: &Path) -> io::Result<(Self, bool)> {
        match scope {
            PathScope::User => UserPathSnapshot::for_registration()?
                .prepend(bin)
                .map(|(change, added)| (Self::User(change), added)),
            PathScope::Process => ProcessPathSnapshot::read()?
                .prepend(bin)
                .map(|(change, added)| (Self::Process(change), added)),
        }
    }

    pub(crate) fn remove(scope: PathScope, bin: &Path, was_added: bool) -> io::Result<Self> {
        match scope {
            PathScope::User => UserPathSnapshot::for_registration()?
                .remove(bin, was_added)
                .map(Self::User),
            PathScope::Process => ProcessPathSnapshot::read()?
                .remove(bin, was_added)
                .map(Self::Process),
        }
    }

    pub(crate) fn is_noop(&self) -> bool {
        match self {
            Self::User(change) => change.is_noop(),
            Self::Process(change) => change.is_noop(),
        }
    }

    pub(crate) fn split(self) -> (Option<UserPathChange>, Option<ProcessPathChange>) {
        match self {
            Self::User(change) => (Some(change), None),
            Self::Process(change) => (None, Some(change)),
        }
    }
}
