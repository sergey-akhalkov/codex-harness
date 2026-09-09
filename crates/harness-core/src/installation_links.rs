//! Read-only owned skill-link guard for an already locked installation.
//!
//! The public receipt names selected skills and their source/destination paths
//! plus schema/identity assurance. It never returns private metadata bytes.
#![cfg(windows)]

use crate::{
    installation_lock::InstallationLock,
    installation_metadata::InstallationMetadata,
    installation_state::{LegacyInstallation, key, normal, portable_name},
    inventory,
    registration::LinkType,
    registration_native::{FileGuard, targets_match},
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

const MAX_SKILL_NAMES: usize = 64;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Public skill destination receipt. Source is the expected kit checkout path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnedSkillReceipt {
    pub name: String,
    pub source: PathBuf,
    pub destination: PathBuf,
}

/// Schema and identity assurance for one captured operation. Schema 1 never
/// stored historical object IDs; the captured IDs are current, not prior proof.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdentityAssurance {
    Schema1CurrentCapture,
    Schema2StoredIdentity,
}

/// Serializable public receipt. Private metadata bytes stay off this type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnedSkillLinksReceipt {
    pub schema: u32,
    pub identity: IdentityAssurance,
    pub skills: Vec<OwnedSkillReceipt>,
}

enum Metadata {
    Schema1(LegacyInstallation),
    Schema2(InstallationMetadata),
}

struct GuardedSkill {
    name: String,
    source: PathBuf,
    destination: PathBuf,
    expected_target: PathBuf,
    directory: bool,
    identity: crate::registration_native::LinkIdentity,
    guard: FileGuard,
}

/// Holds the installation lock, metadata snapshot and live skill-link objects
/// until `verify_unchanged`. Drop preserves every captured input.
pub struct OwnedSkillLinks {
    _lock: InstallationLock,
    metadata: Metadata,
    skills: Vec<GuardedSkill>,
}

impl OwnedSkillLinks {
    /// Capture named owned skill links for an already locked installation.
    ///
    /// `source_root`, `codex_home`, `user_home` and `dependency_user_home` are
    /// the explicit request spellings. Canonical `\\?\` inventory results are
    /// not accepted as those inputs; pass the original Disk-prefix homes.
    /// The lock must already match `user_home` as the legacy installer does.
    pub fn read(
        source_root: &Path,
        codex_home: &Path,
        user_home: &Path,
        dependency_user_home: &Path,
        skill_names: &[&str],
        lock: InstallationLock,
    ) -> io::Result<Self> {
        let names = bound_names(skill_names)?;
        let source_root = normal(source_root)?;
        let codex_home = normal(codex_home)?;
        let user_home = normal(user_home)?;
        let dependency_user_home = normal(dependency_user_home)?;
        refuse_pending(&codex_home)?;
        let inventory = inventory::read(&source_root, &codex_home, &user_home)?;
        if !same_place(&inventory.source_root, &source_root)? {
            return Err(invalid(
                "source-root identity does not match the kit inventory",
            ));
        }
        let metadata = load_metadata(&codex_home, &user_home, &dependency_user_home)?;
        match &metadata {
            Metadata::Schema1(legacy) => {
                let header = legacy.summary();
                if key(header.source_root)? != key(&source_root)?
                    || key(header.codex_home)? != key(&codex_home)?
                    || key(header.user_home)? != key(&user_home)?
                    || key(header.dependency_user_home)? != key(&dependency_user_home)?
                {
                    return Err(invalid(
                        "installation homes or source do not match the request",
                    ));
                }
            }
            Metadata::Schema2(native) => {
                let settings = native.settings();
                if key(&settings.source_root)? != key(&source_root)?
                    || key(&settings.codex_home)? != key(&codex_home)?
                    || key(&settings.user_home)? != key(&user_home)?
                    || key(&settings.dependency_user_home)? != key(&dependency_user_home)?
                {
                    return Err(invalid(
                        "installation homes or source do not match the request",
                    ));
                }
            }
        }
        let skills = names
            .iter()
            .map(|name| capture_skill(name, &inventory, &metadata))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            _lock: lock,
            metadata,
            skills,
        })
    }

    pub fn receipt(&self) -> OwnedSkillLinksReceipt {
        let (schema, identity) = match &self.metadata {
            Metadata::Schema1(_) => (1, IdentityAssurance::Schema1CurrentCapture),
            Metadata::Schema2(_) => (2, IdentityAssurance::Schema2StoredIdentity),
        };
        OwnedSkillLinksReceipt {
            schema,
            identity,
            skills: self
                .skills
                .iter()
                .map(|skill| OwnedSkillReceipt {
                    name: skill.name.clone(),
                    source: skill.source.clone(),
                    destination: skill.destination.clone(),
                })
                .collect(),
        }
    }

    /// Recheck the captured metadata snapshot and each live skill-link object.
    /// This does not publish, normalize or follow link targets.
    pub fn verify_unchanged(&self) -> io::Result<()> {
        match &self.metadata {
            Metadata::Schema1(legacy) => legacy.verify_unchanged()?,
            Metadata::Schema2(native) => native.verify_unchanged()?,
        }
        for skill in &self.skills {
            let (target, directory) = skill.guard.link_description()?;
            if directory != skill.directory
                || !targets_match(&target, &skill.expected_target)?
                || skill.guard.object_identity()? != skill.identity
            {
                return Err(invalid(
                    "owned skill link identity or target changed; preserving it",
                ));
            }
        }
        Ok(())
    }
}

