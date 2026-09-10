//! Native core connection. Inputs and ownership are prepared before publication;
//! the registration journal owns publication, acceptance and interrupted recovery.
#![cfg(windows)]

use crate::{
    agent_config, build_identity,
    config_create::ConfigCreation,
    config_file::{ConfigChange, ConfigSnapshot},
    core_runtime,
    feature_edit::{self, Feature},
    installation_lock::InstallationLocks,
    installation_metadata::{self, InstallationMetadata, InstallationOwners, Previous, Settings},
    installation_path::PathChange,
    installation_state::{LegacyInstallation, PathScope, key, normal},
    inventory::{self, Connection, Link},
    native_launcher, native_upstream,
    registration::{LinkChange, Registration, metadata::MetadataDestination},
    registration_native::{self as native, FileGuard, StagedFile},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Request {
    pub source: PathBuf,
    pub build: PathBuf,
    pub codex_home: PathBuf,
    pub user_home: PathBuf,
    pub dependency_user_home: PathBuf,
    pub upstream: Option<PathBuf>,
    pub timeout: Duration,
    pub path_scope: Option<PathScope>,
}

#[derive(Serialize)]
pub struct Report {
    pub status: &'static str,
    pub model_calls: u32,
    pub links: usize,
    pub changed_links: usize,
    pub path_change: bool,
    pub runtime: Option<core_runtime::RuntimeReceipt>,
}

#[derive(Serialize)]
pub struct RecoveryReport {
    pub status: &'static str,
    pub committed: bool,
    pub model_calls: u32,
}

#[derive(Serialize)]
pub struct RecoveryPreview {
    pub status: &'static str,
    pub action: &'static str,
    pub journal: Option<&'static str>,
    pub model_calls: u32,
}

/// Read-only plan, including ownership and available inverse checks. A later
/// recovery revalidates the current state; this is not a durable commitment.
pub fn preview_recovery(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> io::Result<RecoveryPreview> {
    let owners = InstallationOwners::new(codex_home, user_home, dependency_user_home)?;
    let _locks = InstallationLocks::acquire(user_home, dependency_user_home)?;
    let (action, journal) =
        if crate::legacy_pending::preview(codex_home, user_home, dependency_user_home)? {
            ("rollback", Some("legacy"))
        } else if let Some(reg) =
            Registration::open_existing(&codex_home.join("harness/native-registration"))?
        {
            match reg.preview_owned_installation(&owners)? {
                Some(true) => ("finish", Some("native")),
                Some(false) => ("rollback", Some("native")),
                None => ("none", None),
            }
        } else {
            ("none", None)
        };
    Ok(RecoveryPreview {
        status: "preview",
        action,
        journal,
        model_calls: 0,
    })
}

pub fn recover(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> io::Result<RecoveryReport> {
    let owners = InstallationOwners::new(codex_home, user_home, dependency_user_home)?;
    let _locks = InstallationLocks::acquire(user_home, dependency_user_home)?;
    if crate::legacy_pending::recover(codex_home, user_home, dependency_user_home)? {
        return Ok(RecoveryReport {
            status: "recovered-legacy",
            committed: false,
            model_calls: 0,
        });
    }
    let state = codex_home.join("harness/native-registration");
    inventory::ordinary_parents(&state.join("owner"))?;
    match fs::symlink_metadata(&state) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RecoveryReport {
                status: "no-pending-operation",
                committed: false,
                model_calls: 0,
            });
        }
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let reg = Registration::open(&state)?;
    let report = reg.recover_owned_installation(&owners)?;
    Ok(RecoveryReport {
        status: "recovered",
        committed: report.committed,
        model_calls: 0,
    })
}

pub(crate) enum Prior {
    Fresh,
    Legacy(LegacyInstallation),
    Native(InstallationMetadata),
}
impl Prior {
    fn read(request: &Request) -> io::Result<Self> {
        Self::read_homes(
            &request.codex_home,
            &request.user_home,
            &request.dependency_user_home,
        )
    }
    pub(crate) fn read_homes(
        codex_home: &Path,
        user_home: &Path,
        dependency_user_home: &Path,
    ) -> io::Result<Self> {
        let path = codex_home.join("harness/installation.json");
        absent(&codex_home.join("harness/pending.json"))?;
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::Fresh),
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        let snapshot = ConfigSnapshot::read(&path)?;
        if snapshot.contents().len() > 1024 * 1024 {
            return Err(conflict());
        }
        let header: serde_json::Value =
            serde_json::from_slice(snapshot.contents()).map_err(|_| conflict())?;
        match header.get("schemaVersion").and_then(|v| v.as_u64()) {
            Some(1) => Ok(Self::Legacy(
                LegacyInstallation::read(codex_home, user_home, dependency_user_home)?
                    .ok_or_else(conflict)?,
            )),
            Some(2) => Ok(Self::Native(
                InstallationMetadata::read(codex_home, user_home, dependency_user_home)?
                    .ok_or_else(conflict)?,
            )),
            _ => Err(conflict()),
        }
    }
    fn previous(&self) -> Previous<'_> {
        match self {
            Self::Fresh => Previous::Fresh,
            Self::Legacy(s) => Previous::Legacy(s),
            Self::Native(s) => Previous::Native(s),
        }
    }
    pub(crate) fn settings(&self) -> Option<Settings> {
        match self {
            Self::Fresh => None,
            Self::Legacy(s) => Some(s.native_settings()),
            Self::Native(s) => Some(s.settings().clone()),
        }
    }
    fn destination(&self, path: &Path) -> io::Result<MetadataDestination> {
        match self {
            Self::Fresh => MetadataDestination::absent(path),
            Self::Legacy(s) => s.metadata_destination(),
            Self::Native(s) => s.destination(),
        }
    }
    pub(crate) fn links(&self) -> Vec<(Link, bool)> {
        match self {
            Self::Fresh => Vec::new(),
            Self::Legacy(s) => s
                .links()
                .iter()
                .map(|l| {
                    (
                        Link {
                            kind: l.kind.clone(),
                            name: l.name.clone(),
                            source: l.source.clone(),
                            destination: l.destination.clone(),
                            connection: Connection::Linked,
                        },
                        l.owned,
                    )
                })
                .collect(),
            Self::Native(s) => s
                .links()
                .iter()
                .map(|l| {
                    (
                        Link {
                            kind: l.kind.clone(),
                            name: l.name.clone(),
                            source: l.object.target.clone(),
                            destination: l.object.path.clone(),
                            connection: Connection::Linked,
                        },
                        l.owned,
                    )
                })
                .collect(),
        }
    }

    pub(crate) fn snapshot(&self) -> io::Result<&ConfigSnapshot> {
        match self {
            Self::Fresh => Err(conflict()),
            Self::Legacy(old) => Ok(old.snapshot()),
            Self::Native(old) => Ok(old.snapshot()),
        }
    }

    pub(crate) fn disconnect_bytes(
        &self,
        view: &crate::registration::metadata::MetadataView,
    ) -> io::Result<Vec<u8>> {
        match self {
            Self::Fresh => Err(conflict()),
            Self::Legacy(old) => installation_metadata::disconnect_legacy(old, view),
            Self::Native(old) => old.disconnect_bytes(view),
        }
    }
}

