//! Read-only import boundary for the installed PowerShell metadata schema.
//! Legacy ownership is a recorded claim; mutation still requires capture of
//! the actual link and revalidation of this exact metadata snapshot.
#![cfg(windows)]

use crate::{config_file::ConfigSnapshot, inventory::ordinary_parents};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt, fs, io,
    path::{Component, Path, PathBuf, Prefix},
};

const MAX_STATE: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    schema_version: u32,
    source_root: PathBuf,
    codex_home: PathBuf,
    user_home: PathBuf,
    dependency_user_home: Option<PathBuf>,
    codex_command: PathBuf,
    #[serde(default)]
    config_bridge: Option<PathBuf>,
    profile_name: String,
    links: Vec<LegacyLink>,
    path_scope: PathScope,
    path_added: bool,
    versions: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Copy, Serialize, Debug, PartialEq, Eq)]
pub enum PathScope {
    User,
    Process,
}

impl PathScope {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("User") {
            Some(Self::User)
        } else if value.eq_ignore_ascii_case("Process") {
            Some(Self::Process)
        } else {
            None
        }
    }
}

impl<'de> Deserialize<'de> for PathScope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // PowerShell ValidateSet accepts and preserves mixed-case spelling in
        // old metadata. Serialization of new native records remains canonical.
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| serde::de::Error::custom("unsupported PATH scope"))
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct LegacyLink {
    pub kind: String,
    pub name: String,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub owned: bool,
}

pub struct LegacyInstallation {
    state: State,
    snapshot: ConfigSnapshot,
}

#[derive(Serialize)]
pub struct LegacySummary<'a> {
    pub schema: u32,
    pub source_root: &'a Path,
    pub codex_home: &'a Path,
    pub user_home: &'a Path,
    pub dependency_user_home: &'a Path,
    pub profile_name: &'a str,
    pub links: usize,
    pub owned_links: usize,
    pub adopted_links: usize,
    pub path_scope: PathScope,
    pub path_added: bool,
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "legacy installation metadata is invalid or belongs to another host; preserving it",
    )
}

pub(crate) fn normal(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute()
        || path.to_str().is_none_or(|s| s.contains('\0'))
        || path.components().any(|c| matches!(c, Component::ParentDir))
        || !matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)))
    {
        return Err(invalid());
    }
    std::path::absolute(path).map_err(|_| invalid())
}

pub(crate) fn key(path: &Path) -> io::Result<String> {
    Ok(normal(path)?
        .to_str()
        .ok_or_else(invalid)?
        .trim_end_matches('\\')
        .to_lowercase())
}

pub(crate) fn portable_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

impl LegacyInstallation {
    pub(crate) fn snapshot(&self) -> &ConfigSnapshot {
        &self.snapshot
    }

    pub(crate) fn native_settings(&self) -> crate::installation_metadata::Settings {
        crate::installation_metadata::Settings {
            source_root: self.state.source_root.clone(),
            codex_home: self.state.codex_home.clone(),
            user_home: self.state.user_home.clone(),
            dependency_user_home: self
                .state
                .dependency_user_home
                .clone()
                .unwrap_or_else(|| self.state.user_home.clone()),
            codex_command: self.state.codex_command.clone(),
            path_scope: self.state.path_scope,
            path_added: self.state.path_added,
            versions: self.state.versions.clone(),
        }
    }

    pub(crate) fn metadata_destination(
        &self,
    ) -> io::Result<crate::registration::metadata::MetadataDestination> {
        crate::registration::metadata::MetadataDestination::existing(&self.snapshot)
    }

    pub(crate) fn metadata_fingerprint(
        &self,
    ) -> (crate::registration_native::LinkIdentity, String) {
        self.snapshot.fingerprint()
    }

