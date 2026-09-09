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
    fn validate(&self) -> io::Result<()> {
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

    fn destination(&self, kind: &str, name: &str) -> io::Result<PathBuf> {
        if !portable_name(name) {
            return Err(invalid());
        }
        Ok(match kind {
            "instructions" => self.codex_home.join("AGENTS.md"),
            "profile" => self.codex_home.join("harness.config.toml"),
            "agents" => self.codex_home.join("agents/codex-harness"),
            "hooks" => self.codex_home.join("hooks.json"),
            "skill" => self.user_home.join(".agents/skills").join(name),
            // Transitional entries can be retained while the installer migrates
            // their consumers. They are not proof of completed script retirement.
            "launcher" => self.codex_home.join("harness/bin/codex.ps1"),
            "diagnostic-launcher" => self.codex_home.join("harness/bin/codex-harness-check.ps1"),
            "hook-launcher" => self.codex_home.join("harness/bin/hook.ps1"),
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
            let expected = self.settings.destination(&link.kind, &link.name)?;
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

/// Called only within `apply_with_metadata`, while its old/new IDs are guarded.
/// Every desired destination must occur in the final view. Existing objects are
/// adopted unless a matching verified previous claim establishes ownership.
pub(crate) fn encode(
    settings: &Settings,
    desired: &[Link],
    view: &MetadataView,
    previous: Previous<'_>,
) -> io::Result<Vec<u8>> {
    settings.validate()?;
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
                } else if link.owned {
                    return Err(invalid());
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
                } else if link.owned {
                    return Err(invalid());
                }
            }
        }
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