fn conflict() -> io::Error {
    io::Error::other("core installation conflicts with current ownership; preserving it")
}
fn absent(path: &Path) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(_) => Err(conflict()),
    }
}
fn plain(path: &Path) -> io::Result<PathBuf> {
    let text = path.to_str().ok_or_else(conflict)?;
    normal(Path::new(text.strip_prefix("\\\\?\\").unwrap_or(text)))
}

struct Plan {
    prior: Prior,
    settings: Settings,
    desired: Vec<Link>,
    changes: Vec<LinkChange>,
    retired_absent: Vec<PathBuf>,
    launch_bytes: Vec<u8>,
    reused_launch: Option<PathBuf>,
    instructions: PathBuf,
    upstream: PathBuf,
    path: PathChange,
    planned_changes: usize,
}

impl Plan {
    fn prepare(request: &Request) -> io::Result<Self> {
        for path in [
            &request.source,
            &request.build,
            &request.codex_home,
            &request.user_home,
            &request.dependency_user_home,
        ] {
            normal(path)?;
        }
        if request.timeout.is_zero() || request.timeout > Duration::from_secs(300) {
            return Err(conflict());
        }
        // Old recovery state cannot be treated as a fresh installation.
        absent(
            &request
                .codex_home
                .join("harness/native-registration/journal.json"),
        )?;
        absent(
            &request
                .codex_home
                .join("harness/native-registration/commit.json"),
        )?;
        absent(
            &request
                .codex_home
                .join("harness/native-registration/complete.json"),
        )?;
        absent(
            &request
                .codex_home
                .join("harness/token-workflow-pending.json"),
        )?;
        absent(&request.codex_home.join("AGENTS.override.md"))?;
        let prior = Prior::read(request)?;
        let previous = prior.settings();
        if let (Some(previous), Some(requested)) = (&previous, request.path_scope)
            && previous.path_scope != requested
        {
            return Err(io::Error::other(
                "changing an existing installation's PATH scope requires disconnection first",
            ));
        }
        let scope = previous
            .as_ref()
            .map(|s| s.path_scope)
            .or(request.path_scope)
            .unwrap_or(PathScope::User);
        let build = build_identity::check(&request.build, Some(&request.source));
        if !build.runtime_allowed {
            return Err(io::Error::other(
                "native candidate build is stale, missing or altered; explicit build required",
            ));
        }
        if key(&plain(
            &build_identity::read_record(&request.build)?.source_root,
        )?)? != key(&plain(&request.source.canonicalize()?)?)?
        {
            return Err(conflict());
        }
        let mut excluded = Vec::new();
        for binary in build_identity::BINARIES {
            excluded.push(request.build.join(binary));
            excluded.push(request.codex_home.join("harness/bin").join(binary));
        }
        for name in ["codex.ps1", "codex.cmd"] {
            excluded.push(request.codex_home.join("harness/bin").join(name));
        }
        let selection = native_upstream::resolve(
            request.upstream.as_deref().or_else(|| {
                previous
                    .as_ref()
                    .map(|settings| settings.codex_command.as_path())
            }),
            std::env::var_os("PATH").as_deref(),
            &excluded,
            &native_upstream::ManagerHints {
                user_agent: std::env::var("npm_config_user_agent").unwrap_or_default(),
                exec_path: std::env::var("npm_execpath").unwrap_or_default(),
            },
        )?;
        let upstream = selection.executable.clone();
        let inventory = inventory::read(&request.source, &request.codex_home, &request.user_home)?;
        let instructions = request.source.join(&inventory.manifest.instructions);
        agent_config::check(&request.codex_home, &inventory.agents)?;
        let mut desired = inventory.links;
        for link in &mut desired {
            link.source = plain(&link.source)?;
        }
        let old = prior.links();
        for (link, _) in &old {
            match fs::symlink_metadata(&link.destination) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
                Ok(_) => {}
            }
            let guard = FileGuard::capture_link(&link.destination)?;
            let (target, directory) = guard.link_description()?;
            if !native::targets_match(&target, &link.source)? {
                return Err(conflict());
            }
            if let Prior::Native(metadata) = &prior {
                let expected = metadata
                    .links()
                    .iter()
                    .find(|l| l.object.path == link.destination)
                    .ok_or_else(conflict)?;
                if guard.object_identity()? != expected.object.identity
                    || directory
                        != matches!(
                            expected.object.link_type,
                            crate::registration::LinkType::Directory
                        )
                {
                    return Err(conflict());
                }
            }
        }
        // A core update inherits recorded hook connections until their owning
        // component migrates them. Fresh core connections do not add hooks.
        for (link, _) in old
            .iter()
            .filter(|(l, _)| matches!(l.kind.as_str(), "hooks" | "hook-launcher"))
        {
            let source_root = &previous.as_ref().ok_or_else(conflict)?.source_root;
            let relative = link
                .source
                .strip_prefix(source_root)
                .map_err(|_| conflict())?;
            let mut next = link.clone();
            next.source = request.source.join(relative);
            desired.push(next);
        }
        for binary in build_identity::BINARIES {
            desired.push(Link {
                kind: "native-command".into(),
                name: binary.trim_end_matches(".exe").into(),
                source: request.build.join(binary),
                destination: request.codex_home.join("harness/bin").join(binary),
                connection: Connection::Missing,
            });
        }
        let launch_bytes = serde_json::to_vec_pretty(&native_launcher::Registration {
            schema: 2,
            state: None,
            build: Some(request.build.clone()),
            upstream: selection,
        })?;
        desired.push(Link {
            kind: "native-diagnostic".into(),
            name: "codex-harness-check".into(),
            source: request.build.join("codex-harness.exe"),
            destination: request
                .codex_home
                .join("harness/bin/codex-harness-check.exe"),
            connection: Connection::Missing,
        });
        let mut reused_launch = None;
        if let Some((link, _)) = old.iter().find(|(l, _)| l.kind == "native-launch") {
            let data = ConfigSnapshot::read(&link.source)?;
            if data.contents() == launch_bytes {
                reused_launch = Some(link.source.clone());
                desired.push(link.clone());
            }
        }
        if reused_launch.is_none() {
            match old.iter().find(|(l, _)| l.kind == "native-launch") {
                Some((_, false)) => return Err(conflict()),
                None => absent(&request.codex_home.join("harness/native-launch.json"))?,
                _ => {}
            }
        }
        let mut changes = Vec::new();
        let mut retired_absent = Vec::new();
        let next_names: BTreeSet<_> = desired
            .iter()
            .map(|l| key(&l.destination))
            .collect::<io::Result<_>>()?;
        for (link, owned) in &old {
            let path_key = key(&link.destination)?;
            if link.kind == "native-launch" && reused_launch.is_none() {
                continue;
            }
            if !next_names.contains(&path_key) {
                if *owned {
                    match fs::symlink_metadata(&link.destination) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            retired_absent.push(link.destination.clone());
                        }
                        Err(error) => return Err(error),
                        Ok(_) => changes.push(LinkChange::remove(&link.destination, &link.source)?),
                    }
                } else {
                    desired.push(link.clone());
                }
            }
        }
        for next in &desired {
            if let Some((before, owned)) = old
                .iter()
                .find(|(l, _)| key(&l.destination).ok() == key(&next.destination).ok())
                && key(&before.source)? != key(&next.source)?
                && fs::symlink_metadata(&next.destination).is_ok()
            {
                if !owned {
                    return Err(conflict());
                }
                changes.push(LinkChange::replace(
                    &next.destination,
                    &before.source,
                    &next.source,
                )?);
            }
        }
        let changed: BTreeSet<_> = changes
            .iter()
            .map(|c| key(c.path()))
            .collect::<io::Result<_>>()?;
        let planned = crate::registration::plan(
            &desired
                .iter()
                .filter(|l| !changed.contains(&key(&l.destination).unwrap_or_default()))
                .cloned()
                .collect::<Vec<_>>(),
        )?;
        let planned_changes = planned.iter().filter(|record| record.created).count()
            + changes.len()
            + usize::from(reused_launch.is_none());
        let (path, added) = PathChange::prepend(scope, &request.codex_home.join("harness/bin"))?;
        let mut settings = previous.unwrap_or(Settings {
            source_root: request.source.clone(),
            codex_home: request.codex_home.clone(),
            user_home: request.user_home.clone(),
            dependency_user_home: request.dependency_user_home.clone(),
            codex_command: upstream.clone(),
            path_scope: scope,
            path_added: false,
            versions: BTreeMap::new(),
        });
        settings.source_root = request.source.clone();
        settings.codex_command = upstream.clone();
        settings.path_added |= added;
        settings
            .versions
            .insert("native-harness".into(), env!("CARGO_PKG_VERSION").into());
        settings.validate()?;
        Ok(Self {
            prior,
            settings,
            desired,
            changes,
            retired_absent,
            launch_bytes,
            reused_launch,
            instructions,
            upstream,
            path,
            planned_changes,
        })
    }
}