fn bound_names(skill_names: &[&str]) -> io::Result<Vec<String>> {
    if skill_names.is_empty() || skill_names.len() > MAX_SKILL_NAMES {
        return Err(invalid("skill name list is empty or exceeds its bound"));
    }
    let mut names = Vec::new();
    let mut seen = BTreeSet::new();
    for name in skill_names {
        if !portable_name(name) || !seen.insert(*name) {
            return Err(invalid("skill names must be unique portable names"));
        }
        names.push((*name).to_owned());
    }
    Ok(names)
}

fn refuse_pending(codex_home: &Path) -> io::Result<()> {
    match fs::symlink_metadata(codex_home.join("harness/pending.json")) {
        Ok(_) => Err(io::Error::other(
            "installation has pending recovery; preserve it and recover first",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Inventory may return canonical `\\?\` spellings. Compare those to Disk-prefix
/// request/metadata paths without relaxing `normal()` on explicit inputs.
fn same_place(left: &Path, right: &Path) -> io::Result<bool> {
    targets_match(left, right)
}

fn load_metadata(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> io::Result<Metadata> {
    match InstallationMetadata::read(codex_home, user_home, dependency_user_home) {
        Ok(Some(native)) => return Ok(Metadata::Schema2(native)),
        Ok(None) => {}
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {}
        Err(error) => return Err(error),
    }
    match LegacyInstallation::read(codex_home, user_home, dependency_user_home)? {
        Some(legacy) => Ok(Metadata::Schema1(legacy)),
        None => Err(invalid("installation metadata is absent; preserving it")),
    }
}

fn capture_skill(
    name: &str,
    inventory: &inventory::Inventory,
    metadata: &Metadata,
) -> io::Result<GuardedSkill> {
    let expected = inventory
        .links
        .iter()
        .find(|link| link.kind == "skill" && link.name == name)
        .ok_or_else(|| invalid("required skill is not an expected kit source link"))?;
    let (destination, expected_target, owned) = match metadata {
        Metadata::Schema1(legacy) => {
            let recorded = legacy
                .links()
                .iter()
                .find(|link| link.kind == "skill" && link.name == name)
                .ok_or_else(|| invalid("required skill is absent from installation metadata"))?;
            if !same_place(&recorded.destination, &expected.destination)? {
                return Err(invalid(
                    "recorded skill destination is not the expected kit destination",
                ));
            }
            (
                recorded.destination.clone(),
                recorded.source.clone(),
                recorded.owned,
            )
        }
        Metadata::Schema2(native) => {
            let recorded = native
                .links()
                .iter()
                .find(|link| link.kind == "skill" && link.name == name)
                .ok_or_else(|| invalid("required skill is absent from installation metadata"))?;
            if !same_place(&recorded.object.path, &expected.destination)? {
                return Err(invalid(
                    "recorded skill destination is not the expected kit destination",
                ));
            }
            (
                recorded.object.path.clone(),
                recorded.object.target.clone(),
                recorded.owned,
            )
        }
    };
    if !owned {
        return Err(invalid(
            "required skill is unowned or adopted; preserving it",
        ));
    }
    if !same_place(&expected_target, &expected.source)? {
        return Err(invalid(
            "required skill is not the expected kit source link",
        ));
    }
    let guard = FileGuard::capture_link(&destination).map_err(|_| {
        invalid("required skill link is absent or not a captured registration link")
    })?;
    let (live_target, directory) = guard.link_description()?;
    if !directory {
        return Err(invalid("required skill link type is not a directory"));
    }
    let identity = guard.object_identity()?;
    match metadata {
        Metadata::Schema1(_) => {
            if !targets_match(&live_target, &expected_target)? {
                return Err(invalid(
                    "live skill link target is not the expected kit source",
                ));
            }
        }
        Metadata::Schema2(native) => {
            let recorded = native
                .links()
                .iter()
                .find(|link| link.kind == "skill" && link.name == name)
                .expect("schema2 skill was selected");
            if recorded.object.link_type != LinkType::Directory
                || recorded.object.identity != identity
                || !targets_match(&live_target, &recorded.object.target)?
                || !targets_match(&recorded.object.target, &expected_target)?
            {
                return Err(invalid(
                    "live skill link is a same-target foreign replacement; preserving it",
                ));
            }
        }
    }
    Ok(GuardedSkill {
        name: name.to_owned(),
        source: expected.source.clone(),
        destination,
        expected_target,
        directory,
        identity,
        guard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        installation_metadata::{InstallationMetadata, Previous, Settings, encode},
        installation_state::PathScope,
        inventory::{Connection, Link},
        registration::{JOURNAL, Registration, metadata::MetadataDestination},
        registration_native::FileGuard,
    };
    use serde_json::json;
    use std::{
        collections::BTreeMap,
        fs,
        os::windows::fs::{symlink_dir, symlink_file},
        path::PathBuf,
    };

    const SKILLS: &[&str] = &["project-verification", "reproduce-regression"];

    struct NativeFixture {
        root: PathBuf,
        source: PathBuf,
        home: PathBuf,
        user: PathBuf,
        settings: Settings,
        links: Vec<Link>,
    }

    impl NativeFixture {
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("harness-installation-links-")
                .tempdir()
                .unwrap()
                .keep();
            println!("installation-links evidence: {}", root.display());
            let source = root.join("source");
            let home = root.join("codex");
            let user = root.join("user");
            write_kit(&source);
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&user).unwrap();
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
                    name: "AGENTS".into(),
                    source: source.join("global/principles-of-work.md"),
                    destination: home.join("AGENTS.md"),
                    connection: Connection::Missing,
                },
                Link {
                    kind: "profile".into(),
                    name: "harness".into(),
                    source: source.join("global/harness.config.toml"),
                    destination: home.join("harness.config.toml"),
                    connection: Connection::Missing,
                },
                Link {
                    kind: "agents".into(),
                    name: "codex-harness".into(),
                    source: source.join("global/agents"),
                    destination: home.join("agents/codex-harness"),
                    connection: Connection::Missing,
                },
                skill_link(&source, &user, "project-verification"),
                skill_link(&source, &user, "reproduce-regression"),
            ];
            Self {
                root,
                source,
                home,
                user,
                settings,
                links,
            }
        }

        fn path(&self) -> PathBuf {
            self.home.join("harness/installation.json")
        }

        fn registration(&self) -> Registration {
            Registration::open(&self.root.join("journal-state")).unwrap()
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

        fn lock(&self) -> InstallationLock {
            InstallationLock::acquire(&self.user).unwrap()
        }

        fn read(&self, lock: InstallationLock) -> io::Result<OwnedSkillLinks> {
            OwnedSkillLinks::read(
                &self.source,
                &self.home,
                &self.user,
                &self.user,
                SKILLS,
                lock,
            )
        }
    }

    struct LegacyFixture {
        root: PathBuf,
        source: PathBuf,
        home: PathBuf,
        user: PathBuf,
    }

    impl LegacyFixture {
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("harness-installation-links-legacy-")
                .tempdir()
                .unwrap()
                .keep();
            println!("installation-links legacy evidence: {}", root.display());
            let source = root.join("source");
            let home = root.join("codex");
            let user = root.join("user");
            write_kit(&source);
            fs::create_dir_all(home.join("harness")).unwrap();
            fs::create_dir_all(home.join("agents")).unwrap();
            fs::create_dir_all(user.join(".agents/skills")).unwrap();
            symlink_file(
                source.join("global/principles-of-work.md"),
                home.join("AGENTS.md"),
            )
            .unwrap();
            symlink_file(
                source.join("global/harness.config.toml"),
                home.join("harness.config.toml"),
            )
            .unwrap();
            symlink_dir(
                source.join("global/agents"),
                home.join("agents/codex-harness"),
            )
            .unwrap();
            for name in SKILLS {
                symlink_dir(
                    source.join(".agents/skills").join(name),
                    user.join(".agents/skills").join(name),
                )
                .unwrap();
            }
            Self {
                root,
                source,
                home,
                user,
            }
        }

        fn write_state(&self, owned: bool) {
            let links = vec![
                json!({
                    "kind": "instructions",
                    "name": "AGENTS",
                    "source": self.source.join("global/principles-of-work.md"),
                    "destination": self.home.join("AGENTS.md"),
                    "owned": true
                }),
                json!({
                    "kind": "profile",
                    "name": "harness",
                    "source": self.source.join("global/harness.config.toml"),
                    "destination": self.home.join("harness.config.toml"),
                    "owned": true
                }),
                json!({
                    "kind": "agents",
                    "name": "codex-harness",
                    "source": self.source.join("global/agents"),
                    "destination": self.home.join("agents/codex-harness"),
                    "owned": true
                }),
                json!({
                    "kind": "skill",
                    "name": "project-verification",
                    "source": self.source.join(".agents/skills/project-verification"),
                    "destination": self.user.join(".agents/skills/project-verification"),
                    "owned": owned
                }),
                json!({
                    "kind": "skill",
                    "name": "reproduce-regression",
                    "source": self.source.join(".agents/skills/reproduce-regression"),
                    "destination": self.user.join(".agents/skills/reproduce-regression"),
                    "owned": true
                }),
            ];
            fs::write(
                self.home.join("harness/installation.json"),
                serde_json::to_vec_pretty(&json!({
                    "schemaVersion": 1,
                    "sourceRoot": self.source,
                    "codexHome": self.home,
                    "userHome": self.user,
                    "dependencyUserHome": self.user,
                    "codexCommand": self.root.join("upstream.exe"),
                    "profileName": "harness",
                    "pathScope": "User",
                    "pathAdded": false,
                    "versions": {},
                    "links": links
                }))
                .unwrap(),
            )
            .unwrap();
        }

        fn lock(&self) -> InstallationLock {
            InstallationLock::acquire(&self.user).unwrap()
        }

        fn read(&self, lock: InstallationLock) -> io::Result<OwnedSkillLinks> {
            OwnedSkillLinks::read(
                &self.source,
                &self.home,
                &self.user,
                &self.user,
                SKILLS,
                lock,
            )
        }
    }

    fn write_kit(source: &Path) {
        for path in [
            "global/agents",
            ".agents/skills/project-verification",
            ".agents/skills/reproduce-regression",
        ] {
            fs::create_dir_all(source.join(path)).unwrap();
        }
        for (path, body) in [
            ("global/harness.config.toml", "profile = true\n"),
            ("global/principles-of-work.md", "# principles\n"),
            ("global/hooks.json", "{}\n"),
            ("global/rtk-hooks.json", "{}\n"),
        ] {
            fs::write(source.join(path), body).unwrap();
        }
        fs::write(
            source.join("global/kit.json"),
            serde_json::to_vec(&json!({
                "schema": 1,
                "profile_name": "harness",
                "profile": "global/harness.config.toml",
                "instructions": "global/principles-of-work.md",
                "skills": ".agents/skills",
                "agents": "global/agents",
                "hooks": "global/hooks.json",
                "token_hooks": "global/rtk-hooks.json"
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            source.join("global/agents/example.toml"),
            "name = 'example'\n",
        )
        .unwrap();
        for name in SKILLS {
            fs::write(
                source.join(".agents/skills").join(name).join("SKILL.md"),
                format!("---\nname: {name}\n---\n# {name}\n"),
            )
            .unwrap();
        }
    }

    fn skill_link(source: &Path, user: &Path, name: &str) -> Link {
        Link {
            kind: "skill".into(),
            name: name.into(),
            source: source.join(".agents/skills").join(name),
            destination: user.join(".agents/skills").join(name),
            connection: Connection::Missing,
        }
    }

    fn assert_receipt(
        guard: &OwnedSkillLinks,
        schema: u32,
        fixture_source: &Path,
        fixture_user: &Path,
    ) {
        let receipt = guard.receipt();
        assert_eq!(receipt.schema, schema);
        assert_eq!(
            receipt.identity,
            if schema == 1 {
                IdentityAssurance::Schema1CurrentCapture
            } else {
                IdentityAssurance::Schema2StoredIdentity
            }
        );
        assert_eq!(receipt.skills.len(), 2);
        for (skill, name) in receipt.skills.iter().zip(SKILLS) {
            assert_eq!(skill.name, *name);
            assert_eq!(
                skill.source.canonicalize().unwrap(),
                fixture_source
                    .join(".agents/skills")
                    .join(name)
                    .canonicalize()
                    .unwrap()
            );
            assert_eq!(
                skill.destination,
                fixture_user.join(".agents/skills").join(name)
            );
        }
        let encoded = serde_json::to_string(&receipt).unwrap();
        assert!(!encoded.contains("volume_serial_number"));
        assert!(!encoded.contains("file_id"));
        assert!(!encoded.contains("checksum"));
        guard.verify_unchanged().unwrap();
    }

    #[test]
    fn schema1_owned_skills_capture_current_identity_without_historical_proof() {
        let fixture = LegacyFixture::new();
        fixture.write_state(true);
        let lock = fixture.lock();
        let original = fs::read(fixture.home.join("harness/installation.json")).unwrap();
        let guard = fixture.read(lock).unwrap();
        assert_receipt(&guard, 1, &fixture.source, &fixture.user);
        assert_eq!(
            fs::read(fixture.home.join("harness/installation.json")).unwrap(),
            original
        );
        for name in SKILLS {
            assert_eq!(
                fs::read_link(fixture.user.join(".agents/skills").join(name)).unwrap(),
                fixture.source.join(".agents/skills").join(name)
            );
        }
    }

    #[test]
    fn schema2_owned_skills_compare_stored_target_type_and_object_id() {
        let fixture = NativeFixture::new();
        fixture.publish_fresh();
        let lock = fixture.lock();
        let original = fs::read(fixture.path()).unwrap();
        let metadata = InstallationMetadata::read(&fixture.home, &fixture.user, &fixture.user)
            .unwrap()
            .unwrap();
        for name in SKILLS {
            let recorded = metadata
                .links()
                .iter()
                .find(|link| link.name == *name)
                .unwrap();
            let live = FileGuard::capture_link(&recorded.object.path).unwrap();
            assert_eq!(live.object_identity().unwrap(), recorded.object.identity);
        }
        let guard = fixture.read(lock).unwrap();
        assert_receipt(&guard, 2, &fixture.source, &fixture.user);
        assert_eq!(fs::read(fixture.path()).unwrap(), original);
    }

    #[test]
    fn absent_unowned_and_wrong_source_are_refused() {
        let missing = NativeFixture::new();
        let lock = missing.lock();
        assert!(missing.read(lock).is_err());

        let unowned = LegacyFixture::new();
        unowned.write_state(false);
        let lock = unowned.lock();
        assert!(unowned.read(lock).is_err());

        let wrong = NativeFixture::new();
        wrong.publish_fresh();
        let other = tempfile::Builder::new()
            .prefix("harness-installation-links-other-")
            .tempdir()
            .unwrap()
            .keep();
        write_kit(&other.join("source"));
        let lock = wrong.lock();
        assert!(
            OwnedSkillLinks::read(
                &other.join("source"),
                &wrong.home,
                &wrong.user,
                &wrong.user,
                SKILLS,
                lock,
            )
            .is_err()
        );
        let pending = LegacyFixture::new();
        pending.write_state(true);
        fs::write(
            pending.home.join("harness/pending.json"),
            b"owned unresolved",
        )
        .unwrap();
        let original = fs::read(pending.home.join("harness/installation.json")).unwrap();
        let lock = pending.lock();
        assert!(pending.read(lock).is_err());
        assert_eq!(
            fs::read(pending.home.join("harness/installation.json")).unwrap(),
            original
        );
        assert_eq!(
            fs::read(pending.home.join("harness/pending.json")).unwrap(),
            b"owned unresolved"
        );
    }

    #[test]
    fn schema2_same_target_foreign_replacement_is_refused() {
        let fixture = NativeFixture::new();
        fixture.publish_fresh();
        let original = fs::read(fixture.path()).unwrap();
        let destination = fixture.user.join(".agents/skills/project-verification");
        let old_id = FileGuard::capture_link(&destination)
            .unwrap()
            .object_identity()
            .unwrap();
        fs::remove_dir(&destination).unwrap();
        symlink_dir(
            fixture.source.join(".agents/skills/project-verification"),
            &destination,
        )
        .unwrap();
        let foreign_id = FileGuard::capture_link(&destination)
            .unwrap()
            .object_identity()
            .unwrap();
        assert_ne!(foreign_id, old_id);
        let lock = fixture.lock();
        assert!(fixture.read(lock).is_err());
        assert_eq!(fs::read(fixture.path()).unwrap(), original);
        assert_eq!(
            FileGuard::capture_link(&destination)
                .unwrap()
                .object_identity()
                .unwrap(),
            foreign_id
        );
    }

    #[test]
    fn verify_unchanged_rejects_metadata_and_live_link_edits() {
        let native = NativeFixture::new();
        native.publish_fresh();
        let lock = native.lock();
        let guard = native.read(lock).unwrap();
        guard.verify_unchanged().unwrap();
        fs::write(native.path(), b"foreign metadata replacement").unwrap();
        assert!(guard.verify_unchanged().is_err());
        drop(guard);

        let fixture = NativeFixture::new();
        fixture.publish_fresh();
        let lock = fixture.lock();
        let guard = fixture.read(lock).unwrap();
        let destination = fixture.user.join(".agents/skills/reproduce-regression");
        assert!(fs::remove_dir(&destination).is_err());
        guard.verify_unchanged().unwrap();
        drop(guard);
        fs::remove_dir(&destination).unwrap();
        symlink_dir(
            fixture.source.join(".agents/skills/reproduce-regression"),
            &destination,
        )
        .unwrap();
        let lock = fixture.lock();
        assert!(fixture.read(lock).is_err());

        let legacy = LegacyFixture::new();
        legacy.write_state(true);
        let lock = legacy.lock();
        let guard = legacy.read(lock).unwrap();
        fs::write(
            legacy.home.join("harness/installation.json"),
            b"foreign schema1 replacement",
        )
        .unwrap();
        assert!(guard.verify_unchanged().is_err());
    }

    #[test]
    fn duplicate_or_unbounded_names_are_refused_before_capture() {
        let fixture = NativeFixture::new();
        fixture.publish_fresh();
        let lock = fixture.lock();
        assert!(
            OwnedSkillLinks::read(
                &fixture.source,
                &fixture.home,
                &fixture.user,
                &fixture.user,
                &["project-verification", "project-verification"],
                lock,
            )
            .is_err()
        );
        let lock = fixture.lock();
        assert!(
            OwnedSkillLinks::read(
                &fixture.source,
                &fixture.home,
                &fixture.user,
                &fixture.user,
                &[],
                lock,
            )
            .is_err()
        );
    }
}
