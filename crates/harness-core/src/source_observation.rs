//! Read-only installation/source observations for native Diagnose. No CLI,
//! repair, provisioning or configuration body is returned by this layer.
#![cfg(windows)]

use crate::{
    core_install::Prior,
    installation_lock::InstallationLocks,
    installation_state::{key, normal},
    inventory,
    registration_native::FileGuard,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

pub struct Observation {
    pub source: Option<PathBuf>,
    pub upstream: Option<PathBuf>,
    pub links: Vec<Value>,
    pub findings: Vec<Value>,
}

fn finding(code: &str, source: &Path, action: &str) -> Value {
    json!({"code":code,"source":source,"action":action})
}

pub fn read(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
    source: Option<&Path>,
) -> io::Result<Observation> {
    for path in [codex_home, user_home, dependency_user_home] {
        normal(path)?;
    }
    if let Some(source) = source {
        normal(source)?;
    }
    let _locks = InstallationLocks::acquire(user_home, dependency_user_home)?;
    let prior = Prior::read_homes(codex_home, user_home, dependency_user_home).ok();
    let settings = prior.as_ref().and_then(Prior::settings);
    let source = source
        .map(Path::to_path_buf)
        .or_else(|| settings.as_ref().map(|s| s.source_root.clone()));
    let upstream = settings.as_ref().map(|s| s.codex_command.clone());
    let mut result = Observation {
        source,
        upstream,
        links: Vec::new(),
        findings: Vec::new(),
    };
    if settings.is_none() {
        result.findings.push(finding(
            "installation-unavailable",
            &codex_home.join("harness/installation.json"),
            "Install or recover the kit for these owner homes.",
        ));
    }
    for relative in [
        "pending.json",
        "activation-pending.json",
        "token-workflow-pending.json",
        "native-registration/journal.json",
        "native-registration/complete.json",
        "native-registration/commit.json",
    ] {
        let path = codex_home.join("harness").join(relative);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => result.findings.push(finding(
                "pending-transaction",
                &path,
                "Inspect and recover the recorded operation before changing connections.",
            )),
        }
    }
    let mut expected = BTreeMap::new();
    if let Some(prior) = &prior {
        for (link, _) in prior.links() {
            expected.insert(key(&link.destination)?, link);
        }
    }
    if let Some(source) = &result.source {
        match inventory::read(source, codex_home, user_home) {
            Ok(inventory) => {
                for link in inventory.links {
                    expected.insert(key(&link.destination)?, link);
                }
            }
            Err(_) => result.findings.push(finding(
                "inventory-unavailable",
                source,
                "Restore the complete selected checkout and rerun Diagnose.",
            )),
        }
    } else {
        result.findings.push(finding(
            "inventory-unavailable",
            &codex_home.join("harness/installation.json"),
            "Select the intended source checkout explicitly or restore its installation record.",
        ));
    }
    for (destination_key, link) in expected {
        let mut actual = None;
        let status = match fs::symlink_metadata(&link.destination) {
            Err(_) => "missing-or-unreadable",
            Ok(_) => match FileGuard::capture_link(&link.destination) {
                Err(_) => "not-a-link-or-unreadable",
                Ok(guard) => {
                    let observed = (|| -> io::Result<_> {
                        let (target, _) = guard.link_description()?;
                        actual = Some(target.clone());
                        let correct =
                            crate::registration_native::targets_match(&target, &link.source)?;
                        let identity = guard.object_identity()?;
                        let identity_changed = match &prior {
                            Some(Prior::Native(metadata)) => metadata
                                .links()
                                .iter()
                                .find(|old| {
                                    key(&old.object.path).ok().as_ref() == Some(&destination_key)
                                })
                                .is_some_and(|old| identity != old.object.identity),
                            _ => false,
                        };
                        Ok(if !correct {
                            "retargeted"
                        } else if identity_changed {
                            "identity-changed"
                        } else if !link.source.try_exists()? {
                            "missing-source"
                        } else {
                            "connected"
                        })
                    })();
                    observed.unwrap_or("unreadable")
                }
            },
        };
        result
            .links
            .push(json!({"kind":link.kind,"destination":link.destination,
            "expected":link.source,"actual":actual,"status":status}));
        if status != "connected" {
            result.findings.push(finding(&format!("link-{status}"), &link.destination,
                "Resolve ownership of this destination, then explicitly update the intended installation."));
        }
    }
    let override_path = codex_home.join("AGENTS.override.md");
    if fs::symlink_metadata(&override_path).is_ok() {
        result.findings.push(finding(
            "instructions-hidden",
            &override_path,
            "Review the global override that hides AGENTS.md; compose or remove it explicitly.",
        ));
    }
    Ok(result)
}
