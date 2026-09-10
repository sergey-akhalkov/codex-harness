//! Stable installer ownership, generated from the registration builder's guarded
//! IDs. Lifecycle orchestration supplies the complete component/link selection.
#![cfg(windows)]
#![cfg_attr(not(test), allow(dead_code))]

use crate::{
    build_identity,
    config_file::ConfigSnapshot,
    installation_state::{LegacyInstallation, PathScope, key, normal, portable_name},
    inventory::{Link, ordinary_parents},
    registration::{
        LinkType,
        metadata::{MetadataDestination, MetadataLink, MetadataView},
    },
    registration_native::LinkIdentity,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

const SCHEMA: u32 = 2;
const MAX_BYTES: usize = 1024 * 1024;

pub(crate) struct InstallationOwners {
    codex_home: PathBuf,
    user_home: PathBuf,
    dependency_user_home: PathBuf,
}

impl InstallationOwners {
    pub(crate) fn new(
        codex_home: &Path,
        user_home: &Path,
        dependency_user_home: &Path,
    ) -> io::Result<Self> {
        Ok(Self {
            codex_home: normal(codex_home)?,
            user_home: normal(user_home)?,
            dependency_user_home: normal(dependency_user_home)?,
        })
    }

    pub(crate) fn metadata_path(&self) -> PathBuf {
        self.codex_home.join("harness/installation.json")
    }

    /// Bind recovery to the owners whose mutexes the core caller acquired.
    /// The caller supplies bytes and ID from the exact journal/commit guard;
    /// current metadata may still be absent or hold its pre-activation contents.
    pub(crate) fn validate_candidate(
        &self,
        bytes: &[u8],
        identity: &LinkIdentity,
    ) -> io::Result<()> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid());
        }
        let document: Document = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        document.validate()?;
        if &document.metadata_identity != identity
            || key(&document.settings.codex_home)? != key(&self.codex_home)?
            || key(&document.settings.user_home)? != key(&self.user_home)?
            || key(&document.settings.dependency_user_home)? != key(&self.dependency_user_home)?
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "native installation ownership is invalid or changed; preserving it",
    )
}

fn same_object(left: &MetadataLink, right: &MetadataLink) -> io::Result<bool> {
    Ok(left.identity == right.identity
        && left.link_type == right.link_type
        && crate::registration_native::targets_match(&left.target, &right.target)?)
}

fn prior_matches(view: &MetadataView, fingerprint: (LinkIdentity, String)) -> io::Result<()> {
    if view.identity != fingerprint.0
        || view.previous_metadata_sha256.as_deref() != Some(fingerprint.1.as_str())
    {
        return Err(invalid());
    }
    Ok(())
}

/// Machine-local installation choices; no credential material belongs here.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    pub source_root: PathBuf,
    pub codex_home: PathBuf,
    pub user_home: PathBuf,
    pub dependency_user_home: PathBuf,
    pub codex_command: PathBuf,
    pub path_scope: PathScope,
    pub path_added: bool,
    pub versions: BTreeMap<String, String>,
}

impl Settings {
    pub(crate) fn validate(&self) -> io::Result<()> {
        for path in [
            &self.source_root,
            &self.codex_home,
            &self.user_home,
            &self.dependency_user_home,
            &self.codex_command,
        ] {
            normal(path)?;
        }
        let source = key(&self.source_root)?;
        let home = key(&self.codex_home)?;
        if home == source
            || home.starts_with(&format!("{source}\\"))
            || self.versions.len() > 64
            || self.versions.iter().any(|(k, v)| {
                k.is_empty()
                    || k.len() > 128
                    || k.contains('\0')
                    || v.len() > 4096
                    || v.contains('\0')
            })
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn destination(&self, kind: &str, name: &str, source: &Path) -> io::Result<PathBuf> {
        if !portable_name(name) {
            return Err(invalid());
        }
        Ok(match kind {
            "instructions" => self.codex_home.join("AGENTS.md"),
            "profile" => self.codex_home.join("harness.config.toml"),
            "agents" => self.codex_home.join("agents/codex-harness"),
            "hooks" => self.codex_home.join("hooks.json"),
            "skill" => self
                .user_home
                .join(".agents/skills")
                .join(source.file_name().ok_or_else(invalid)?),
            // Transitional entries can be retained while the installer migrates
            // their consumers. They are not proof of completed script retirement.
            "launcher" => self.codex_home.join("harness/bin/codex.ps1"),
            "diagnostic-launcher" => self.codex_home.join("harness/bin/codex-harness-check.ps1"),
            "hook-launcher" => self.codex_home.join("harness/bin/hook.ps1"),
            "native-launch" if name == "codex" => {
                self.codex_home.join("harness/native-launch.json")
            }
            "native-diagnostic" if name == "codex-harness-check" => {
                self.codex_home.join("harness/bin/codex-harness-check.exe")
            }
            "native-command"
                if [
                    "codex",
                    "codex-harness",
                    "harness-rtk",
                    "harness-inspect",
                    "harness-observe",
                ]
                .contains(&name) =>
            {
                self.codex_home
                    .join("harness/bin")
                    .join(format!("{name}.exe"))
            }
            _ => return Err(invalid()),
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstalledLink {
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) owned: bool,
    pub(crate) object: MetadataLink,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema_version: u32,
    settings: Settings,
    metadata_identity: LinkIdentity,
    links: Vec<InstalledLink>,
    checksum: String,
}

impl Document {
    fn checksum(&self) -> io::Result<String> {
        Ok(build_identity::hash_bytes(&serde_json::to_vec(&(
            self.schema_version,
            &self.settings,
            &self.metadata_identity,
            &self.links,
        ))?))
    }

    fn validate(&self) -> io::Result<()> {
        self.settings.validate()?;
        if self.schema_version != SCHEMA
            || self.links.len() > 4096
            || self.checksum != self.checksum()?
        {
            return Err(invalid());
        }
        let mut destinations = BTreeSet::new();
        for link in &self.links {
            let expected =
                self.settings
                    .destination(&link.kind, &link.name, &link.object.target)?;
            let actual = key(&link.object.path)?;
            if key(&expected)? != actual || !destinations.insert(actual) {
                return Err(invalid());
            }
            let expected_type = if matches!(link.kind.as_str(), "skill" | "agents") {
                LinkType::Directory
            } else {
                LinkType::File
            };
            if link.object.link_type != expected_type {
                return Err(invalid());
            }
            normal(&link.object.target)?;
            ordinary_parents(&link.object.path)?;
            // Link source targets may be a retained old checkout, an adopted
            // literal alias, or a verified derived build. No target is deleted.
        }
        Ok(())
    }
}

/// The exact metadata snapshot is retained so a builder cannot bless a replaced
/// metadata file. Its embedded ID also rejects copies before planning begins.
pub(crate) struct InstallationMetadata {
    document: Document,
    snapshot: ConfigSnapshot,
}

impl InstallationMetadata {
    pub(crate) fn read(
        codex_home: &Path,
        user_home: &Path,
        dependency_user_home: &Path,
    ) -> io::Result<Option<Self>> {
        let path = normal(codex_home)?.join("harness/installation.json");
        ordinary_parents(&path)?;
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        let snapshot = ConfigSnapshot::read(&path)?;
        if snapshot.contents().len() > MAX_BYTES {
            return Err(invalid());
        }
        let document: Document =
            serde_json::from_slice(snapshot.contents()).map_err(|_| invalid())?;
        document.validate()?;
        let proof = snapshot.plan_replace(snapshot.contents())?;
        if &document.metadata_identity != proof.record.published_identity()
            || key(&document.settings.codex_home)? != key(codex_home)?
            || key(&document.settings.user_home)? != key(user_home)?
            || key(&document.settings.dependency_user_home)? != key(dependency_user_home)?
        {
            return Err(invalid());
        }
        proof.record.check_before()?;
        Ok(Some(Self { document, snapshot }))
    }

    pub(crate) fn destination(&self) -> io::Result<MetadataDestination> {
        MetadataDestination::existing(&self.snapshot)
    }

    pub(crate) fn snapshot(&self) -> &ConfigSnapshot {
        &self.snapshot
    }

    /// Disconnection preserves this exact owner witness until commit. Only
    /// recorded owned objects can occur in the producer's removal captures.
    pub(crate) fn disconnect_bytes(&self, view: &MetadataView) -> io::Result<Vec<u8>> {
        prior_matches(view, self.snapshot.fingerprint())?;
        if key(&view.path)? != key(self.snapshot.path())? || !view.links.is_empty() {
            return Err(invalid());
        }
        let mut used = BTreeSet::new();
        for captured in &view.previous {
            let path = key(&captured.path)?;
            if !used.insert(path.clone()) {
                return Err(invalid());
            }
            let owned = self
                .links()
                .iter()
                .find(|link| {
                    link.owned && key(&link.object.path).ok().as_deref() == Some(path.as_str())
                })
                .ok_or_else(invalid)?;
            if !same_object(captured, &owned.object)? {
                return Err(invalid());
            }
        }
        for link in self.links().iter().filter(|link| link.owned) {
            if !used.contains(&key(&link.object.path)?) {
                // An already absent owned link needs no deletion authority.
                // A foreign arrival is never silently added to removal intent.
                match fs::symlink_metadata(&link.object.path) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    _ => return Err(invalid()),
                }
            }
        }
        Ok(self.snapshot.contents().to_vec())
    }

    pub(crate) fn settings(&self) -> &Settings {
        &self.document.settings
    }

    pub(crate) fn links(&self) -> &[InstalledLink] {
        &self.document.links
    }

    pub(crate) fn verify_unchanged(&self) -> io::Result<()> {
        self.snapshot
            .plan_replace(self.snapshot.contents())?
            .record
            .check_before()
    }
}

pub(crate) enum Previous<'a> {
    Fresh,
    Legacy(&'a LegacyInstallation),
    Native(&'a InstallationMetadata),
}

/// Import only the owner witness needed to finish an explicit legacy
/// disconnection. Its reversible config record retains the original legacy
/// bytes; no adopted object is captured or claimed for deletion.
pub(crate) fn disconnect_legacy(
    legacy: &LegacyInstallation,
    view: &MetadataView,
) -> io::Result<Vec<u8>> {
    prior_matches(view, legacy.metadata_fingerprint())?;
    let settings = legacy.native_settings();
    if key(&view.path)? != key(&settings.codex_home.join("harness/installation.json"))?
        || !view.links.is_empty()
    {
        return Err(invalid());
    }
    let mut used = BTreeSet::new();
    for captured in &view.previous {
        let path = key(&captured.path)?;
        if !used.insert(path.clone()) {
            return Err(invalid());
        }
        let owned = legacy
            .links()
            .iter()
            .find(|link| {
                link.owned && key(&link.destination).ok().as_deref() == Some(path.as_str())
            })
            .ok_or_else(invalid)?;
        let directory = matches!(owned.kind.as_str(), "skill" | "agents");
        if key(&owned.source)? != key(&captured.target)?
            || directory != (captured.link_type == LinkType::Directory)
        {
            return Err(invalid());
        }
    }
    for link in legacy.links().iter().filter(|link| link.owned) {
        if !used.contains(&key(&link.destination)?) {
            match fs::symlink_metadata(&link.destination) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                _ => return Err(invalid()),
            }
        }
    }
    let mut document = Document {
        schema_version: SCHEMA,
        settings,
        metadata_identity: view.identity.clone(),
        links: Vec::new(),
        checksum: String::new(),
    };
    document.checksum = document.checksum()?;
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(&document)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid());
    }
    Ok(bytes)
}

/// Called only within `apply_with_metadata`, while its old/new IDs are guarded.
/// Every desired destination must occur in the final view. Existing objects are
/// adopted unless a matching verified previous claim establishes ownership.
pub(crate) fn encode(
    settings: &Settings,
    desired: &[Link],
    view: &MetadataView,
    previous: Previous<'_>,
) -> io::Result<Vec<u8>> {
    encode_retiring_absent(settings, desired, view, previous, &[])
}

/// Explicitly retire obsolete owned names which already have no object. This
/// grants no deletion authority. Ordinary component plans keep the stricter
/// default above; every nomination must match one omitted prior owned entry.
pub(crate) fn encode_retiring_absent(
    settings: &Settings,
    desired: &[Link],
    view: &MetadataView,
    previous: Previous<'_>,
    retired_absent: &[PathBuf],
) -> io::Result<Vec<u8>> {
    settings.validate()?;
    let mut retirements = retired_absent
        .iter()
        .map(|path| key(path))
        .collect::<io::Result<BTreeSet<_>>>()?;
    if retirements.len() != retired_absent.len() || retirements.len() > 4096 {
        return Err(invalid());
    }
    if key(&view.path)? != key(&settings.codex_home.join("harness/installation.json"))?
        || desired.len() != view.links.len()
        || desired.len() > 4096
    {
        return Err(invalid());
    }
    let previous_objects: BTreeMap<_, _> = view
        .previous
        .iter()
        .map(|link| Ok((key(&link.path)?, link)))
        .collect::<io::Result<_>>()?;
    if previous_objects.len() != view.previous.len() {
        return Err(invalid());
    }
    let final_paths = view
        .links
        .iter()
        .map(|link| key(&link.path))
        .collect::<io::Result<BTreeSet<_>>>()?;
    // The registration producer puts reused/replaced objects in `previous`.
    // A final object without a previous capture is a newly guarded stage.
    // This permits repair with its new ID; it never permits silently omitting
    // a recorded owned destination or acting on a present foreign replacement.
    let mut ownership = BTreeMap::new();
    match previous {
        Previous::Fresh => {
            if view.previous_metadata_sha256.is_some() {
                return Err(invalid());
            }
        }
        Previous::Legacy(legacy) => {
            prior_matches(view, legacy.metadata_fingerprint())?;
            let header = legacy.summary();
            if key(header.codex_home)? != key(&settings.codex_home)?
                || key(header.user_home)? != key(&settings.user_home)?
                || key(header.dependency_user_home)? != key(&settings.dependency_user_home)?
            {
                return Err(invalid());
            }
            for link in legacy.links() {
                let path = key(&link.destination)?;
                if let Some(captured) = previous_objects.get(&path) {
                    if key(&captured.target)? != key(&link.source)? {
                        return Err(invalid());
                    }
                    ownership.insert(path, link.owned);
                } else if link.owned && !final_paths.contains(&path) {
                    require_retired_absent(&link.destination, &mut retirements)?;
                }
            }
        }
        Previous::Native(native) => {
            prior_matches(view, native.snapshot.fingerprint())?;
            let header = native.settings();
            if key(&header.codex_home)? != key(&settings.codex_home)?
                || key(&header.user_home)? != key(&settings.user_home)?
                || key(&header.dependency_user_home)? != key(&settings.dependency_user_home)?
            {
                return Err(invalid());
            }
            for link in &native.document.links {
                let path = key(&link.object.path)?;
                if let Some(captured) = previous_objects.get(&path) {
                    if !same_object(captured, &link.object)? {
                        return Err(invalid());
                    }
                    ownership.insert(path, link.owned);
                } else if link.owned && !final_paths.contains(&path) {
                    require_retired_absent(&link.object.path, &mut retirements)?;
                }
            }
        }
    }
    if !retirements.is_empty() {
        return Err(invalid());
    }
    let mut links = Vec::new();
    let mut used = BTreeSet::new();
    for descriptor in desired {
        let path = key(&descriptor.destination)?;
        if !used.insert(path.clone()) {
            return Err(invalid());
        }
        let object = view
            .links
            .iter()
            .find(|link| key(&link.path).ok().as_deref() == Some(path.as_str()))
            .ok_or_else(invalid)?;
        // Replacing an adopted link is not authorized by possession of its
        // target alone. A removal of an adopted entry is refused below too.
        if ownership.get(&path) != Some(&true)
            && let Some(old) = previous_objects.get(&path)
            && !same_object(old, object)?
        {
            return Err(invalid());
        }
        let owned = ownership
            .get(&path)
            .copied()
            .unwrap_or(!previous_objects.contains_key(&path));
        links.push(InstalledLink {
            kind: descriptor.kind.clone(),
            name: descriptor.name.clone(),
            owned,
            object: object.clone(),
        });
    }
    for path in previous_objects.keys() {
        if ownership.get(path) != Some(&true) && !used.contains(path) {
            return Err(invalid());
        }
    }
    links.sort_by(|a, b| a.object.path.cmp(&b.object.path));
    let mut document = Document {
        schema_version: SCHEMA,
        settings: settings.clone(),
        metadata_identity: view.identity.clone(),
        links,
        checksum: String::new(),
    };
    document.checksum = document.checksum()?;
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(&document)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid());
    }
    Ok(bytes)
}

