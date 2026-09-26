//! Small kernel for owned skill-library comparison, isolation and later decisions.
//!
//! Model execution stays on the existing `outcome-*` runner. This crate does not
//! launch Codex, mutate the user library or write Git.
//! `catalogue` derives its view from the native consumer's effective discovery
//! and never scans discovery roots itself; `delivery` renders that view bounded.

pub mod budget;
pub mod catalogue;
pub mod comparison;
pub mod consolidate;
pub mod decision;
pub mod delivery;
pub mod disable;
pub mod identity;
pub mod integrity;
pub mod isolation;
pub mod journal;
pub mod kernel;
pub mod ledger;
pub mod ownership;
pub mod package;
pub mod plan;
pub mod promotion;
pub mod publication;
pub mod routing;
pub mod session;
pub mod signal;
pub mod staging;
pub mod usage;

use sha2::{Digest, Sha256};
use std::{fs, io, path::Path};

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn hash_file(path: &Path) -> io::Result<String> {
    hash_reader(&mut fs::File::open(path)?)
}

pub(crate) fn hash_reader(reader: &mut impl io::Read) -> io::Result<String> {
    let mut digest = Sha256::new();
    io::copy(reader, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn invalid(message: &'static str) -> io::Error {
    io::Error::other(message)
}

pub(crate) fn ordinary_metadata(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("skill evolution refuses reparse points"));
    }
    Ok(metadata)
}
