//! Native disconnection of a legacy or schema-2 installation. No source target is opened
//! for deletion; the exact recorded link IDs and metadata authorize each change.
#![cfg(windows)]
use crate::{
    core_install::Prior,
    installation_lock::InstallationLocks,
    installation_metadata::InstallationOwners,
    installation_path::PathChange,
    installation_state::normal,
    inventory,
    registration::{COMMIT, COMPLETION, JOURNAL, LinkChange, Registration},
    registration_native,
};
use serde::Serialize;
use std::{fs, io, path::Path};

#[derive(Serialize)]
pub struct Report {
    pub status: &'static str,
    pub removed_links: usize,
    pub already_missing: usize,
    pub preserved_adopted: usize,
    pub path_change: bool,
    pub model_calls: u32,
}

fn absent(path: &Path) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(io::Error::other(
            "pending operation or changed destination; preserving it",
        )),
    }
}

pub fn disconnect(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
    preview: bool,
) -> io::Result<Report> {
    let codex_home = normal(codex_home)?;
    let user_home = normal(user_home)?;
    let dependency_user_home = normal(dependency_user_home)?;
    let owners = InstallationOwners::new(&codex_home, &user_home, &dependency_user_home)?;
    let _locks = InstallationLocks::acquire(&user_home, &dependency_user_home)?;
    for name in ["pending.json", "token-workflow-pending.json"] {
        absent(&codex_home.join("harness").join(name))?;
    }
    let state = codex_home.join("harness/native-registration");
    for name in [JOURNAL, COMMIT, COMPLETION] {
        absent(&state.join(name))?;
    }
    let metadata = Prior::read_homes(&codex_home, &user_home, &dependency_user_home)?;
    let Some(settings) = metadata.settings() else {
        return Ok(Report {
            status: "not-connected",
            removed_links: 0,
            already_missing: 0,
            preserved_adopted: 0,
            path_change: false,
            model_calls: 0,
        });
    };
    let mut changes = Vec::new();
    let mut missing = Vec::new();
    let links = metadata.links();
    for (link, _) in links.iter().filter(|(_, owned)| *owned) {
        match fs::symlink_metadata(&link.destination) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(link.destination.clone());
                continue;
            }
            Err(error) => return Err(error),
            Ok(_) => {}
        }
        let guard = registration_native::verified_link(
            &link.destination,
            &link.source,
            matches!(link.kind.as_str(), "skill" | "agents"),
        )?;
        if let Prior::Native(native) = &metadata {
            let expected = native
                .links()
                .iter()
                .find(|old| old.object.path == link.destination)
                .ok_or_else(|| io::Error::other("missing native ownership record"))?;
            if guard.object_identity()? != expected.object.identity {
                return Err(io::Error::other(
                    "owned link identity changed; preserving it",
                ));
            }
        }
        drop(guard);
        changes.push(LinkChange::remove(&link.destination, &link.source)?);
    }
    let path = PathChange::remove(
        settings.path_scope,
        &codex_home.join("harness/bin"),
        settings.path_added,
    )?;
    let report = Report {
        status: if preview { "preview" } else { "disconnected" },
        removed_links: changes.len(),
        already_missing: missing.len(),
        preserved_adopted: links.iter().filter(|(_, owned)| !owned).count(),
        path_change: !path.is_noop(),
        model_calls: 0,
    };
    metadata.snapshot()?.verify_unchanged()?;
    if preview {
        return Ok(report);
    }
    let reg = Registration::open(&state)?;
    let applied = reg.apply_disconnection(
        &changes,
        metadata.snapshot()?,
        |view| metadata.disconnect_bytes(view),
        report.path_change.then_some(path),
        || {
            for path in &missing {
                absent(path)?;
            }
            Ok(())
        },
    );
    if let Err(error) = applied {
        if let Err(recovery) = reg.recover() {
            return Err(io::Error::other(format!(
                "native disconnect failed: {error}; recovery pending: {recovery}"
            )));
        }
        return Err(error);
    }
    reg.finish_owned_installation(&owners)?;
    Ok(report)
}