    /// Missing state returns None. Existing pending recovery, foreign/reparse
    /// metadata, unsupported schema and host mismatches never become a fresh
    /// installation. Reading does not execute the recorded upstream command.
    pub fn read(
        codex_home: &Path,
        user_home: &Path,
        dependency_user_home: &Path,
    ) -> io::Result<Option<Self>> {
        let codex_home = normal(codex_home)?;
        let user_home = normal(user_home)?;
        let dependency_user_home = normal(dependency_user_home)?;
        let path = codex_home.join("harness/installation.json");
        ordinary_parents(&path)?;
        match fs::symlink_metadata(codex_home.join("harness/pending.json")) {
            Ok(_) => {
                return Err(io::Error::other(
                    "legacy installation has pending recovery; preserve it and recover before migration",
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        let snapshot = ConfigSnapshot::read(&path)?;
        if snapshot.contents().len() > MAX_STATE {
            return Err(invalid());
        }
        let state: State = serde_json::from_slice(snapshot.contents()).map_err(|_| invalid())?;
        if state.schema_version != 1
            || key(&state.codex_home)? != key(&codex_home)?
            || key(&state.user_home)? != key(&user_home)?
            || key(state
                .dependency_user_home
                .as_deref()
                .unwrap_or(&state.user_home))?
                != key(&dependency_user_home)?
            || state.profile_name != "harness"
            || state.links.len() > 4096
            || state.versions.len() > 64
        {
            return Err(invalid());
        }
        let source_root = normal(&state.source_root)?;
        normal(&state.codex_command)?;
        if let Some(bridge) = &state.config_bridge {
            normal(bridge)?;
        }
        // The old checkout and upstream may be missing after relocation. Check
        // only recorded names here; do not follow/read their former locations.
        let source_prefix = format!("{}\\", key(&source_root)?);
        if key(&codex_home)?.starts_with(&source_prefix) || key(&codex_home)? == key(&source_root)?
        {
            return Err(invalid());
        }
        let mut destinations = BTreeSet::new();
        for link in &state.links {
            if !portable_name(&link.name) {
                return Err(invalid());
            }
            let expected = match link.kind.as_str() {
                "instructions" => codex_home.join("AGENTS.md"),
                "profile" => codex_home.join("harness.config.toml"),
                "launcher" => codex_home.join("harness/bin/codex.ps1"),
                "diagnostic-launcher" => codex_home.join("harness/bin/codex-harness-check.ps1"),
                "agents" => codex_home.join("agents/codex-harness"),
                "hooks" => codex_home.join("hooks.json"),
                "hook-launcher" => codex_home.join("harness/bin/hook.ps1"),
                // The old inventory registered the directory name; the skill's
                // declared capability name is independent of that directory.
                "skill" => user_home
                    .join(".agents/skills")
                    .join(link.source.file_name().ok_or_else(invalid)?),
                _ => return Err(invalid()),
            };
            let destination = key(&link.destination)?;
            if destination != key(&expected)?
                || !destinations.insert(destination)
                || !key(&link.source)?.starts_with(&source_prefix)
            {
                return Err(invalid());
            }
            ordinary_parents(&link.destination)?;
        }
        Ok(Some(Self { state, snapshot }))
    }

    pub fn summary(&self) -> LegacySummary<'_> {
        let owned = self.state.links.iter().filter(|l| l.owned).count();
        LegacySummary {
            schema: self.state.schema_version,
            source_root: &self.state.source_root,
            codex_home: &self.state.codex_home,
            user_home: &self.state.user_home,
            dependency_user_home: self
                .state
                .dependency_user_home
                .as_deref()
                .unwrap_or(&self.state.user_home),
            profile_name: &self.state.profile_name,
            links: self.state.links.len(),
            owned_links: owned,
            adopted_links: self.state.links.len() - owned,
            path_scope: self.state.path_scope,
            path_added: self.state.path_added,
        }
    }

    pub fn links(&self) -> &[LegacyLink] {
        &self.state.links
    }

    /// Recheck identity and every original byte before relying on the metadata
    /// for a mutation plan. This does not publish or normalize the JSON file.
    pub fn verify_unchanged(&self) -> io::Result<()> {
        self.snapshot
            .plan_replace(self.snapshot.contents())?
            .record
            .check_before()
    }
}

impl fmt::Debug for LegacyInstallation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LegacyInstallation")
            .field("links", &self.state.links.len())
            .finish_non_exhaustive()
    }
}