/// Explicit native core connection; dependency and subscription components are
/// selected by the outer manager. Preview performs no publication or execution.
pub fn connect(request: &Request, preview: bool) -> io::Result<Report> {
    let _owners = InstallationLocks::acquire(&request.user_home, &request.dependency_user_home)?;
    let mut plan = Plan::prepare(request)?;
    if preview {
        return Ok(Report {
            status: "preview",
            model_calls: 0,
            links: plan.desired.len() + usize::from(plan.reused_launch.is_none()),
            changed_links: plan.planned_changes,
            path_change: !plan.path.is_noop(),
            runtime: None,
        });
    }
    let config_path = request.codex_home.join("config.toml");
    let config = match fs::symlink_metadata(&config_path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
        Ok(_) => Some(ConfigSnapshot::read(&config_path)?),
    };
    let selection = request.codex_home.join("harness/token-workflow.json");
    let keep_hooks = match fs::symlink_metadata(&selection) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
        Ok(_) => {
            let snapshot = ConfigSnapshot::read(&selection)?;
            let value: serde_json::Value =
                serde_json::from_slice(snapshot.contents()).map_err(|_| conflict())?;
            value
                .get("enabled")
                .and_then(|v| v.as_bool())
                .ok_or_else(conflict)?
        }
    };
    let mut configs: Vec<ConfigChange> = Vec::new();
    let mut creations = Vec::new();
    if !keep_hooks {
        let edit = feature_edit::prepare_feature(
            &plan.upstream,
            config.as_ref().map_or(&[], ConfigSnapshot::contents),
            Feature::Hooks,
            false,
            request.timeout,
        )?;
        if let Some(config) = &config {
            configs.push(edit.plan_for(config)?);
        } else {
            creations.push(ConfigCreation::new(&config_path, edit.proposed_config())?);
        }
    }
    let reg = Registration::open(&request.codex_home.join("harness/native-registration"))?;
    if plan.reused_launch.is_none() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let source = reg
            .state()
            .join(format!("launch-{}-{stamp}.json", std::process::id()));
        StagedFile::create(&source, &plan.launch_bytes)?.commit()?;
        let destination = request.codex_home.join("harness/native-launch.json");
        if let Some((old, owned)) = plan
            .prior
            .links()
            .iter()
            .find(|(l, _)| l.kind == "native-launch")
        {
            if !owned {
                return Err(conflict());
            }
            if fs::symlink_metadata(&destination).is_ok() {
                plan.changes
                    .push(LinkChange::replace(&destination, &old.source, &source)?);
            }
        }
        plan.desired.push(Link {
            kind: "native-launch".into(),
            name: "codex".into(),
            source,
            destination,
            connection: Connection::Missing,
        });
    }
    let changed: BTreeSet<_> = plan
        .changes
        .iter()
        .map(|l| key(l.path()))
        .collect::<io::Result<_>>()?;
    let retained: Vec<_> = plan
        .desired
        .iter()
        .filter(|l| !changed.contains(&key(&l.destination).unwrap_or_default()))
        .cloned()
        .collect();
    let metadata = request.codex_home.join("harness/installation.json");
    let mut instructions = Vec::new();
    fs::File::open(&plan.instructions)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut instructions)?;
    if instructions.len() > 1024 * 1024 {
        return Err(conflict());
    }
    let mut runtime = None;
    let applied = reg.apply_installation(
        &retained,
        &configs,
        &creations,
        &plan.changes,
        plan.prior.destination(&metadata)?,
        |view| {
            installation_metadata::encode_retiring_absent(
                &plan.settings,
                &plan.desired,
                view,
                plan.prior.previous(),
                &plan.retired_absent,
            )
        },
        Some(plan.path.clone()),
        || {
            runtime = Some(core_runtime::verify(
                &plan.upstream,
                &request.codex_home.join("harness/bin/codex.exe"),
                &request.codex_home,
                &instructions,
                request.timeout,
            )?);
            Ok(())
        },
    );
    let applied = match applied {
        Ok(report) => report,
        Err(error) => {
            return match reg.recover() {
                Ok(_) => Err(error),
                Err(recovery) => Err(io::Error::other(format!(
                    "core activation failed ({error}); recovery retained: {recovery}"
                ))),
            };
        }
    };
    reg.finish_owned_installation(&InstallationOwners::new(
        &request.codex_home,
        &request.user_home,
        &request.dependency_user_home,
    )?)?;
    Ok(Report {
        status: "connected",
        model_calls: 0,
        links: plan.desired.len(),
        changed_links: applied
            .links
            .iter()
            .filter(|l| l.outcome == crate::registration::Outcome::Created)
            .count()
            + applied.changed_links.len(),
        path_change: !plan.path.is_noop(),
        runtime,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment_path::UserPathSnapshot;
    use serde_json::json;

    struct Fixture {
        root: PathBuf,
        request: Request,
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("native-core проба-")
                .tempdir()
                .unwrap()
                .keep();
            let source = root.join("source");
            let build = root.join("build");
            for dir in [
                source.join("global/agents"),
                source.join("skills/one"),
                source.join("crates/one/src"),
                source.join("tools/rtk-adapter/src"),
                source
                    .join(build_identity::INSPECTION_SCHEMA)
                    .parent()
                    .unwrap()
                    .to_owned(),
                build.clone(),
            ] {
                fs::create_dir_all(dir).unwrap();
            }
            for file in [
                "Cargo.toml",
                "Cargo.lock",
                "crates/one/src/lib.rs",
                build_identity::INSPECTION_SCHEMA,
            ] {
                fs::write(source.join(file), b"fixture").unwrap();
            }
            fs::write(source.join("global/profile.toml"), "approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\nmodel = 'gpt-6-astra'\n").unwrap();
            fs::write(
                source.join("global/instructions.md"),
                "Owned native core acceptance. Preserve foreign data.\n",
            )
            .unwrap();
            for file in ["global/hooks.json", "global/token-hooks.json"] {
                fs::write(source.join(file), b"{}").unwrap();
            }
            fs::write(source.join("skills/one/SKILL.md"), "---\nname: one\ndescription: Owned acceptance skill.\n---\nPreserve foreign data.\n").unwrap();
            fs::write(source.join("global/kit.json"), serde_json::to_vec(&json!({"schema":1,"profile_name":"harness","profile":"global/profile.toml","instructions":"global/instructions.md","skills":"skills","agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/token-hooks.json"})).unwrap()).unwrap();
            for binary in build_identity::BINARIES {
                fs::write(build.join(binary), binary).unwrap();
            }
            let upstream = root.join("upstream.exe");
            fs::write(&upstream, b"explicit fixture upstream").unwrap();
            let fixture = Self {
                request: Request {
                    source,
                    build,
                    codex_home: root.join("codex"),
                    user_home: root.join("user"),
                    dependency_user_home: root.join("dependency"),
                    upstream: Some(upstream),
                    timeout: Duration::from_secs(45),
                    path_scope: None,
                },
                root,
            };
            fixture.record();
            fixture
        }
        fn record(&self) {
            let record = build_identity::BuildRecord {
                schema: build_identity::SCHEMA,
                source_root: self.request.source.clone(),
                source: build_identity::source_identity(&self.request.source).unwrap(),
                rustc: "fixture".into(),
                cargo: "fixture".into(),
                target: "x86_64-pc-windows-msvc".into(),
                profile: "release".into(),
                binaries: build_identity::BINARIES
                    .iter()
                    .map(|name| {
                        (
                            name.to_string(),
                            build_identity::hash_file(&self.request.build.join(name)).unwrap(),
                        )
                    })
                    .collect(),
            };
            fs::write(
                self.request.build.join("build.json"),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        fn real(&mut self) {
            self.request.upstream = Some(
                std::env::var_os("HARNESS_CORE_REAL_UPSTREAM")
                    .expect("explicit original Codex executable required")
                    .into(),
            );
            let launcher = std::env::var_os("HARNESS_CORE_REAL_LAUNCHER")
                .expect("explicit current Rust launcher required");
            fs::copy(launcher, self.request.build.join("codex.exe")).unwrap();
            if let Some(manager) = std::env::var_os("HARNESS_CORE_REAL_MANAGER") {
                fs::copy(manager, self.request.build.join("codex-harness.exe")).unwrap();
            }
            self.record();
        }
    }

    #[test]
    fn preview_checks_build_and_foreign_destinations_without_creating_homes() {
        crate::environment_path::with_test_registry(|| {
            let fixture = Fixture::new();
            let report = connect(&fixture.request, true).unwrap();
            assert_eq!(report.status, "preview");
            assert_eq!(report.links, 11);
            assert!(!fixture.request.codex_home.exists());
            assert!(!fixture.request.user_home.exists());
            assert!(!fixture.request.dependency_user_home.exists());
            fs::write(fixture.request.build.join("codex.exe"), b"altered").unwrap();
            assert!(connect(&fixture.request, true).is_err());
            assert!(!fixture.request.codex_home.exists());
            fixture.record();
            fs::create_dir_all(&fixture.request.codex_home).unwrap();
            fs::write(fixture.request.codex_home.join("AGENTS.md"), b"foreign").unwrap();
            assert!(connect(&fixture.request, true).is_err());
            assert_eq!(
                fs::read(fixture.request.codex_home.join("AGENTS.md")).unwrap(),
                b"foreign"
            );
            assert!(!fixture.request.codex_home.join("harness").exists());
        });
    }

    #[test]
    fn diagnostic_source_observation_preserves_links_and_partial_evidence() {
        let fixture = Fixture::new();
        let request = &fixture.request;
        let observe = || {
            crate::source_observation::read(
                &request.codex_home,
                &request.user_home,
                &request.dependency_user_home,
                Some(&request.source),
            )
            .unwrap()
        };
        let absent = observe();
        assert!(
            absent
                .findings
                .iter()
                .any(|f| f["code"] == "installation-unavailable")
        );
        assert!(!absent.links.is_empty());
        assert!(!request.codex_home.exists());
        assert!(!request.user_home.exists());
        let inventory =
            inventory::read(&request.source, &request.codex_home, &request.user_home).unwrap();
        let mut links = Vec::new();
        for mut link in inventory.links {
            link.source = plain(&link.source).unwrap();
            fs::create_dir_all(link.destination.parent().unwrap()).unwrap();
            if link.source.is_dir() {
                std::os::windows::fs::symlink_dir(&link.source, &link.destination).unwrap();
            } else {
                std::os::windows::fs::symlink_file(&link.source, &link.destination).unwrap();
            }
            links.push(json!({"kind":link.kind,"name":link.name,"source":link.source,"destination":link.destination,"owned":true}));
        }
        let metadata = request.codex_home.join("harness/installation.json");
        fs::create_dir_all(metadata.parent().unwrap()).unwrap();
        let original = serde_json::to_vec(&json!({"schemaVersion":1,"sourceRoot":request.source,
            "codexHome":request.codex_home,"userHome":request.user_home,"dependencyUserHome":request.dependency_user_home,
            "codexCommand":request.upstream,"profileName":"harness","links":links,"pathScope":"Process","pathAdded":false,"versions":{}})).unwrap();
        fs::write(&metadata, &original).unwrap();
        let connected = observe();
        assert!(connected.findings.is_empty(), "{:?}", connected.findings);
        assert!(connected.links.iter().all(|l| l["status"] == "connected"));
        assert_eq!(connected.upstream, request.upstream);
        assert_eq!(fs::read(&metadata).unwrap(), original);
        let instructions = request.codex_home.join("AGENTS.md");
        let alternate = fixture.root.join("alternate.md");
        fs::write(&alternate, b"private contents are not diagnostic output").unwrap();
        fs::remove_file(&instructions).unwrap();
        std::os::windows::fs::symlink_file(&alternate, &instructions).unwrap();
        let conflicting = observe();
        assert!(
            conflicting
                .findings
                .iter()
                .any(|f| f["code"] == "link-retargeted")
        );
        assert_eq!(fs::read_link(&instructions).unwrap(), alternate);
        assert_eq!(fs::read(&metadata).unwrap(), original);
        let bad = b"PRIVATE_MALFORMED_METADATA_SENTINEL";
        fs::write(&metadata, bad).unwrap();
        let partial = observe();
        assert!(
            partial
                .findings
                .iter()
                .any(|f| f["code"] == "installation-unavailable")
        );
        assert!(
            partial
                .findings
                .iter()
                .any(|f| f["code"] == "link-retargeted")
        );
        assert!(
            !serde_json::to_string(&partial.findings)
                .unwrap()
                .contains("PRIVATE_MALFORMED")
        );
        assert_eq!(fs::read(&metadata).unwrap(), bad);
        assert_eq!(fs::read_link(instructions).unwrap(), alternate);
    }

    #[test]
    #[ignore = "explicit original CLI, Rust launcher and manager; owned process PATH and homes"]
    fn real_cli_upstream_discovery_preview_install_update_and_refusal() {
        use std::os::windows::fs::symlink_file;
        let mut fixture = Fixture::new();
        fixture.real();
        let request = &fixture.request;
        let manager = PathBuf::from(std::env::var_os("HARNESS_CORE_REAL_MANAGER").unwrap());
        let upstream = request.upstream.as_ref().unwrap();
        let expected = native_upstream::resolve(
            Some(upstream),
            None,
            &[],
            &native_upstream::ManagerHints::default(),
        )
        .unwrap();
        assert!(
            expected.package.is_some(),
            "this check requires the original managed package"
        );
        let prefix = fixture.root.join("original commands");
        fs::create_dir(&prefix).unwrap();
        symlink_file(upstream, prefix.join("codex.exe")).unwrap();
        let copied_harness = fixture.root.join("copied-harness.exe");
        fs::copy(request.build.join("codex.exe"), &copied_harness).unwrap();
        let user_path_before = UserPathSnapshot::read().unwrap().text().unwrap();
        let process_path_before = std::env::var_os("PATH");
        let invoke = |label: &str,
                      mode: &str,
                      preview: bool,
                      explicit: Option<&Path>,
                      paths: &[PathBuf],
                      success: bool| {
            let mut command = crate::process::CommandSpec::new(&manager);
            command.args = vec![
                mode.into(),
                "--core-only".into(),
                "--codex-home".into(),
                request.codex_home.as_os_str().into(),
                "--user-home".into(),
                request.user_home.as_os_str().into(),
                "--dependency-user-home".into(),
                request.dependency_user_home.as_os_str().into(),
            ];
            if matches!(mode, "install" | "update") {
                command.args.extend([
                    "--source".into(),
                    request.source.as_os_str().into(),
                    "--build".into(),
                    request.build.as_os_str().into(),
                    "--path-scope".into(),
                    "Process".into(),
                ]);
            }
            if preview {
                command.args.push("--preview".into());
            }
            if let Some(explicit) = explicit {
                command
                    .args
                    .extend(["--upstream".into(), explicit.as_os_str().into()]);
            }
            command
                .env
                .insert("PATH".into(), Some(std::env::join_paths(paths).unwrap()));
            command.env.insert("npm_config_user_agent".into(), None);
            command.env.insert("npm_execpath".into(), None);
            command.current_dir = Some(fixture.root.clone());
            let stdout = fixture.root.join(format!("{label}.stdout"));
            let result = crate::native_build::invoke_management(
                command,
                &fixture.root.join(format!("{label}.stderr")),
                Some(&stdout),
                Duration::from_secs(180),
            )
            .unwrap();
            assert_eq!(result.reason, crate::process::StopReason::Exited);
            assert_eq!(
                result.exit_code == 0,
                success,
                "{label}: evidence {}",
                fixture.root.display()
            );
            assert_eq!(std::env::var_os("PATH"), process_path_before);
            assert_eq!(
                UserPathSnapshot::read().unwrap().text().unwrap(),
                user_path_before
            );
            if success {
                let report: serde_json::Value =
                    serde_json::from_slice(&fs::read(stdout).unwrap()).unwrap();
                assert_eq!(report["model_calls"], 0);
                report
            } else {
                serde_json::Value::Null
            }
        };
        // A current harness build comes first; discovery must skip it. An
        // unrelated cwd has no bearing on selection of the package executable.
        let paths = [request.build.clone(), prefix];
        let preview = invoke("preview", "install", true, None, &paths, true);
        assert_eq!(preview["status"], "preview");
        assert!(!request.codex_home.exists());
        assert!(!request.user_home.exists());
        let installed = invoke("install", "install", false, None, &paths, true);
        assert_eq!(installed["status"], "connected");
        let launch_path = request.codex_home.join("harness/native-launch.json");
        let before_launch = fs::read(&launch_path).unwrap();
        let launch: native_launcher::Registration = serde_json::from_slice(&before_launch).unwrap();
        assert_eq!(launch.upstream.executable, expected.executable);
        assert_eq!(launch.upstream.sha256, expected.sha256);
        assert_eq!(
            launch.upstream.package.unwrap().root,
            expected.package.unwrap().root
        );
        let metadata = request.codex_home.join("harness/installation.json");
        let before_metadata = fs::read(&metadata).unwrap();
        // An explicit recursive selection is rejected before changing state.
        for preview in [true, false] {
            invoke(
                if preview {
                    "refuse-preview"
                } else {
                    "refuse-update"
                },
                "update",
                preview,
                Some(&copied_harness),
                &[],
                false,
            );
            assert_eq!(fs::read(&metadata).unwrap(), before_metadata);
            assert_eq!(fs::read(&launch_path).unwrap(), before_launch);
        }
        // A saved selection survives an absent original command in PATH.
        let updated = invoke("update", "update", false, None, &[], true);
        assert_eq!(updated["changed_links"], 0);
        assert_eq!(fs::read(&launch_path).unwrap(), before_launch);
        let updated_metadata = fs::read(&metadata).unwrap();
        invoke("check-missing-path", "check", false, None, &[], false);
        assert_eq!(fs::read(&metadata).unwrap(), updated_metadata);
        assert_eq!(
            invoke(
                "check",
                "check",
                false,
                None,
                &[request.codex_home.join("harness/bin")],
                true
            )["status"],
            "connected"
        );
        let alias = request
            .codex_home
            .join("harness/bin/codex-harness-check.exe");
        let mut diagnostic = crate::process::CommandSpec::new(&alias);
        diagnostic.args = vec![
            "--codex-home".into(),
            request.codex_home.as_os_str().into(),
            "--user-home".into(),
            request.user_home.as_os_str().into(),
            "--dependency-user-home".into(),
            request.dependency_user_home.as_os_str().into(),
            "--project".into(),
            fixture.root.as_os_str().into(),
        ];
        diagnostic.current_dir = Some(fixture.root.clone());
        let diagnostic_stdout = fixture.root.join("native-diagnostic-alias.stdout");
        let result = crate::native_build::invoke_management(
            diagnostic,
            &fixture.root.join("native-diagnostic-alias.stderr"),
            Some(&diagnostic_stdout),
            Duration::from_secs(120),
        )
        .unwrap();
        assert_eq!(
            result.exit_code,
            0,
            "diagnostic alias evidence: {}",
            fixture.root.display()
        );
        let diagnostic: serde_json::Value =
            serde_json::from_slice(&fs::read(diagnostic_stdout).unwrap()).unwrap();
        assert_eq!(diagnostic["native"]["status"], "observed", "{diagnostic}");
        assert_eq!(diagnostic["sourceRoot"], json!(request.source));
        assert_eq!(diagnostic["model_calls"], 0);
        assert_eq!(fs::read(&metadata).unwrap(), updated_metadata);
        assert_eq!(
            invoke("disconnect", "disconnect", false, None, &[], true)["status"],
            "disconnected"
        );
        assert!(!metadata.exists());
        assert!(!alias.exists());
        assert!(upstream.exists());
        assert!(request.source.join("skills/one/SKILL.md").exists());
        println!(
            "actual CLI upstream lifecycle evidence: {}",
            fixture.root.display()
        );
    }

    #[test]
    #[ignore = "explicit original CLI, Rust launcher and manager; owned process PATH and homes"]
    fn real_process_scope_supports_native_check_and_disconnect_without_registry_writes() {
        crate::process_path::with_test_environment(|| {
            let mut fixture = Fixture::new();
            fixture.real();
            fixture.request.path_scope = Some(PathScope::Process);
            let request = &fixture.request;
            let user_path_before = UserPathSnapshot::read().unwrap().text().unwrap();
            let process_path_before = std::env::var_os("PATH");
            let preview = connect(request, true).unwrap();
            assert!(preview.path_change);
            assert!(std::env::var_os("PATH") == process_path_before);
            assert!(!request.codex_home.exists());
            let installed = connect(request, false).unwrap();
            assert!(installed.path_change);
            assert!(installed.runtime.unwrap().passed);
            let installed_path = std::env::var_os("PATH");
            assert!(installed_path != process_path_before);
            let updated = connect(request, false).unwrap();
            assert!(!updated.path_change);
            assert_eq!(updated.changed_links, 0);
            // A removed manifest entry can already be absent on disk. Preview
            // preserves its old metadata; update explicitly retires that name.
            let obsolete = request.user_home.join(".agents/skills/one");
            fs::remove_dir(&obsolete).unwrap();
            fs::create_dir(request.source.join("empty-skills")).unwrap();
            let manifest_path = request.source.join("global/kit.json");
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
            manifest["skills"] = "empty-skills".into();
            fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            let metadata_path = request.codex_home.join("harness/installation.json");
            let before = fs::read(&metadata_path).unwrap();
            let preview = connect(request, true).unwrap();
            assert_eq!(preview.links, installed.links - 1);
            assert_eq!(fs::read(&metadata_path).unwrap(), before);
            let reconciled = connect(request, false).unwrap();
            assert_eq!(reconciled.links, installed.links - 1);
            let current = installation_metadata::InstallationMetadata::read(
                &request.codex_home,
                &request.user_home,
                &request.dependency_user_home,
            )
            .unwrap()
            .unwrap();
            assert!(
                !current
                    .links()
                    .iter()
                    .any(|link| link.object.path == obsolete)
            );
            assert!(!obsolete.exists());
            assert!(request.source.join("skills/one/SKILL.md").exists());
            let manager = PathBuf::from(
                std::env::var_os("HARNESS_CORE_REAL_MANAGER")
                    .expect("explicit current native manager required"),
            );
            for mode in ["check", "disconnect"] {
                let metadata_path = request.codex_home.join("harness/installation.json");
                let metadata_before = fs::read(&metadata_path).unwrap();
                let mut command = crate::process::CommandSpec::new(&manager);
                command.args = vec![
                    mode.into(),
                    "--core-only".into(),
                    "--codex-home".into(),
                    request.codex_home.as_os_str().into(),
                    "--user-home".into(),
                    request.user_home.as_os_str().into(),
                    "--dependency-user-home".into(),
                    request.dependency_user_home.as_os_str().into(),
                ];
                command.current_dir = Some(fixture.root.clone());
                let stdout = fixture.root.join(format!("process-{mode}.stdout"));
                let result = crate::native_build::invoke_management(
                    command,
                    &fixture.root.join(format!("process-{mode}.stderr")),
                    Some(&stdout),
                    Duration::from_secs(180),
                )
                .unwrap();
                assert_eq!(result.reason, crate::process::StopReason::Exited);
                assert_eq!(
                    result.exit_code,
                    0,
                    "process CLI evidence: {}",
                    fixture.root.display()
                );
                let report: serde_json::Value =
                    serde_json::from_slice(&fs::read(stdout).unwrap()).unwrap();
                assert_eq!(report["model_calls"], 0);
                if mode == "check" {
                    assert_eq!(report["status"], "connected");
                    assert_eq!(fs::read(&metadata_path).unwrap(), metadata_before);
                } else {
                    assert_eq!(report["status"], "disconnected");
                    assert!(report["path_change"].as_bool().unwrap());
                    assert!(!metadata_path.exists());
                }
                // A CLI child's process scope does not change its caller.
                assert!(std::env::var_os("PATH") == installed_path);
                assert!(UserPathSnapshot::read().unwrap().text().unwrap() == user_path_before);
            }
            assert!(request.source.join("skills/one/SKILL.md").exists());
            println!("real process PATH evidence: {}", fixture.root.display());
        });
    }

    #[test]
    #[ignore = "explicit complete source/build candidate and original CLI; owned homes and registry key only"]
    fn full_checkout_native_core_connect_check_update_and_disconnect() {
        crate::environment_path::with_test_registry(|| {
            let root = tempfile::Builder::new()
                .prefix("native-full-core-")
                .tempdir()
                .unwrap()
                .keep();
            let explicit = |key| {
                PathBuf::from(
                    std::env::var_os(key).expect("explicit full native acceptance input required"),
                )
            };
            let request = Request {
                source: explicit("HARNESS_CORE_FULL_SOURCE"),
                build: explicit("HARNESS_CORE_FULL_BUILD"),
                upstream: Some(explicit("HARNESS_CORE_REAL_UPSTREAM")),
                codex_home: root.join("codex"),
                user_home: root.join("user"),
                dependency_user_home: root.join("dependency"),
                timeout: Duration::from_secs(60),
                path_scope: None,
            };
            println!("full native core evidence: {}", root.display());
            let before_path = UserPathSnapshot::for_registration()
                .unwrap()
                .text()
                .unwrap();
            assert!(build_identity::check(&request.build, Some(&request.source)).runtime_allowed);
            let expected =
                inventory::read(&request.source, &request.codex_home, &request.user_home).unwrap();
            let preview = connect(&request, true).unwrap();
            assert_eq!(
                preview.links,
                expected.links.len() + build_identity::BINARIES.len() + 2
            );
            assert!(!request.codex_home.exists());
            let installed = connect(&request, false).unwrap();
            assert!(installed.runtime.as_ref().unwrap().passed);
            let metadata_path = request.codex_home.join("harness/installation.json");
            let metadata_before = fs::read(&metadata_path).unwrap();
            let checked = crate::core_check::check(
                &request.codex_home,
                &request.user_home,
                &request.dependency_user_home,
                request.timeout,
            )
            .unwrap();
            assert!(checked.runtime.passed);
            assert_eq!(fs::read(&metadata_path).unwrap(), metadata_before);
            let update = connect(&request, false).unwrap();
            assert_eq!(update.changed_links, 0);
            assert!(update.runtime.as_ref().unwrap().passed);
            let disconnected = crate::core_disconnect::disconnect(
                &request.codex_home,
                &request.user_home,
                &request.dependency_user_home,
                false,
            )
            .unwrap();
            assert_eq!(disconnected.removed_links, installed.links);
            assert_eq!(
                UserPathSnapshot::for_registration()
                    .unwrap()
                    .text()
                    .unwrap(),
                before_path
            );
            assert!(!metadata_path.exists());
            assert!(build_identity::check(&request.build, Some(&request.source)).runtime_allowed);
            fs::write(root.join("full-core-result.json"), serde_json::to_vec_pretty(&serde_json::json!({
                "schema":1,"passed":true,"model_calls":0,"source":request.source,"build":request.build,
                "core_data_links":expected.links.len(),"installed":installed,"check":checked,
                "update":update,"disconnect":disconnected,"global_activation":false,
            })).unwrap()).unwrap();
        });
    }

    #[test]
    #[ignore = "explicit original CLI and Rust launcher; owned homes and test registry only"]
    fn real_native_check_observes_connection_and_disconnect_preserves_user_path() {
        crate::environment_path::with_test_registry(|| {
            let mut fixture = Fixture::new();
            fixture.real();
            let request = &fixture.request;
            let path_before = UserPathSnapshot::for_registration()
                .unwrap()
                .text()
                .unwrap();
            connect(request, false).unwrap();
            let metadata = request.codex_home.join("harness/installation.json");
            let before = fs::read(&metadata).unwrap();
            let config = request.codex_home.join("config.toml");
            let config_before = fs::read(&config).unwrap();
            let result = crate::core_check::check(
                &request.codex_home,
                &request.user_home,
                &request.dependency_user_home,
                request.timeout,
            )
            .unwrap();
            assert_eq!(result.status, "connected");
            assert!(result.runtime.passed);
            assert_eq!(fs::read(&metadata).unwrap(), before);
            assert_eq!(fs::read(&config).unwrap(), config_before);
            fs::create_dir(request.source.join("skills/two")).unwrap();
            fs::write(request.source.join("skills/two/SKILL.md"), b"---\nname: two\ndescription: New manifest entry.\n---\nOwned missing-link scenario.\n").unwrap();
            let error = crate::core_check::check(
                &request.codex_home,
                &request.user_home,
                &request.dependency_user_home,
                request.timeout,
            )
            .err()
            .expect("new skill requires update");
            assert!(error.to_string().contains("not registered"), "{error}");
            assert_eq!(fs::read(&metadata).unwrap(), before);
            let disconnected = crate::core_disconnect::disconnect(
                &request.codex_home,
                &request.user_home,
                &request.dependency_user_home,
                false,
            )
            .unwrap();
            assert_eq!(disconnected.status, "disconnected");
            assert!(disconnected.path_change);
            assert_eq!(
                UserPathSnapshot::for_registration()
                    .unwrap()
                    .text()
                    .unwrap(),
                path_before
            );
            assert_eq!(fs::read(config).unwrap(), config_before);
            assert!(request.source.join("skills/two/SKILL.md").exists());
            assert!(!metadata.exists());
            println!("real check/disconnect evidence: {}", fixture.root.display());
        });
    }

    #[test]
    #[ignore = "explicit model-free original CLI and current Rust launcher; owned homes and test registry only"]
    fn real_core_install_repeat_and_runtime_failure_preserve_prior_connections() {
        crate::environment_path::with_test_registry(|| {
            let mut fixture = Fixture::new();
            fixture.real();
            println!("real native core evidence: {}", fixture.root.display());
            for _ in 0..3 {
                let report = connect(&fixture.request, false).unwrap();
                assert_eq!(report.status, "connected");
                assert!(report.runtime.unwrap().passed);
            }
            let metadata = fixture.request.codex_home.join("harness/installation.json");
            let before = fs::read(&metadata).unwrap();
            let source_profile = fixture.request.source.join("global/profile.toml");
            let valid = fs::read(&source_profile).unwrap();
            fs::write(&source_profile, b"malformed [").unwrap();
            assert!(connect(&fixture.request, false).is_err());
            assert_eq!(fs::read(&metadata).unwrap(), before);
            assert!(
                !fixture
                    .request
                    .codex_home
                    .join("harness/native-registration/journal.json")
                    .exists()
            );
            assert_eq!(fs::read(&source_profile).unwrap(), b"malformed [");
            fs::write(source_profile, valid).unwrap();
            for link in InstallationMetadata::read(
                &fixture.request.codex_home,
                &fixture.request.user_home,
                &fixture.request.dependency_user_home,
            )
            .unwrap()
            .unwrap()
            .links()
            {
                assert_eq!(
                    FileGuard::capture_link(&link.object.path)
                        .unwrap()
                        .object_identity()
                        .unwrap(),
                    link.object.identity
                );
            }
            let relocated = fixture.root.join("relocated источник");
            fs::rename(&fixture.request.source, &relocated).unwrap();
            fixture.request.source = relocated;
            fixture.record();
            let report = connect(&fixture.request, false).unwrap();
            assert!(report.runtime.unwrap().passed);
            assert_eq!(
                fs::read_link(fixture.request.codex_home.join("AGENTS.md")).unwrap(),
                fixture.request.source.join("global/instructions.md")
            );
        });
    }

    #[test]
    #[ignore = "explicit model-free original CLI; legacy ownership upgrade on owned homes and test registry"]
    fn real_core_import_preserves_adopted_profile_and_repairs_inherited_hooks() {
        use std::os::windows::fs::symlink_file;
        crate::environment_path::with_test_registry(|| {
            let mut fixture = Fixture::new();
            fixture.real();
            let request = &fixture.request;
            let home = &request.codex_home;
            fs::create_dir_all(home.join("harness/bin")).unwrap();
            for name in [
                "legacy-launcher.fixture",
                "legacy-hook.fixture",
                "legacy-diagnostic.fixture",
            ] {
                fs::write(
                    request.source.join(name),
                    b"inert legacy source; never executed",
                )
                .unwrap();
            }
            let entries = [
                (
                    "instructions",
                    "AGENTS",
                    "global/instructions.md",
                    "AGENTS.md",
                    true,
                ),
                (
                    "profile",
                    "harness",
                    "global/profile.toml",
                    "harness.config.toml",
                    false,
                ),
                (
                    "launcher",
                    "codex",
                    "legacy-launcher.fixture",
                    "harness/bin/codex.ps1",
                    true,
                ),
                ("hooks", "hooks", "global/hooks.json", "hooks.json", true),
                (
                    "diagnostic-launcher",
                    "codex-harness-check",
                    "legacy-diagnostic.fixture",
                    "harness/bin/codex-harness-check.ps1",
                    true,
                ),
                (
                    "hook-launcher",
                    "hook",
                    "legacy-hook.fixture",
                    "harness/bin/hook.ps1",
                    true,
                ),
            ];
            let mut links = Vec::new();
            for (kind, name, source, destination, owned) in entries {
                symlink_file(request.source.join(source), home.join(destination)).unwrap();
                links.push(json!({"kind":kind,"name":name,"source":request.source.join(source),"destination":home.join(destination),"owned":owned}));
            }
            let metadata = home.join("harness/installation.json");
            fs::write(&metadata, serde_json::to_vec(&json!({"schemaVersion":1,"sourceRoot":request.source,"codexHome":home,"userHome":request.user_home,"dependencyUserHome":request.dependency_user_home,"codexCommand":request.upstream,"profileName":"harness","links":links,"pathScope":"User","pathAdded":false,"versions":{}})).unwrap()).unwrap();
            let adopted = FileGuard::capture_link(&home.join("harness.config.toml"))
                .unwrap()
                .object_identity()
                .unwrap();
            println!("legacy native core evidence: {}", fixture.root.display());
            let legacy_metadata = fs::read(&metadata).unwrap();
            assert_eq!(connect(request, true).unwrap().status, "preview");
            assert_eq!(fs::read(&metadata).unwrap(), legacy_metadata);
            assert!(home.join("harness/bin/codex-harness-check.ps1").exists());
            assert!(connect(request, false).unwrap().runtime.unwrap().passed);
            assert!(!home.join("harness/bin/codex.ps1").exists());
            assert!(!home.join("harness/bin/codex-harness-check.ps1").exists());
            assert!(request.source.join("legacy-diagnostic.fixture").exists());
            assert_eq!(
                fs::read_link(home.join("harness/bin/codex-harness-check.exe")).unwrap(),
                request.build.join("codex-harness.exe")
            );
            let current =
                InstallationMetadata::read(home, &request.user_home, &request.dependency_user_home)
                    .unwrap()
                    .unwrap();
            let profile = current
                .links()
                .iter()
                .find(|l| l.kind == "profile")
                .unwrap();
            assert!(!profile.owned);
            assert_eq!(profile.object.identity, adopted);
            assert!(
                current
                    .links()
                    .iter()
                    .any(|link| link.kind == "native-diagnostic" && link.owned)
            );
            fs::remove_file(home.join("hooks.json")).unwrap();
            assert!(connect(request, false).unwrap().runtime.unwrap().passed);
            assert_eq!(
                fs::read_link(home.join("hooks.json")).unwrap(),
                request.source.join("global/hooks.json")
            );
            let before = fs::read(&metadata).unwrap();
            fs::remove_file(home.join("harness/bin/hook.ps1")).unwrap();
            fs::write(home.join("harness/bin/hook.ps1"), b"foreign replacement").unwrap();
            for preview in [true, false] {
                assert!(connect(request, preview).is_err());
            }
            assert_eq!(fs::read(&metadata).unwrap(), before);
            assert_eq!(
                fs::read(home.join("harness/bin/hook.ps1")).unwrap(),
                b"foreign replacement"
            );
        });
    }
}