fn require_retired_absent(path: &Path, retirements: &mut BTreeSet<String>) -> io::Result<()> {
    if !retirements.remove(&key(path)?) {
        return Err(invalid());
    }
    crate::inventory::ordinary_parents(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inventory::Connection,
        registration::{JOURNAL, LinkChange, Registration},
        registration_native::FileGuard,
    };
    use std::os::windows::fs::{symlink_dir, symlink_file};

    struct Fixture {
        root: PathBuf,
        settings: Settings,
        links: Vec<Link>,
    }

    // Snapshot only these owned fixtures, without following source links.
    fn recovery_image(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(path: &Path, image: &mut BTreeMap<PathBuf, Vec<u8>>) {
            let metadata = match fs::symlink_metadata(path) {
                Ok(value) => value,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return,
                Err(error) => panic!("fixture snapshot: {error}"),
            };
            let bytes = if metadata.is_symlink() {
                let guard = FileGuard::capture_link(path).unwrap();
                serde_json::to_vec(&(
                    guard.object_identity().unwrap(),
                    guard.link_description().unwrap(),
                ))
                .unwrap()
            } else if metadata.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    visit(&entry.unwrap().path(), image);
                }
                b"directory".to_vec()
            } else {
                let (guard, bytes) = FileGuard::read_regular(path).unwrap();
                serde_json::to_vec(&(guard.object_identity().unwrap(), bytes)).unwrap()
            };
            image.insert(path.to_path_buf(), bytes);
        }
        let mut image = BTreeMap::new();
        visit(root, &mut image);
        image
    }

    fn assert_recovery_preview(fixture: &Fixture, action: &str) {
        let before = recovery_image(&fixture.root);
        let process_path = std::env::var_os("PATH");
        let user_path = crate::environment_path::UserPathSnapshot::for_registration()
            .unwrap()
            .text()
            .unwrap();
        for _ in 0..2 {
            let report = crate::core_install::preview_recovery(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home,
            )
            .unwrap();
            assert_eq!(report.status, "preview");
            assert_eq!(report.action, action);
            assert_eq!(report.journal, (action != "none").then_some("native"));
            assert_eq!(report.model_calls, 0);
            assert!(
                serde_json::to_value(report)
                    .unwrap()
                    .get("committed")
                    .is_none()
            );
            assert_eq!(before, recovery_image(&fixture.root));
            assert!(std::env::var_os("PATH") == process_path);
            assert!(
                crate::environment_path::UserPathSnapshot::for_registration()
                    .unwrap()
                    .text()
                    .unwrap()
                    == user_path
            );
        }
    }

    fn legacy_fixture(fixture: &Fixture) -> LegacyInstallation {
        fixture.publish_fresh();
        fs::create_dir_all(fixture.settings.codex_home.join("harness/bin")).unwrap();
        let launcher_source = fixture.settings.source_root.join("inert-old-launcher.txt");
        fs::write(
            &launcher_source,
            b"inert legacy launcher target; never executed",
        )
        .unwrap();
        let launcher = fixture.settings.codex_home.join("harness/bin/codex.ps1");
        symlink_file(&launcher_source, &launcher).unwrap();
        let mut links = fixture
            .links
            .iter()
            .map(|link| {
                serde_json::json!({
                    "kind":link.kind,"name":link.name,"source":link.source,
                    "destination":link.destination,"owned":link.kind!="profile",
                })
            })
            .collect::<Vec<_>>();
        links.push(serde_json::json!({"kind":"launcher","name":"codex","source":launcher_source,"destination":launcher,"owned":true}));
        let settings = &fixture.settings;
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion":1,"sourceRoot":settings.source_root,"codexHome":settings.codex_home,
            "userHome":settings.user_home,"dependencyUserHome":settings.dependency_user_home,
            "codexCommand":settings.codex_command,"profileName":"harness","links":links,
            "pathScope":"User","pathAdded":false,"versions":{},
        }))
        .unwrap();
        fs::write(fixture.path(), bytes).unwrap();
        LegacyInstallation::read(
            &settings.codex_home,
            &settings.user_home,
            &settings.dependency_user_home,
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn legacy_disconnect_preserves_adopted_and_unavailable_source_without_upgrade() {
        crate::environment_path::with_test_registry(|| {
            for case in [
                "healthy",
                "missing",
                "dangling",
                "foreign-owned",
                "changed-adopted",
            ] {
                let fixture = Fixture::new();
                let legacy = legacy_fixture(&fixture);
                let agent = fixture.settings.codex_home.join("AGENTS.md");
                let profile = fixture.settings.codex_home.join("harness.config.toml");
                if case == "missing" {
                    fs::remove_file(&agent).unwrap();
                }
                if case == "dangling" {
                    fs::rename(
                        &fixture.settings.source_root,
                        fixture.root.join("moved-source"),
                    )
                    .unwrap();
                }
                if case == "foreign-owned" {
                    fs::remove_file(&agent).unwrap();
                    fs::write(&agent, b"foreign owner").unwrap();
                }
                if case == "changed-adopted" {
                    fs::remove_file(&profile).unwrap();
                    fs::write(&profile, b"adopted edit").unwrap();
                }
                let run = |preview| {
                    crate::core_disconnect::disconnect(
                        &fixture.settings.codex_home,
                        &fixture.settings.user_home,
                        &fixture.settings.dependency_user_home,
                        preview,
                    )
                };
                let before = fs::read(fixture.path()).unwrap();
                if case == "foreign-owned" {
                    assert!(run(true).is_err());
                    assert!(run(false).is_err());
                    assert_eq!(fs::read(fixture.path()).unwrap(), before);
                    assert_eq!(fs::read(&agent).unwrap(), b"foreign owner");
                    continue;
                }
                assert_eq!(
                    run(true).unwrap().removed_links,
                    if case == "missing" { 2 } else { 3 }
                );
                assert_eq!(fs::read(fixture.path()).unwrap(), before);
                legacy.verify_unchanged().unwrap();
                assert_eq!(run(false).unwrap().status, "disconnected");
                assert!(!fixture.path().exists());
                for link in legacy.links().iter().filter(|link| link.owned) {
                    assert!(fs::symlink_metadata(&link.destination).is_err());
                }
                assert!(fs::symlink_metadata(profile).is_ok());
                let source = if case == "dangling" {
                    fixture.root.join("moved-source")
                } else {
                    fixture.settings.source_root.clone()
                };
                assert_eq!(
                    fs::read(source.join("skill-one/keep")).unwrap(),
                    b"source skill stays"
                );
                assert!(source.join("inert-old-launcher.txt").exists());
            }
        });
    }

    #[test]
    fn legacy_disconnect_recovery_restores_original_format_or_finishes_owner_bound_cleanup() {
        for phase in ["partial", "completed", "decision", "backup", "journal"] {
            let fixture = Fixture::new();
            let legacy = legacy_fixture(&fixture);
            let before = fs::read(fixture.path()).unwrap();
            let state = fixture
                .settings
                .codex_home
                .join("harness/native-registration");
            let reg = Registration::open(&state).unwrap();
            let changes = legacy
                .links()
                .iter()
                .filter(|link| link.owned)
                .map(|link| LinkChange::remove(&link.destination, &link.source).unwrap())
                .collect::<Vec<_>>();
            let result = reg.apply_disconnection(
                &changes,
                legacy.snapshot(),
                |view| disconnect_legacy(&legacy, view),
                None,
                || {
                    if phase == "partial" {
                        Err(io::Error::other("owned interrupted legacy disconnect"))
                    } else {
                        Ok(())
                    }
                },
            );
            assert_eq!(result.is_ok(), phase != "partial");
            if !["partial", "completed"].contains(&phase) {
                assert!(
                    reg.test_finish(&fixture.path(), |at, index| if at == phase && index == 0 {
                        Err(io::Error::other("owned finish stop"))
                    } else {
                        Ok(())
                    })
                    .is_err()
                );
            }
            drop(reg);
            let pending_metadata = fs::read(fixture.path()).ok();
            let pending_journal = fs::read(state.join(JOURNAL)).ok();
            assert!(
                crate::core_install::recover(
                    &fixture.settings.codex_home,
                    &fixture.root.join("wrong-owner"),
                    &fixture.settings.dependency_user_home
                )
                .is_err()
            );
            assert_eq!(fs::read(fixture.path()).ok(), pending_metadata);
            assert_eq!(fs::read(state.join(JOURNAL)).ok(), pending_journal);
            let report = crate::core_install::recover(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home,
            )
            .unwrap();
            assert_eq!(report.committed, phase != "partial");
            if phase == "partial" {
                assert_eq!(fs::read(fixture.path()).unwrap(), before);
                legacy.verify_unchanged().unwrap();
                for link in legacy.links() {
                    assert!(link.destination.is_symlink());
                }
            } else {
                assert!(!fixture.path().exists());
            }
            assert!(
                fixture
                    .settings
                    .codex_home
                    .join("harness.config.toml")
                    .is_symlink()
            );
            assert_eq!(
                fs::read(fixture.settings.source_root.join("skill-one/keep")).unwrap(),
                b"source skill stays"
            );
            assert!(!state.join(JOURNAL).exists());
            assert!(!state.join(crate::registration::COMMIT).exists());
        }
    }

    #[test]
    fn native_disconnect_preserves_adopted_and_source_handles_missing_and_dangling() {
        crate::environment_path::with_test_registry(|| {
            for case in [
                "healthy",
                "missing",
                "dangling",
                "foreign-owned",
                "changed-adopted",
            ] {
                let fixture = Fixture::new();
                fixture.publish_fresh();
                let original = fixture.read();
                let agent = fixture.settings.codex_home.join("AGENTS.md");
                let profile = fixture.settings.codex_home.join("harness.config.toml");
                if case == "missing" {
                    fs::remove_file(&agent).unwrap();
                }
                if case == "dangling" {
                    fs::rename(
                        &fixture.settings.source_root,
                        fixture.root.join("moved-source"),
                    )
                    .unwrap();
                }
                if case == "foreign-owned" {
                    fs::remove_file(&agent).unwrap();
                    fs::write(&agent, b"foreign owner").unwrap();
                }
                if case == "changed-adopted" {
                    fs::remove_file(&profile).unwrap();
                    fs::write(&profile, b"adopted user edit").unwrap();
                }
                let run = |preview| {
                    crate::core_disconnect::disconnect(
                        &fixture.settings.codex_home,
                        &fixture.settings.user_home,
                        &fixture.settings.dependency_user_home,
                        preview,
                    )
                };
                let metadata_before = fs::read(fixture.path()).unwrap();
                if case == "foreign-owned" {
                    assert!(run(true).is_err());
                    assert!(run(false).is_err());
                    assert_eq!(fs::read(fixture.path()).unwrap(), metadata_before);
                    assert_eq!(fs::read(&agent).unwrap(), b"foreign owner");
                    continue;
                }
                let preview = run(true).unwrap();
                assert_eq!(preview.removed_links, if case == "missing" { 1 } else { 2 });
                assert_eq!(fs::read(fixture.path()).unwrap(), metadata_before);
                original.verify_unchanged().unwrap();
                assert_eq!(run(false).unwrap().status, "disconnected");
                assert!(!fixture.path().exists());
                assert!(fs::symlink_metadata(&agent).is_err());
                assert!(
                    fs::symlink_metadata(
                        fixture.settings.user_home.join(".agents/skills/skill-one")
                    )
                    .is_err()
                );
                assert!(fs::symlink_metadata(&profile).is_ok());
                let source = if case == "dangling" {
                    fixture.root.join("moved-source")
                } else {
                    fixture.settings.source_root.clone()
                };
                assert_eq!(
                    fs::read(source.join("skill-one/keep")).unwrap(),
                    b"source skill stays"
                );
                assert_eq!(run(false).unwrap().status, "not-connected");
            }
        });
    }

    #[test]
    fn disconnected_metadata_recovery_rolls_back_before_acceptance_and_retires_after_commit() {
        for phase in [
            "partial",
            "completed",
            "committed",
            "metadata-retired",
            "journal-retired",
        ] {
            let fixture = Fixture::new();
            fixture.publish_fresh();
            let metadata = fixture.read();
            let state = fixture
                .settings
                .codex_home
                .join("harness/native-registration");
            let reg = Registration::open(&state).unwrap();
            let changes = metadata
                .links()
                .iter()
                .filter(|link| link.owned)
                .map(|link| LinkChange::remove(&link.object.path, &link.object.target).unwrap())
                .collect::<Vec<_>>();
            let applied = reg.apply_disconnection(
                &changes,
                metadata.snapshot(),
                |view| metadata.disconnect_bytes(view),
                None,
                || {
                    if phase == "partial" {
                        Err(io::Error::other("owned pre-acceptance interruption"))
                    } else {
                        Ok(())
                    }
                },
            );
            assert_eq!(applied.is_ok(), phase != "partial");
            if !["partial", "completed"].contains(&phase) {
                let boundary = match phase {
                    "committed" => "decision",
                    "metadata-retired" => "backup",
                    _ => "journal",
                };
                assert!(
                    reg.test_finish(&fixture.path(), |at, index| {
                        if at == boundary && index == 0 {
                            Err(io::Error::other("owned retirement interruption"))
                        } else {
                            Ok(())
                        }
                    })
                    .is_err()
                );
            }
            drop(reg);
            let current_metadata = fs::read(fixture.path()).ok();
            let commit_before = fs::read(state.join(crate::registration::COMMIT)).ok();
            assert_recovery_preview(
                &fixture,
                if phase == "partial" {
                    "rollback"
                } else {
                    "finish"
                },
            );
            assert!(
                crate::core_install::recover(
                    &fixture.settings.codex_home,
                    &fixture.root.join("wrong-owner"),
                    &fixture.settings.dependency_user_home
                )
                .is_err()
            );
            assert_eq!(fs::read(fixture.path()).ok(), current_metadata);
            assert_eq!(
                fs::read(state.join(crate::registration::COMMIT)).ok(),
                commit_before
            );
            let result = crate::core_install::recover(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home,
            )
            .unwrap();
            assert_eq!(result.committed, phase != "partial");
            if phase == "partial" {
                fixture.assert_objects(&metadata);
            } else {
                assert!(!fixture.path().exists());
                assert!(
                    fs::symlink_metadata(fixture.settings.codex_home.join("AGENTS.md")).is_err()
                );
            }
            assert!(
                fixture
                    .settings
                    .codex_home
                    .join("harness.config.toml")
                    .is_symlink()
            );
            assert!(!state.join(JOURNAL).exists());
            assert!(!state.join(crate::registration::COMMIT).exists());
        }
    }

    #[test]
    fn foreign_metadata_replacement_blocks_disconnection_cleanup() {
        for committed in [false, true] {
            let fixture = Fixture::new();
            fixture.publish_fresh();
            let metadata = fixture.read();
            let reg = fixture.registration();
            let changes = metadata
                .links()
                .iter()
                .filter(|link| link.owned)
                .map(|link| LinkChange::remove(&link.object.path, &link.object.target).unwrap())
                .collect::<Vec<_>>();
            reg.apply_disconnection(
                &changes,
                metadata.snapshot(),
                |view| metadata.disconnect_bytes(view),
                None,
                || Ok(()),
            )
            .unwrap();
            if committed {
                assert!(
                    reg.test_finish(&fixture.path(), |at, _| if at == "decision" {
                        Err(io::Error::other("owned stop"))
                    } else {
                        Ok(())
                    })
                    .is_err()
                );
            }
            let bytes = fs::read(fixture.path()).unwrap();
            fs::rename(fixture.path(), fixture.root.join("saved-metadata")).unwrap();
            fs::write(fixture.path(), &bytes).unwrap();
            assert!(reg.finish(&fixture.path()).is_err());
            assert!(reg.recover().is_err());
            assert_eq!(fs::read(fixture.path()).unwrap(), bytes);
            assert!(fixture.root.join("saved-metadata").exists());
            assert!(fixture.root.join("journal-state").join(JOURNAL).exists());
        }
    }

    #[test]
    #[ignore = "explicit native manager on owned paths; installation owns no PATH entry"]
    fn native_manager_disconnects_native_and_legacy_installations_with_preview_and_owner_refusal() {
        use crate::process::{CommandSpec, StopReason};
        let manager = PathBuf::from(
            std::env::var_os("HARNESS_CORE_REAL_MANAGER")
                .expect("explicit native manager required"),
        );
        for legacy in [false, true] {
            let fixture = Fixture::new();
            if legacy {
                legacy_fixture(&fixture);
            } else {
                fixture.publish_fresh();
            }
            let metadata = fs::read(fixture.path()).unwrap();
            for phase in ["wrong-owner", "preview", "disconnect", "again"] {
                let user = if phase == "wrong-owner" {
                    fixture.root.join("wrong-owner")
                } else {
                    fixture.settings.user_home.clone()
                };
                let mut command = CommandSpec::new(&manager);
                command.args = vec![
                    "disconnect".into(),
                    "--core-only".into(),
                    "--codex-home".into(),
                    fixture.settings.codex_home.as_os_str().into(),
                    "--user-home".into(),
                    user.into_os_string(),
                    "--dependency-user-home".into(),
                    fixture.settings.dependency_user_home.as_os_str().into(),
                ];
                if phase == "preview" {
                    command.args.push("--preview".into());
                }
                command.current_dir = Some(fixture.root.clone());
                let stdout = fixture.root.join(format!("disconnect-{phase}.stdout"));
                let outcome = crate::native_build::invoke_management(
                    command,
                    &fixture.root.join(format!("disconnect-{phase}.stderr")),
                    Some(&stdout),
                    std::time::Duration::from_secs(30),
                )
                .unwrap();
                assert_eq!(outcome.reason, StopReason::Exited);
                if phase == "wrong-owner" {
                    assert_ne!(outcome.exit_code, 0);
                } else {
                    assert_eq!(
                        outcome.exit_code,
                        0,
                        "CLI evidence: {}",
                        fixture.root.display()
                    );
                    let report: serde_json::Value =
                        serde_json::from_slice(&fs::read(stdout).unwrap()).unwrap();
                    assert_eq!(report["model_calls"], 0);
                    assert_eq!(report["path_change"], false);
                    assert_eq!(
                        report["status"],
                        match phase {
                            "preview" => "preview",
                            "disconnect" => "disconnected",
                            _ => "not-connected",
                        }
                    );
                }
                if ["wrong-owner", "preview"].contains(&phase) {
                    assert_eq!(fs::read(fixture.path()).unwrap(), metadata);
                    assert!(fixture.settings.codex_home.join("AGENTS.md").is_symlink());
                } else {
                    assert!(!fixture.path().exists());
                    assert!(
                        fs::symlink_metadata(fixture.settings.codex_home.join("AGENTS.md"))
                            .is_err()
                    );
                }
                assert!(
                    fixture
                        .settings
                        .codex_home
                        .join("harness.config.toml")
                        .is_symlink()
                );
                assert_eq!(
                    fs::read(fixture.settings.source_root.join("skill-one/keep")).unwrap(),
                    b"source skill stays"
                );
            }
        }
    }

    #[test]
    #[ignore = "owned child of killed_native_disconnection_recovers_without_destructors"]
    fn disconnection_process_fixture() {
        let root = PathBuf::from(std::env::var_os("HARNESS_DISCONNECTION_ROOT").unwrap());
        let requested = std::env::var("HARNESS_DISCONNECTION_PHASE").unwrap();
        let settings: Settings =
            serde_json::from_slice(&fs::read(root.join("disconnect-input.json")).unwrap()).unwrap();
        let metadata = InstallationMetadata::read(
            &settings.codex_home,
            &settings.user_home,
            &settings.dependency_user_home,
        )
        .unwrap()
        .unwrap();
        let reg =
            Registration::open(&settings.codex_home.join("harness/native-registration")).unwrap();
        let pause = |at: &str| {
            if at == requested {
                fs::write(root.join("paused"), at.as_bytes()).unwrap();
                std::thread::sleep(std::time::Duration::from_secs(20));
                std::process::exit(79);
            }
        };
        let changes = metadata
            .links()
            .iter()
            .filter(|link| link.owned)
            .map(|link| LinkChange::remove(&link.object.path, &link.object.target).unwrap())
            .collect::<Vec<_>>();
        let path = if settings.path_scope == PathScope::Process {
            let bin = settings.codex_home.join("harness/bin");
            let (install, added) = crate::process_path::ProcessPathSnapshot::read()
                .unwrap()
                .prepend(&bin)
                .unwrap();
            assert!(added);
            install.publish().unwrap();
            Some(
                crate::installation_path::PathChange::remove(PathScope::Process, &bin, true)
                    .unwrap(),
            )
        } else {
            None
        };
        reg.apply_disconnection(
            &changes,
            metadata.snapshot(),
            |view| metadata.disconnect_bytes(view),
            path,
            || {
                pause("partial");
                Ok(())
            },
        )
        .unwrap();
        pause("completed");
        reg.test_finish(metadata.snapshot().path(), |at, index| {
            if index == 0 {
                pause(at);
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn killed_native_disconnection_recovers_without_destructors() {
        killed_disconnection_cases(PathScope::User);
    }

    #[test]
    fn skill_descriptor_name_is_independent_of_its_source_and_destination_folder() {
        crate::environment_path::with_test_registry(|| {
            let mut fixture = Fixture::new();
            fixture
                .links
                .iter_mut()
                .find(|link| link.kind == "skill")
                .unwrap()
                .name = "different-descriptor-name".into();
            fixture.publish_fresh();
            let metadata = fixture.read();
            assert_eq!(
                metadata
                    .links()
                    .iter()
                    .find(|link| link.kind == "skill")
                    .unwrap()
                    .name,
                "different-descriptor-name"
            );
            let report = crate::core_disconnect::disconnect(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home,
                false,
            )
            .unwrap();
            assert_eq!(report.status, "disconnected");
            assert!(
                !fixture
                    .settings
                    .user_home
                    .join(".agents/skills/skill-one")
                    .exists()
            );
            assert_eq!(
                fs::read(fixture.settings.source_root.join("skill-one/keep")).unwrap(),
                b"source skill stays"
            );
        });
    }

    #[test]
    fn process_path_disconnection_restores_or_commits_same_process_environment() {
        use crate::{installation_path::PathChange, process_path::ProcessPathSnapshot};
        crate::process_path::with_test_environment(|| {
            for phase in [
                "direct",
                "partial",
                "completed",
                "decision",
                "backup",
                "journal",
                "foreign",
            ] {
                let before_path = std::env::var_os("PATH");
                let mut fixture = Fixture::new();
                fixture.settings.path_scope = PathScope::Process;
                fixture.settings.path_added = true;
                let bin = fixture.settings.codex_home.join("harness/bin");
                let (install, added) = ProcessPathSnapshot::read().unwrap().prepend(&bin).unwrap();
                assert!(added);
                install.publish().unwrap();
                let installed_path = std::env::var_os("PATH");
                fixture.publish_fresh();
                let metadata = fixture.read();
                if phase == "direct" {
                    let run = |preview| {
                        crate::core_disconnect::disconnect(
                            &fixture.settings.codex_home,
                            &fixture.settings.user_home,
                            &fixture.settings.dependency_user_home,
                            preview,
                        )
                    };
                    assert!(run(true).unwrap().path_change);
                    metadata.verify_unchanged().unwrap();
                    assert!(std::env::var_os("PATH") == installed_path);
                    assert!(run(false).unwrap().path_change);
                    assert!(std::env::var_os("PATH") == before_path);
                    assert!(!fixture.path().exists());
                    continue;
                }
                let state = fixture
                    .settings
                    .codex_home
                    .join("harness/native-registration");
                let reg = Registration::open(&state).unwrap();
                let changes = metadata
                    .links()
                    .iter()
                    .filter(|link| link.owned)
                    .map(|link| LinkChange::remove(&link.object.path, &link.object.target).unwrap())
                    .collect::<Vec<_>>();
                let applied = reg.apply_disconnection(
                    &changes,
                    metadata.snapshot(),
                    |view| metadata.disconnect_bytes(view),
                    Some(PathChange::remove(PathScope::Process, &bin, true).unwrap()),
                    || {
                        if phase == "partial" {
                            Err(io::Error::other("owned validation stop"))
                        } else {
                            Ok(())
                        }
                    },
                );
                assert_eq!(applied.is_ok(), phase != "partial");
                assert!(std::env::var_os("PATH") == before_path);
                if ["decision", "backup", "journal"].contains(&phase) {
                    assert!(
                        reg.test_finish(&fixture.path(), |at, index| {
                            if at == phase && index == 0 {
                                Err(io::Error::other("owned cleanup stop"))
                            } else {
                                Ok(())
                            }
                        })
                        .is_err()
                    );
                }
                drop(reg);
                let recover = || {
                    crate::core_install::recover(
                        &fixture.settings.codex_home,
                        &fixture.settings.user_home,
                        &fixture.settings.dependency_user_home,
                    )
                };
                if phase == "foreign" {
                    let (foreign, _) = ProcessPathSnapshot::read()
                        .unwrap()
                        .prepend(&fixture.root.join("foreign-path"))
                        .unwrap();
                    foreign.publish().unwrap();
                    let foreign_path = std::env::var_os("PATH");
                    let journal = fs::read(state.join(JOURNAL)).unwrap();
                    let image = recovery_image(&fixture.root);
                    assert!(
                        crate::core_install::preview_recovery(
                            &fixture.settings.codex_home,
                            &fixture.settings.user_home,
                            &fixture.settings.dependency_user_home,
                        )
                        .is_err()
                    );
                    assert_eq!(image, recovery_image(&fixture.root));
                    assert!(recover().is_err());
                    assert!(std::env::var_os("PATH") == foreign_path);
                    assert_eq!(fs::read(state.join(JOURNAL)).unwrap(), journal);
                    assert_eq!(
                        fs::read(fixture.path()).unwrap(),
                        metadata.snapshot().contents()
                    );
                    foreign.rollback().unwrap();
                }
                assert_recovery_preview(
                    &fixture,
                    if phase == "partial" {
                        "rollback"
                    } else {
                        "finish"
                    },
                );
                let report = recover().unwrap();
                assert_eq!(report.committed, phase != "partial");
                if phase == "partial" {
                    assert!(std::env::var_os("PATH") == installed_path);
                    fixture.assert_objects(&metadata);
                    install.rollback().unwrap();
                } else {
                    assert!(!fixture.path().exists());
                }
                assert!(std::env::var_os("PATH") == before_path);
                assert!(!state.join(JOURNAL).exists());
                assert!(!state.join(crate::registration::COMMIT).exists());
                assert_eq!(
                    fs::read(fixture.settings.source_root.join("skill-one/keep")).unwrap(),
                    b"source skill stays"
                );
                assert!(
                    fixture
                        .settings
                        .codex_home
                        .join("harness.config.toml")
                        .is_symlink()
                );
            }
        });
    }

    #[test]
    fn killed_process_path_disconnection_preserves_recovering_process_environment() {
        killed_disconnection_cases(PathScope::Process);
    }

    fn killed_disconnection_cases(scope: PathScope) {
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        for phase in ["partial", "completed", "decision", "backup", "journal"] {
            let parent_path = std::env::var_os("PATH");
            let mut fixture = Fixture::new();
            fixture.settings.path_scope = scope;
            fixture.settings.path_added = scope == PathScope::Process;
            fixture.publish_fresh();
            let metadata = fixture.read();
            fs::write(
                fixture.root.join("disconnect-input.json"),
                serde_json::to_vec(&fixture.settings).unwrap(),
            )
            .unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "installation_metadata::tests::disconnection_process_fixture",
                    "--nocapture",
                ])
                .env("HARNESS_DISCONNECTION_ROOT", &fixture.root)
                .env("HARNESS_DISCONNECTION_PHASE", phase)
                .env("PATH", "C:\\owned-disconnection-child\\bin")
                .stdin(Stdio::null())
                .stdout(fs::File::create(fixture.root.join("child.stdout")).unwrap())
                .stderr(fs::File::create(fixture.root.join("child.stderr")).unwrap())
                .spawn()
                .unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while !fixture.root.join("paused").exists() && Instant::now() < until {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let reached =
                fs::read(fixture.root.join("paused")).ok().as_deref() == Some(phase.as_bytes());
            let _ = child.kill();
            let exit = child.wait().unwrap();
            assert!(
                reached,
                "boundary {phase} not observed: {}",
                fixture.root.display()
            );
            assert!(!exit.success());
            assert_ne!(exit.code(), Some(79));
            assert_recovery_preview(
                &fixture,
                if phase == "partial" {
                    "rollback"
                } else {
                    "finish"
                },
            );
            let report = crate::core_install::recover(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home,
            )
            .unwrap();
            assert!(std::env::var_os("PATH") == parent_path);
            assert_eq!(report.committed, phase != "partial");
            if phase == "partial" {
                fixture.assert_objects(&metadata);
            } else {
                assert!(!fixture.path().exists());
            }
            assert!(
                fixture
                    .settings
                    .codex_home
                    .join("harness.config.toml")
                    .is_symlink()
            );
            assert_eq!(
                fs::read(fixture.settings.source_root.join("skill-one/keep")).unwrap(),
                b"source skill stays"
            );
        }
    }

    #[test]
    fn core_recovery_checks_both_owners_before_undo_or_committed_cleanup() {
        for phase in ["partial", "completed", "committed", "journal-retired"] {
            let fixture = Fixture::new();
            let state = fixture
                .settings
                .codex_home
                .join("harness/native-registration");
            let reg = Registration::open(&state).unwrap();
            if phase == "partial" {
                assert!(
                    reg.test_apply_metadata(
                        &fixture.links,
                        MetadataDestination::absent(&fixture.path()).unwrap(),
                        |view| encode(&fixture.settings, &fixture.links, view, Previous::Fresh),
                        || Err(io::Error::other("owned activation failure"))
                    )
                    .is_err()
                );
            } else {
                reg.apply_with_metadata(
                    &fixture.links,
                    &[],
                    &[],
                    &[],
                    MetadataDestination::absent(&fixture.path()).unwrap(),
                    |view| encode(&fixture.settings, &fixture.links, view, Previous::Fresh),
                )
                .unwrap();
                if phase != "completed" {
                    let boundary = if phase == "committed" {
                        "decision"
                    } else {
                        "journal"
                    };
                    assert!(
                        reg.test_finish(&fixture.path(), |at, _| {
                            if at == boundary {
                                Err(io::Error::other("owned cleanup interruption"))
                            } else {
                                Ok(())
                            }
                        })
                        .is_err()
                    );
                }
            }
            drop(reg);
            let metadata_before = fs::read(fixture.path()).unwrap();
            let journal_before = fs::read(state.join(JOURNAL)).ok();
            let commit_before = fs::read(state.join(crate::registration::COMMIT)).ok();
            assert_recovery_preview(
                &fixture,
                if phase == "partial" {
                    "rollback"
                } else {
                    "finish"
                },
            );
            for (user, dependency) in [
                (
                    fixture.root.join("wrong-user"),
                    fixture.settings.dependency_user_home.clone(),
                ),
                (
                    fixture.settings.user_home.clone(),
                    fixture.root.join("wrong-dependency"),
                ),
            ] {
                let image = recovery_image(&fixture.root);
                assert!(
                    crate::core_install::preview_recovery(
                        &fixture.settings.codex_home,
                        &user,
                        &dependency
                    )
                    .is_err()
                );
                assert_eq!(image, recovery_image(&fixture.root));
                assert!(
                    crate::core_install::recover(&fixture.settings.codex_home, &user, &dependency)
                        .is_err(),
                    "{phase}"
                );
                assert_eq!(fs::read(fixture.path()).unwrap(), metadata_before);
                assert_eq!(fs::read(state.join(JOURNAL)).ok(), journal_before);
                assert_eq!(
                    fs::read(state.join(crate::registration::COMMIT)).ok(),
                    commit_before
                );
            }
            let result = crate::core_install::recover(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home,
            )
            .unwrap();
            assert_eq!(result.committed, phase != "partial");
            assert_eq!(fixture.path().exists(), phase != "partial");
            assert!(
                fixture
                    .settings
                    .codex_home
                    .join("harness.config.toml")
                    .is_symlink()
            );
            assert!(!state.join(JOURNAL).exists());
            assert!(!state.join(crate::registration::COMMIT).exists());
        }
    }

    #[test]
    fn recovery_preview_preserves_absence_unowned_state_and_busy_locks() {
        let fixture = Fixture::new();
        assert_recovery_preview(&fixture, "none");
        let missing = fixture.root.join("absent-codex");
        let missing_user = fixture.root.join("absent-user");
        let image = recovery_image(&fixture.root);
        assert_eq!(
            crate::core_install::preview_recovery(&missing, &missing_user, &missing_user)
                .unwrap()
                .action,
            "none"
        );
        assert_eq!(image, recovery_image(&fixture.root));
        let state = fixture
            .settings
            .codex_home
            .join("harness/native-registration");
        fs::create_dir_all(&state).unwrap();
        let image = recovery_image(&fixture.root);
        assert!(
            crate::core_install::preview_recovery(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home
            )
            .is_err()
        );
        assert_eq!(image, recovery_image(&fixture.root));
        let reg = Registration::open(&state).unwrap();
        assert!(
            crate::core_install::preview_recovery(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home
            )
            .is_err()
        );
        drop(reg);
        assert_recovery_preview(&fixture, "none");
        let locks = crate::installation_lock::InstallationLocks::acquire(
            &fixture.settings.user_home,
            &fixture.settings.dependency_user_home,
        )
        .unwrap();
        let image = recovery_image(&fixture.root);
        assert!(
            crate::core_install::preview_recovery(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home
            )
            .is_err()
        );
        assert_eq!(image, recovery_image(&fixture.root));
        drop(locks);
        fs::remove_file(state.join("registration.lock")).unwrap();
        let image = recovery_image(&fixture.root);
        assert!(
            crate::core_install::preview_recovery(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home
            )
            .is_err()
        );
        assert_eq!(image, recovery_image(&fixture.root));
    }

    #[test]
    fn recovery_preview_user_path_observes_applied_undone_and_foreign_values_without_writes() {
        use crate::{
            environment_path::{UserPathSnapshot, receipt::PathReceipts},
            installation_path::PathChange,
        };
        crate::environment_path::with_test_registry(|| {
            for phase in [
                "partial",
                "undone",
                "completed",
                "committed",
                "foreign",
                "foreign-receipt",
            ] {
                let mut fixture = Fixture::new();
                fixture.settings.path_added = true;
                let state = fixture
                    .settings
                    .codex_home
                    .join("harness/native-registration");
                let reg = Registration::open(&state).unwrap();
                let (change, added) = UserPathSnapshot::for_registration()
                    .unwrap()
                    .prepend(&fixture.settings.codex_home.join("harness/bin"))
                    .unwrap();
                assert!(added);
                let completed = ["completed", "committed", "foreign-receipt"].contains(&phase);
                let result = reg.apply_installation(
                    &fixture.links,
                    &[],
                    &[],
                    &[],
                    MetadataDestination::absent(&fixture.path()).unwrap(),
                    |view| encode(&fixture.settings, &fixture.links, view, Previous::Fresh),
                    Some(PathChange::User(change.clone())),
                    || {
                        if completed {
                            Ok(())
                        } else {
                            Err(io::Error::other("owned preview interruption"))
                        }
                    },
                );
                assert_eq!(result.is_ok(), completed);
                if phase == "undone" {
                    let (guard, bytes) = FileGuard::read_regular(&reg.journal_path()).unwrap();
                    PathReceipts::held_registration(&reg.journal_path(), &bytes, &guard, &change)
                        .undo_registration()
                        .unwrap();
                }
                if ["committed", "foreign-receipt"].contains(&phase) {
                    assert!(
                        reg.test_finish(&fixture.path(), |at, _| if at == "decision" {
                            Err(io::Error::other("owned commitment stop"))
                        } else {
                            Ok(())
                        })
                        .is_err()
                    );
                }
                drop(reg);
                if phase == "foreign-receipt" {
                    let receipt = fs::read_dir(&state)
                        .unwrap()
                        .map(|entry| entry.unwrap().path())
                        .find(|path| {
                            path.file_name()
                                .unwrap()
                                .to_string_lossy()
                                .ends_with("-applied.json")
                        })
                        .unwrap();
                    let saved = fixture.root.join("saved-receipt");
                    let bytes = fs::read(&receipt).unwrap();
                    fs::rename(&receipt, &saved).unwrap();
                    fs::write(&receipt, &bytes).unwrap();
                    let image = recovery_image(&fixture.root);
                    assert!(
                        crate::core_install::preview_recovery(
                            &fixture.settings.codex_home,
                            &fixture.settings.user_home,
                            &fixture.settings.dependency_user_home
                        )
                        .is_err()
                    );
                    assert!(
                        crate::core_install::recover(
                            &fixture.settings.codex_home,
                            &fixture.settings.user_home,
                            &fixture.settings.dependency_user_home
                        )
                        .is_err()
                    );
                    assert_eq!(image, recovery_image(&fixture.root));
                    fs::remove_file(&receipt).unwrap();
                    fs::rename(&saved, &receipt).unwrap();
                }
                if phase == "foreign" {
                    let original = UserPathSnapshot::for_registration()
                        .unwrap()
                        .text()
                        .unwrap();
                    let foreign = Some("C:\\owned-foreign-preview".to_string());
                    UserPathSnapshot::for_registration()
                        .unwrap()
                        .legacy_restore(&foreign, &original)
                        .unwrap()
                        .publish_legacy()
                        .unwrap();
                    let image = recovery_image(&fixture.root);
                    assert!(
                        crate::core_install::preview_recovery(
                            &fixture.settings.codex_home,
                            &fixture.settings.user_home,
                            &fixture.settings.dependency_user_home
                        )
                        .is_err()
                    );
                    assert_eq!(image, recovery_image(&fixture.root));
                    assert!(
                        UserPathSnapshot::for_registration()
                            .unwrap()
                            .text()
                            .unwrap()
                            == foreign
                    );
                    UserPathSnapshot::for_registration()
                        .unwrap()
                        .legacy_restore(&original, &foreign)
                        .unwrap()
                        .publish_legacy()
                        .unwrap();
                }
                assert_recovery_preview(&fixture, if completed { "finish" } else { "rollback" });
                let result = crate::core_install::recover(
                    &fixture.settings.codex_home,
                    &fixture.settings.user_home,
                    &fixture.settings.dependency_user_home,
                )
                .unwrap();
                assert_eq!(result.committed, completed);
                assert_recovery_preview(&fixture, "none");
                assert!(!fs::read_dir(&state).unwrap().any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .contains("path-")
                }));
            }
        });
    }

    #[test]
    #[ignore = "explicit current native manager; only owned file intent with no registry change"]
    fn native_manager_recovers_owned_core_intent_and_preserves_a_wrong_owner() {
        use crate::process::{CommandSpec, StopReason};
        let manager = PathBuf::from(
            std::env::var_os("HARNESS_CORE_REAL_MANAGER")
                .expect("explicit native manager required"),
        );
        for completed in [false, true] {
            let fixture = Fixture::new();
            let state = fixture
                .settings
                .codex_home
                .join("harness/native-registration");
            let reg = Registration::open(&state).unwrap();
            if completed {
                reg.apply_with_metadata(
                    &fixture.links,
                    &[],
                    &[],
                    &[],
                    MetadataDestination::absent(&fixture.path()).unwrap(),
                    |view| encode(&fixture.settings, &fixture.links, view, Previous::Fresh),
                )
                .unwrap();
            } else {
                assert!(
                    reg.test_apply_metadata(
                        &fixture.links,
                        MetadataDestination::absent(&fixture.path()).unwrap(),
                        |view| encode(&fixture.settings, &fixture.links, view, Previous::Fresh),
                        || Err(io::Error::other("owned interruption"))
                    )
                    .is_err()
                );
            }
            drop(reg);
            let before = fs::read(fixture.path()).unwrap();
            for (valid_owner, preview) in
                [(false, true), (false, false), (true, true), (true, false)]
            {
                let user = if valid_owner {
                    fixture.settings.user_home.clone()
                } else {
                    fixture.root.join("wrong-owner")
                };
                let mut command = CommandSpec::new(&manager);
                command.args = vec![
                    "recover".into(),
                    "--core-only".into(),
                    "--codex-home".into(),
                    fixture.settings.codex_home.as_os_str().into(),
                    "--user-home".into(),
                    user.into_os_string(),
                    "--dependency-user-home".into(),
                    fixture.settings.dependency_user_home.as_os_str().into(),
                ];
                if preview {
                    command.args.push("--preview".into());
                }
                command.current_dir = Some(fixture.root.clone());
                let prefix = if valid_owner {
                    if preview {
                        "preview-correct"
                    } else {
                        "recover-correct"
                    }
                } else {
                    if preview {
                        "preview-wrong"
                    } else {
                        "recover-wrong"
                    }
                };
                let stdout = fixture.root.join(format!("{prefix}.stdout"));
                let outcome = crate::native_build::invoke_management(
                    command,
                    &fixture.root.join(format!("{prefix}.stderr")),
                    Some(&stdout),
                    std::time::Duration::from_secs(30),
                )
                .unwrap();
                assert_eq!(outcome.reason, StopReason::Exited);
                if valid_owner {
                    assert_eq!(
                        outcome.exit_code,
                        0,
                        "CLI evidence: {}",
                        fixture.root.display()
                    );
                    let report: serde_json::Value =
                        serde_json::from_slice(&fs::read(stdout).unwrap()).unwrap();
                    if preview {
                        assert_eq!(report["status"], "preview");
                        assert_eq!(
                            report["action"],
                            if completed { "finish" } else { "rollback" }
                        );
                        assert_eq!(report["journal"], "native");
                        assert!(report.get("committed").is_none());
                        assert_eq!(fs::read(fixture.path()).unwrap(), before);
                        assert!(state.join(JOURNAL).exists());
                    } else {
                        assert_eq!(report["committed"], completed);
                    }
                    assert_eq!(report["model_calls"], 0);
                } else {
                    assert_ne!(outcome.exit_code, 0);
                    assert_eq!(fs::read(fixture.path()).unwrap(), before);
                    assert!(state.join(JOURNAL).exists());
                }
            }
            assert_eq!(fixture.path().exists(), completed);
            assert!(!state.join(JOURNAL).exists());
            assert!(
                fixture
                    .settings
                    .codex_home
                    .join("harness.config.toml")
                    .is_symlink()
            );
        }
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("harness-installation-metadata-")
                .tempdir()
                .unwrap()
                .keep();
            let source = root.join("source");
            let home = root.join("codex");
            let user = root.join("user");
            fs::create_dir_all(source.join("skill-one")).unwrap();
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&user).unwrap();
            fs::write(source.join("instructions"), b"instructions").unwrap();
            fs::write(source.join("next-instructions"), b"updated instructions").unwrap();
            fs::write(source.join("profile"), b"profile").unwrap();
            fs::write(source.join("next-profile"), b"next profile").unwrap();
            fs::write(source.join("skill-one/keep"), b"source skill stays").unwrap();
            symlink_file(source.join("profile"), home.join("harness.config.toml")).unwrap();
            let settings = Settings {
                source_root: source.clone(),
                codex_home: home.clone(),
                user_home: user.clone(),
                dependency_user_home: user.clone(),
                codex_command: root.join("upstream.exe"),
                path_scope: PathScope::User,
                path_added: false,
                versions: BTreeMap::new(),
            };
            let links = vec![
                Link {
                    kind: "instructions".into(),
                    name: "global-instructions".into(),
                    source: source.join("instructions"),
                    destination: home.join("AGENTS.md"),
                    connection: Connection::Missing,
                },
                Link {
                    kind: "profile".into(),
                    name: "harness".into(),
                    source: source.join("profile"),
                    destination: home.join("harness.config.toml"),
                    connection: Connection::Linked,
                },
                Link {
                    kind: "skill".into(),
                    name: "skill-one".into(),
                    source: source.join("skill-one"),
                    destination: user.join(".agents/skills/skill-one"),
                    connection: Connection::Missing,
                },
            ];
            println!("stable metadata evidence: {}", root.display());
            Self {
                root,
                settings,
                links,
            }
        }

        fn path(&self) -> PathBuf {
            self.settings.codex_home.join("harness/installation.json")
        }

        fn registration(&self) -> Registration {
            Registration::open(&self.root.join("journal-state")).unwrap()
        }

        fn read(&self) -> InstallationMetadata {
            InstallationMetadata::read(
                &self.settings.codex_home,
                &self.settings.user_home,
                &self.settings.dependency_user_home,
            )
            .unwrap()
            .unwrap()
        }

        fn publish_fresh(&self) {
            let reg = self.registration();
            reg.apply_with_metadata(
                &self.links,
                &[],
                &[],
                &[],
                MetadataDestination::absent(&self.path()).unwrap(),
                |view| encode(&self.settings, &self.links, view, Previous::Fresh),
            )
            .unwrap();
            assert!(reg.finish(&self.path()).unwrap().committed);
            assert!(!self.root.join("journal-state").join(JOURNAL).exists());
        }

        fn assert_objects(&self, metadata: &InstallationMetadata) {
            for link in &metadata.document.links {
                let actual = FileGuard::capture_link(&link.object.path).unwrap();
                assert_eq!(actual.object_identity().unwrap(), link.object.identity);
                assert_eq!(
                    fs::read_link(&link.object.path).unwrap(),
                    link.object.target
                );
            }
        }
    }

    #[test]
    fn fresh_update_remove_owned_and_preserve_adopted_with_real_ids() {
        let fixture = Fixture::new();
        assert!(
            InstallationMetadata::read(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home
            )
            .unwrap()
            .is_none()
        );
        assert!(!fixture.settings.codex_home.join("harness").exists());
        fixture.publish_fresh();
        let original = fixture.read();
        fixture.assert_objects(&original);
        assert_eq!(
            original.document.links.iter().filter(|x| x.owned).count(),
            2
        );
        assert_eq!(
            original.document.links.iter().filter(|x| !x.owned).count(),
            1
        );
        let identity = original.document.metadata_identity.clone();

        let reg = fixture.registration();
        let kept = &fixture.links[1..];
        let replacement = LinkChange::replace(
            &fixture.links[0].destination,
            &fixture.links[0].source,
            &fixture.settings.source_root.join("next-instructions"),
        )
        .unwrap();
        let mut desired: Vec<Link> = fixture
            .links
            .iter()
            .map(|x| Link {
                kind: x.kind.clone(),
                name: x.name.clone(),
                source: x.source.clone(),
                destination: x.destination.clone(),
                connection: Connection::Linked,
            })
            .collect();
        desired[0].source = fixture.settings.source_root.join("next-instructions");
        reg.apply_with_metadata(
            kept,
            &[],
            &[],
            &[replacement],
            original.destination().unwrap(),
            |view| {
                encode(
                    &fixture.settings,
                    &desired,
                    view,
                    Previous::Native(&original),
                )
            },
        )
        .unwrap();
        reg.finish(&fixture.path()).unwrap();
        let updated = fixture.read();
        fixture.assert_objects(&updated);
        assert_eq!(updated.document.metadata_identity, identity);
        assert_eq!(updated.document.links.iter().filter(|x| x.owned).count(), 2);
        assert_eq!(
            fs::read(&fixture.links[0].destination).unwrap(),
            b"updated instructions"
        );

        let removal =
            LinkChange::remove(&fixture.links[2].destination, &fixture.links[2].source).unwrap();
        reg.apply_with_metadata(
            &desired[..2],
            &[],
            &[],
            &[removal],
            updated.destination().unwrap(),
            |view| {
                encode(
                    &fixture.settings,
                    &desired[..2],
                    view,
                    Previous::Native(&updated),
                )
            },
        )
        .unwrap();
        reg.finish(&fixture.path()).unwrap();
        let last = fixture.read();
        fixture.assert_objects(&last);
        assert_eq!(last.document.links.len(), 2);
        assert_eq!(last.document.metadata_identity, identity);
        assert!(fs::symlink_metadata(&fixture.links[2].destination).is_err());
        assert_eq!(
            fs::read(fixture.links[2].source.join("keep")).unwrap(),
            b"source skill stays"
        );
        assert_eq!(
            fs::read_link(&fixture.links[1].destination).unwrap(),
            fixture.links[1].source
        );
    }

    #[test]
    fn fresh_foreign_and_recorded_adopted_links_cannot_be_replaced_or_removed() {
        for native in [false, true] {
            for replace in [false, true] {
                let fixture = Fixture::new();
                if native {
                    fixture.publish_fresh();
                }
                let previous = native.then(|| fixture.read());
                let original = fs::read(fixture.path()).ok();
                let descriptor = &fixture.links[1];
                let next_source = fixture.settings.source_root.join("next-profile");
                let change = if replace {
                    LinkChange::replace(&descriptor.destination, &descriptor.source, &next_source)
                        .unwrap()
                } else {
                    LinkChange::remove(&descriptor.destination, &descriptor.source).unwrap()
                };
                let kept: Vec<Link> = if native {
                    fixture
                        .links
                        .iter()
                        .filter(|x| x.kind != "profile")
                        .map(|x| Link {
                            kind: x.kind.clone(),
                            name: x.name.clone(),
                            source: x.source.clone(),
                            destination: x.destination.clone(),
                            connection: Connection::Linked,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let mut desired: Vec<Link> = kept
                    .iter()
                    .map(|x| Link {
                        kind: x.kind.clone(),
                        name: x.name.clone(),
                        source: x.source.clone(),
                        destination: x.destination.clone(),
                        connection: Connection::Linked,
                    })
                    .collect();
                if replace {
                    desired.push(Link {
                        kind: "profile".into(),
                        name: "harness".into(),
                        source: next_source,
                        destination: descriptor.destination.clone(),
                        connection: Connection::Linked,
                    });
                }
                let destination = match &previous {
                    Some(x) => x.destination().unwrap(),
                    None => MetadataDestination::absent(&fixture.path()).unwrap(),
                };
                let reg = fixture.registration();
                assert!(
                    reg.apply_with_metadata(
                        &kept,
                        &[],
                        &[],
                        &[change],
                        destination,
                        |view| encode(
                            &fixture.settings,
                            &desired,
                            view,
                            previous.as_ref().map_or(Previous::Fresh, Previous::Native)
                        )
                    )
                    .is_err()
                );
                assert_eq!(
                    fs::read_link(&descriptor.destination).unwrap(),
                    descriptor.source
                );
                assert_eq!(fs::read(fixture.path()).ok(), original);
                assert!(!fixture.root.join("journal-state").join(JOURNAL).exists());
            }
        }
    }

    #[test]
    fn checksum_host_schema_destination_and_metadata_identity_fail_closed() {
        let fixture = Fixture::new();
        fixture.publish_fresh();
        let accepted = fixture.read();
        let original = fs::read(fixture.path()).unwrap();
        for case in 0..6 {
            let mut document: Document = serde_json::from_slice(&original).unwrap();
            match case {
                0 => document.schema_version = 999,
                1 => document.settings.dependency_user_home = fixture.root.join("other-user"),
                2 => {
                    document.links[0].object.path =
                        fixture.settings.source_root.join("instructions")
                }
                _ => {}
            }
            document.checksum = document.checksum().unwrap();
            let bytes = match case {
                3 => {
                    document.checksum = "foreign-checksum".into();
                    serde_json::to_vec(&document).unwrap()
                }
                4 => b"{malformed-private-canary".to_vec(),
                5 => vec![b'x'; MAX_BYTES + 1],
                _ => serde_json::to_vec(&document).unwrap(),
            };
            fs::write(fixture.path(), &bytes).unwrap();
            let error = match InstallationMetadata::read(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home,
            ) {
                Ok(_) => panic!("invalid state accepted"),
                Err(error) => error,
            };
            assert!(!error.to_string().contains("private-canary"));
            assert_eq!(fs::read(fixture.path()).unwrap(), bytes);
            assert!(accepted.verify_unchanged().is_err());
            fs::write(fixture.path(), &original).unwrap();
        }
        accepted.verify_unchanged().unwrap();
        let saved = fixture.root.join("original-metadata");
        fs::rename(fixture.path(), &saved).unwrap();
        fs::write(fixture.path(), &original).unwrap();
        assert!(
            InstallationMetadata::read(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home
            )
            .is_err()
        );
        assert!(accepted.verify_unchanged().is_err());
        assert_eq!(fs::read(fixture.path()).unwrap(), original);
        assert_eq!(fs::read(saved).unwrap(), original);
        assert_eq!(
            fs::read(fixture.settings.source_root.join("instructions")).unwrap(),
            b"instructions"
        );
    }

    #[test]
    fn legacy_import_captures_ids_once_and_preserves_owned_adopted_claims() {
        let fixture = Fixture::new();
        symlink_file(&fixture.links[0].source, &fixture.links[0].destination).unwrap();
        fs::create_dir_all(fixture.links[2].destination.parent().unwrap()).unwrap();
        symlink_dir(&fixture.links[2].source, &fixture.links[2].destination).unwrap();
        fs::create_dir_all(fixture.path().parent().unwrap()).unwrap();
        let legacy = serde_json::json!({
            "schemaVersion":1, "sourceRoot":fixture.settings.source_root, "codexHome":fixture.settings.codex_home,
            "userHome":fixture.settings.user_home, "dependencyUserHome":fixture.settings.dependency_user_home,
            "codexCommand":fixture.settings.codex_command, "profileName":"harness", "pathScope":"User", "pathAdded":false, "versions":{},
            "links":fixture.links.iter().map(|x| serde_json::json!({"kind":x.kind,"name":x.name,"source":x.source,"destination":x.destination,"owned":x.kind!="profile"})).collect::<Vec<_>>()
        });
        fs::write(fixture.path(), serde_json::to_vec(&legacy).unwrap()).unwrap();
        let old = LegacyInstallation::read(
            &fixture.settings.codex_home,
            &fixture.settings.user_home,
            &fixture.settings.dependency_user_home,
        )
        .unwrap()
        .unwrap();
        let old_identity = FileGuard::read_regular(&fixture.path())
            .unwrap()
            .0
            .object_identity()
            .unwrap();
        let snapshot = ConfigSnapshot::read(&fixture.path()).unwrap();
        let reg = fixture.registration();
        reg.apply_with_metadata(
            &fixture.links,
            &[],
            &[],
            &[],
            MetadataDestination::existing(&snapshot).unwrap(),
            |view| {
                encode(
                    &fixture.settings,
                    &fixture.links,
                    view,
                    Previous::Legacy(&old),
                )
            },
        )
        .unwrap();
        reg.finish(&fixture.path()).unwrap();
        let imported = fixture.read();
        fixture.assert_objects(&imported);
        assert_eq!(imported.document.metadata_identity, old_identity);
        assert_eq!(
            imported.document.links.iter().filter(|x| x.owned).count(),
            2
        );
        assert_eq!(
            imported.document.links.iter().filter(|x| !x.owned).count(),
            1
        );
        let native_bytes = fs::read(fixture.path()).unwrap();
        assert!(
            LegacyInstallation::read(
                &fixture.settings.codex_home,
                &fixture.settings.user_home,
                &fixture.settings.dependency_user_home
            )
            .is_err()
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), native_bytes);
    }

    #[test]
    fn previous_native_ownership_cannot_bless_same_target_foreign_replacement() {
        let fixture = Fixture::new();
        fixture.publish_fresh();
        let prior = fixture.read();
        let metadata = fs::read(fixture.path()).unwrap();
        let path = &fixture.links[0].destination;
        let old_id = FileGuard::capture_link(path)
            .unwrap()
            .object_identity()
            .unwrap();
        fs::remove_file(path).unwrap();
        symlink_file(&fixture.links[0].source, path).unwrap();
        let foreign_id = FileGuard::capture_link(path)
            .unwrap()
            .object_identity()
            .unwrap();
        assert_ne!(foreign_id, old_id);
        let reg = fixture.registration();
        assert!(
            reg.apply_with_metadata(
                &fixture.links,
                &[],
                &[],
                &[],
                prior.destination().unwrap(),
                |view| encode(
                    &fixture.settings,
                    &fixture.links,
                    view,
                    Previous::Native(&prior)
                )
            )
            .is_err()
        );
        assert_eq!(
            FileGuard::capture_link(path)
                .unwrap()
                .object_identity()
                .unwrap(),
            foreign_id
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), metadata);
        assert!(!fixture.root.join("journal-state").join(JOURNAL).exists());
        // A component-only caller must carry untouched owned entries too; it
        // cannot silently discard their ownership by omitting them from a plan.
        assert!(
            reg.apply_with_metadata(
                &fixture.links[1..],
                &[],
                &[],
                &[],
                prior.destination().unwrap(),
                |view| encode(
                    &fixture.settings,
                    &fixture.links[1..],
                    view,
                    Previous::Native(&prior)
                )
            )
            .is_err()
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), metadata);
        assert_eq!(fs::read_link(path).unwrap(), fixture.links[0].source);
        let replace =
            LinkChange::replace(path, &fixture.links[0].source, &fixture.links[0].source).unwrap();
        assert!(
            reg.apply_with_metadata(
                &fixture.links[1..],
                &[],
                &[],
                &[replace],
                prior.destination().unwrap(),
                |view| encode(
                    &fixture.settings,
                    &fixture.links,
                    view,
                    Previous::Native(&prior)
                )
            )
            .is_err()
        );
        assert_eq!(
            FileGuard::capture_link(path)
                .unwrap()
                .object_identity()
                .unwrap(),
            foreign_id
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), metadata);
        assert!(!fixture.root.join("journal-state").join(JOURNAL).exists());
    }

    #[test]
    fn missing_owned_native_link_is_repaired_with_a_new_identity_and_retained_peers() {
        for (index, kind) in [(0, "instructions"), (2, "skill")] {
            let fixture = Fixture::new();
            fixture.publish_fresh();
            let prior = fixture.read();
            let old = prior
                .document
                .links
                .iter()
                .find(|link| link.kind == kind)
                .unwrap()
                .object
                .identity
                .clone();
            if index == 2 {
                fs::remove_dir(&fixture.links[index].destination).unwrap();
            } else {
                fs::remove_file(&fixture.links[index].destination).unwrap();
            }
            let reg = fixture.registration();
            reg.apply_with_metadata(
                &fixture.links,
                &[],
                &[],
                &[],
                prior.destination().unwrap(),
                |view| {
                    encode(
                        &fixture.settings,
                        &fixture.links,
                        view,
                        Previous::Native(&prior),
                    )
                },
            )
            .unwrap();
            assert!(reg.finish(&fixture.path()).unwrap().committed);
            let repaired = fixture.read();
            fixture.assert_objects(&repaired);
            let new = repaired
                .document
                .links
                .iter()
                .find(|link| link.kind == kind)
                .unwrap();
            assert!(new.owned);
            assert_ne!(new.object.identity, old);
            for previous in prior.document.links.iter().filter(|link| link.kind != kind) {
                let current = repaired
                    .document
                    .links
                    .iter()
                    .find(|link| link.kind == previous.kind)
                    .unwrap();
                assert_eq!(current.object, previous.object);
                assert_eq!(current.owned, previous.owned);
            }
            assert_eq!(
                fs::read(fixture.settings.source_root.join("instructions")).unwrap(),
                b"instructions"
            );
            assert_eq!(
                fs::read(fixture.settings.source_root.join("skill-one/keep")).unwrap(),
                b"source skill stays"
            );
        }
    }

    #[test]
    fn missing_owned_legacy_link_is_repaired_without_inventing_historical_identity() {
        let fixture = Fixture::new();
        fixture.publish_fresh();
        fs::remove_file(&fixture.links[0].destination).unwrap();
        let legacy = serde_json::json!({
            "schemaVersion":1,"sourceRoot":fixture.settings.source_root,"codexHome":fixture.settings.codex_home,
            "userHome":fixture.settings.user_home,"dependencyUserHome":fixture.settings.dependency_user_home,
            "codexCommand":fixture.settings.codex_command,"profileName":"harness","pathScope":"User","pathAdded":false,"versions":{},
            "links":fixture.links.iter().map(|link|serde_json::json!({"kind":link.kind,"name":link.name,"source":link.source,"destination":link.destination,"owned":link.kind!="profile"})).collect::<Vec<_>>()
        });
        fs::write(fixture.path(), serde_json::to_vec(&legacy).unwrap()).unwrap();
        let prior = LegacyInstallation::read(
            &fixture.settings.codex_home,
            &fixture.settings.user_home,
            &fixture.settings.dependency_user_home,
        )
        .unwrap()
        .unwrap();
        let snapshot = ConfigSnapshot::read(&fixture.path()).unwrap();
        let reg = fixture.registration();
        reg.apply_with_metadata(
            &fixture.links,
            &[],
            &[],
            &[],
            MetadataDestination::existing(&snapshot).unwrap(),
            |view| {
                encode(
                    &fixture.settings,
                    &fixture.links,
                    view,
                    Previous::Legacy(&prior),
                )
            },
        )
        .unwrap();
        reg.finish(&fixture.path()).unwrap();
        let repaired = fixture.read();
        fixture.assert_objects(&repaired);
        assert_eq!(
            repaired
                .document
                .links
                .iter()
                .filter(|link| link.owned)
                .count(),
            2
        );
        assert!(
            !repaired
                .document
                .links
                .iter()
                .find(|link| link.kind == "profile")
                .unwrap()
                .owned
        );
    }

    #[test]
    fn missing_owned_link_cannot_be_silently_omitted_from_a_component_plan() {
        let fixture = Fixture::new();
        fixture.publish_fresh();
        let prior = fixture.read();
        fs::remove_file(&fixture.links[0].destination).unwrap();
        let before = fs::read(fixture.path()).unwrap();
        let reg = fixture.registration();
        assert!(
            reg.apply_with_metadata(
                &fixture.links[1..],
                &[],
                &[],
                &[],
                prior.destination().unwrap(),
                |view| encode(
                    &fixture.settings,
                    &fixture.links[1..],
                    view,
                    Previous::Native(&prior)
                )
            )
            .is_err()
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), before);
        assert!(!fixture.links[0].destination.exists());
        assert!(!fixture.root.join("journal-state").join(JOURNAL).exists());
    }

    #[test]
    fn explicit_absent_retirement_updates_native_and_legacy_metadata_or_restores_history() {
        for legacy in [false, true] {
            for completed in [false, true] {
                let fixture = Fixture::new();
                let old_legacy = legacy.then(|| legacy_fixture(&fixture));
                let old_native = (!legacy).then(|| {
                    fixture.publish_fresh();
                    fixture.read()
                });
                let prior = || match (&old_legacy, &old_native) {
                    (Some(old), None) => Previous::Legacy(old),
                    (None, Some(old)) => Previous::Native(old),
                    _ => unreachable!(),
                };
                let desired = if let Some(old) = &old_legacy {
                    old.links()
                        .iter()
                        .filter(|link| link.kind != "instructions")
                        .map(|link| Link {
                            kind: link.kind.clone(),
                            name: link.name.clone(),
                            source: link.source.clone(),
                            destination: link.destination.clone(),
                            connection: Connection::Linked,
                        })
                        .collect::<Vec<_>>()
                } else {
                    fixture.links[1..].to_vec()
                };
                let retired = fixture.links[0].destination.clone();
                fs::remove_file(&retired).unwrap();
                let before = fs::read(fixture.path()).unwrap();
                let reg = fixture.registration();
                let destination = match (&old_legacy, &old_native) {
                    (Some(old), None) => old.metadata_destination().unwrap(),
                    (None, Some(old)) => old.destination().unwrap(),
                    _ => unreachable!(),
                };
                let result = reg.apply_installation(
                    &desired,
                    &[],
                    &[],
                    &[],
                    destination,
                    |view| {
                        encode_retiring_absent(
                            &fixture.settings,
                            &desired,
                            view,
                            prior(),
                            std::slice::from_ref(&retired),
                        )
                    },
                    None,
                    || {
                        if completed {
                            Ok(())
                        } else {
                            Err(io::Error::other("owned retirement stop"))
                        }
                    },
                );
                assert_eq!(result.is_ok(), completed);
                if completed {
                    reg.finish(&fixture.path()).unwrap();
                    let current = fixture.read();
                    assert!(
                        !current
                            .links()
                            .iter()
                            .any(|link| link.object.path == retired)
                    );
                    fixture.assert_objects(&current);
                } else {
                    reg.recover().unwrap();
                    assert_eq!(fs::read(fixture.path()).unwrap(), before);
                }
                assert!(fs::symlink_metadata(&retired).is_err());
                assert_eq!(fs::read(&fixture.links[0].source).unwrap(), b"instructions");
            }
        }
    }

    #[test]
    fn absent_retirement_never_authorizes_foreign_arrival_adopted_or_unrelated_names() {
        for case in [
            "foreign-file",
            "foreign-link",
            "adopted",
            "unrelated",
            "duplicate",
            "still-desired",
        ] {
            let fixture = Fixture::new();
            fixture.publish_fresh();
            let prior = fixture.read();
            let retired = fixture.links[0].destination.clone();
            fs::remove_file(&retired).unwrap();
            let before = fs::read(fixture.path()).unwrap();
            let mut desired = fixture.links[1..].to_vec();
            let nominations = match case {
                "adopted" => {
                    desired = fixture.links[2..].to_vec();
                    fs::remove_file(&fixture.links[1].destination).unwrap();
                    vec![retired.clone(), fixture.links[1].destination.clone()]
                }
                "unrelated" => vec![fixture.root.join("not-recorded")],
                "duplicate" => vec![retired.clone(), retired.clone()],
                "still-desired" => {
                    desired = fixture.links.clone();
                    vec![retired.clone()]
                }
                _ => vec![retired.clone()],
            };
            let reg = fixture.registration();
            assert!(
                reg.apply_with_metadata(
                    &desired,
                    &[],
                    &[],
                    &[],
                    prior.destination().unwrap(),
                    |view| {
                        match case {
                            "foreign-file" => fs::write(&retired, b"foreign arrival")?,
                            "foreign-link" => symlink_file(&fixture.links[0].source, &retired)?,
                            _ => {}
                        }
                        encode_retiring_absent(
                            &fixture.settings,
                            &desired,
                            view,
                            Previous::Native(&prior),
                            &nominations,
                        )
                    }
                )
                .is_err(),
                "{case}"
            );
            assert_eq!(fs::read(fixture.path()).unwrap(), before);
            assert!(!reg.journal_path().exists());
            assert_eq!(fixture.links[1].destination.is_symlink(), case != "adopted");
            if case == "foreign-file" {
                assert_eq!(fs::read(&retired).unwrap(), b"foreign arrival");
            }
            if case == "foreign-link" {
                assert_eq!(fs::read_link(&retired).unwrap(), fixture.links[0].source);
            }
        }
    }

    #[test]
    fn missing_owned_repair_rechecks_a_foreign_arrival_before_durable_intent() {
        let fixture = Fixture::new();
        fixture.publish_fresh();
        let prior = fixture.read();
        fs::remove_file(&fixture.links[0].destination).unwrap();
        let before = fs::read(fixture.path()).unwrap();
        let mut foreign_id = None;
        let reg = fixture.registration();
        assert!(
            reg.apply_with_metadata(
                &fixture.links,
                &[],
                &[],
                &[],
                prior.destination().unwrap(),
                |view| {
                    let bytes = encode(
                        &fixture.settings,
                        &fixture.links,
                        view,
                        Previous::Native(&prior),
                    )?;
                    symlink_file(&fixture.links[0].source, &fixture.links[0].destination)?;
                    foreign_id = Some(
                        FileGuard::capture_link(&fixture.links[0].destination)?
                            .object_identity()?,
                    );
                    Ok(bytes)
                }
            )
            .is_err()
        );
        assert!(foreign_id.is_some());
        assert_eq!(
            FileGuard::capture_link(&fixture.links[0].destination)
                .unwrap()
                .object_identity()
                .unwrap(),
            foreign_id.unwrap()
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), before);
        assert!(!fixture.root.join("journal-state").join(JOURNAL).exists());
    }

    #[test]
    fn missing_owned_repair_interruption_recovers_to_absence_and_original_metadata() {
        let fixture = Fixture::new();
        fixture.publish_fresh();
        let prior = fixture.read();
        fs::remove_file(&fixture.links[0].destination).unwrap();
        let before = fs::read(fixture.path()).unwrap();
        let reg = fixture.registration();
        let mut reached = false;
        assert!(
            reg.test_apply_metadata(
                &fixture.links,
                prior.destination().unwrap(),
                |view| encode(
                    &fixture.settings,
                    &fixture.links,
                    view,
                    Previous::Native(&prior)
                ),
                || {
                    reached = true;
                    Err(io::Error::other("owned interruption after publication"))
                }
            )
            .is_err()
        );
        assert!(reached);
        assert!(fixture.links[0].destination.exists());
        assert_ne!(fs::read(fixture.path()).unwrap(), before);
        assert!(fixture.root.join("journal-state").join(JOURNAL).exists());
        let report = reg.recover().unwrap();
        assert!(!report.committed);
        assert!(!fixture.links[0].destination.exists());
        assert_eq!(fs::read(fixture.path()).unwrap(), before);
        assert!(!fixture.root.join("journal-state").join(JOURNAL).exists());
        assert_eq!(
            fs::read(fixture.settings.source_root.join("skill-one/keep")).unwrap(),
            b"source skill stays"
        );
        assert_eq!(
            fs::read_link(&fixture.links[1].destination).unwrap(),
            fixture.links[1].source
        );
    }

    #[test]
    fn held_metadata_witness_rejects_stale_prior_bytes_and_false_fresh_claim() {
        let fixture = Fixture::new();
        fixture.publish_fresh();
        let prior = fixture.read();
        let mut foreign: Document =
            serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
        foreign
            .settings
            .versions
            .insert("external-change".into(), "preserve".into());
        foreign.checksum = foreign.checksum().unwrap();
        let bytes = serde_json::to_vec(&foreign).unwrap();
        fs::write(fixture.path(), &bytes).unwrap();
        // Same file ID and valid current document, but a different whole-file
        // baseline than the stale semantic reader retained before publication.
        let current = ConfigSnapshot::read(&fixture.path()).unwrap();
        assert_eq!(current.fingerprint().0, prior.snapshot.fingerprint().0);
        assert_ne!(current.fingerprint().1, prior.snapshot.fingerprint().1);
        let reg = fixture.registration();
        for fresh in [false, true] {
            assert!(
                reg.apply_with_metadata(
                    &fixture.links,
                    &[],
                    &[],
                    &[],
                    MetadataDestination::existing(&current).unwrap(),
                    |view| encode(
                        &fixture.settings,
                        &fixture.links,
                        view,
                        if fresh {
                            Previous::Fresh
                        } else {
                            Previous::Native(&prior)
                        }
                    )
                )
                .is_err()
            );
            assert_eq!(fs::read(fixture.path()).unwrap(), bytes);
            assert!(!fixture.root.join("journal-state").join(JOURNAL).exists());
        }
        fixture.assert_objects(&prior);
    }
}
