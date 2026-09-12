//! Direct source links and existing configuration changes with an owned,
//! exact-content journal and guarded rollback.
//!
//! Installer activation can journal PATH and verify the runtime before
//! completion. Selection of components and ownership belongs to its caller.
#![cfg(windows)]

use crate::{
    build_identity,
    config_create::{ConfigCreation, CreatedConfig},
    config_file::{ConfigChange, ConfigRecord},
    inventory::{self, Link},
    process::ExclusiveFileLock,
    registration_native::{self as native, FileGuard, LinkIdentity, StagedFile, StagedLink},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

#[path = "registration_changes.rs"]
mod changes;
use changes::ChangeRecord;
pub use changes::LinkChange;
#[path = "registration_finish.rs"]
mod finish;
pub use finish::COMMIT;
#[path = "registration_metadata.rs"]
pub(crate) mod metadata;

pub const JOURNAL: &str = "journal.json";
// Schema 11 distinguishes process-owned PATH from registry PATH. Older readers
// must refuse it rather than attempt the wrong environment's inverse.
pub const SCHEMA: u32 = 11;
pub const COMPLETION: &str = "complete.json";
const MAX_JOURNAL: u64 = 4 * 1024 * 1024;

#[cfg(test)]
mod schema_compatibility_tests {
    use super::*;

    #[test]
    fn schema8_9_10_can_recover_and_finish_but_cannot_smuggle_new_actions() {
        for schema in [8, 9, 10] {
            for completed in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let reg = Registration::open(&root.path().join("state")).unwrap();
                let metadata = root.path().join("metadata");
                reg.apply_with_files(
                    &[],
                    &[],
                    &[ConfigCreation::new(&metadata, b"metadata").unwrap()],
                )
                .unwrap();
                let mut journal: Journal =
                    serde_json::from_slice(&fs::read(reg.journal_path()).unwrap()).unwrap();
                journal.schema = schema;
                journal.checksum = journal.calculate_checksum().unwrap();
                let bytes = serde_json::to_vec(&journal).unwrap();
                fs::write(reg.journal_path(), &bytes).unwrap();
                if completed {
                    fs::write(
                        reg.state.join(COMPLETION),
                        serde_json::to_vec(&Completion {
                            schema,
                            journal_sha256: build_identity::hash_bytes(&bytes),
                        })
                        .unwrap(),
                    )
                    .unwrap();
                    assert!(reg.recover_installation(&metadata).unwrap().committed);
                    assert_eq!(fs::read(&metadata).unwrap(), b"metadata");
                } else {
                    fs::remove_file(reg.state.join(COMPLETION)).unwrap();
                    assert!(!reg.recover_installation(&metadata).unwrap().committed);
                    assert!(!metadata.exists());
                }
                assert!(!reg.journal_path().exists());
                if schema == 8 {
                    journal.path_change =
                        Some(serde_json::from_str(r#"{"before":null,"after":null}"#).unwrap());
                    journal.checksum = journal.calculate_checksum().unwrap();
                    assert!(journal.verify().is_err());
                    journal.path_change = None;
                }
                journal.retire_configuration = Some(metadata);
                journal.checksum = journal.calculate_checksum().unwrap();
                assert!(journal.verify().is_err());
                journal.retire_configuration = None;
                journal.process_path = Some(
                    crate::process_path::ProcessPathSnapshot::read()
                        .unwrap()
                        .prepend(Path::new("C:\\owned-process-compatibility\\bin"))
                        .unwrap()
                        .0,
                );
                journal.checksum = journal.calculate_checksum().unwrap();
                assert!(journal.verify().is_err());
            }
        }
    }

    #[test]
    fn schema10_metadata_retirement_remains_recoverable() {
        let root = tempfile::tempdir().unwrap();
        let metadata = root.path().join("metadata");
        fs::write(&metadata, b"owner witness").unwrap();
        let snapshot = crate::config_file::ConfigSnapshot::read(&metadata).unwrap();
        let reg = Registration::open(&root.path().join("state")).unwrap();
        reg.apply_disconnection(
            &[],
            &snapshot,
            |_| Ok(snapshot.contents().to_vec()),
            None,
            || Ok(()),
        )
        .unwrap();
        let mut journal: Journal =
            serde_json::from_slice(&fs::read(reg.journal_path()).unwrap()).unwrap();
        journal.schema = 10;
        journal.checksum = journal.calculate_checksum().unwrap();
        let bytes = serde_json::to_vec(&journal).unwrap();
        fs::write(reg.journal_path(), &bytes).unwrap();
        fs::write(
            reg.state.join(COMPLETION),
            serde_json::to_vec(&Completion {
                schema: 10,
                journal_sha256: build_identity::hash_bytes(&bytes),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(reg.recover_installation(&metadata).unwrap().committed);
        assert!(!metadata.exists());
    }
}
const OWNER: &[u8] = b"codex-harness-registration-v1\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkType {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub path: PathBuf,
    #[serde(rename = "type")]
    pub link_type: LinkType,
    pub target: PathBuf,
    pub checksum: String,
    pub created: bool,
    staged: Option<Staged>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reused_identity: Option<LinkIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Staged {
    path: PathBuf,
    identity: LinkIdentity,
}

impl Record {
    pub fn new(
        path: PathBuf,
        link_type: LinkType,
        target: PathBuf,
        created: bool,
    ) -> io::Result<Self> {
        let checksum = checksum(&link_type, &path, &target, created)?;
        Ok(Self {
            path,
            link_type,
            target,
            checksum,
            created,
            staged: None,
            reused_identity: None,
        })
    }

    fn verify(&self) -> io::Result<()> {
        refuse_escape(&self.path)?;
        refuse_escape(&self.target)?;
        if self.created != self.staged.is_some() || (self.created && self.reused_identity.is_some())
        {
            return Err(invalid(
                "registration creation proof is missing or inconsistent",
            ));
        }
        if let Some(staged) = &self.staged {
            refuse_escape(&staged.path)?;
            if staged.path.parent() != self.path.parent() || staged.path == self.path {
                return Err(invalid("registration staging path changed; preserving it"));
            }
        }
        if self.checksum != checksum(&self.link_type, &self.path, &self.target, self.created)? {
            return Err(invalid(
                "registration journal ownership changed; preserving it",
            ));
        }
        Ok(())
    }

    fn expected_identity(&self) -> Option<&LinkIdentity> {
        self.staged
            .as_ref()
            .map(|stage| &stage.identity)
            .or(self.reused_identity.as_ref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub schema: u32,
    pub records: Vec<Record>,
    configurations: Vec<ConfigRecord>,
    creations: Vec<CreatedConfig>,
    link_changes: Vec<ChangeRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path_change: Option<crate::environment_path::UserPathChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retire_configuration: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    process_path: Option<crate::process_path::ProcessPathChange>,
    pub checksum: String,
}

impl Journal {
    fn new(
        records: Vec<Record>,
        configurations: Vec<ConfigRecord>,
        creations: Vec<CreatedConfig>,
        link_changes: Vec<ChangeRecord>,
    ) -> io::Result<Self> {
        let mut journal = Self {
            schema: SCHEMA,
            records,
            configurations,
            creations,
            link_changes,
            path_change: None,
            retire_configuration: None,
            process_path: None,
            checksum: String::new(),
        };
        journal.checksum = journal.calculate_checksum()?;
        Ok(journal)
    }

    fn calculate_checksum(&self) -> io::Result<String> {
        let bytes = if self.schema == 8 && self.path_change.is_none() {
            serde_json::to_vec(&(
                self.schema,
                &self.records,
                &self.configurations,
                &self.creations,
                &self.link_changes,
            ))?
        } else if self.schema == 9 {
            serde_json::to_vec(&(
                self.schema,
                &self.records,
                &self.configurations,
                &self.creations,
                &self.link_changes,
                &self.path_change,
            ))?
        } else if self.schema == 10 {
            serde_json::to_vec(&(
                self.schema,
                &self.records,
                &self.configurations,
                &self.creations,
                &self.link_changes,
                &self.path_change,
                &self.retire_configuration,
            ))?
        } else {
            serde_json::to_vec(&(
                self.schema,
                &self.records,
                &self.configurations,
                &self.creations,
                &self.link_changes,
                &self.path_change,
                &self.retire_configuration,
                &self.process_path,
            ))?
        };
        Ok(build_identity::hash_bytes(&bytes))
    }

    fn verify(&self) -> io::Result<()> {
        if ![8, 9, 10, SCHEMA].contains(&self.schema)
            || self.schema == 8 && self.path_change.is_some()
            || self.schema < 10 && self.retire_configuration.is_some()
            || self.schema < 11 && self.process_path.is_some()
            || self.path_change.is_some() && self.process_path.is_some()
        {
            return Err(invalid("unsupported registration journal; preserving it"));
        }
        if self.checksum != self.calculate_checksum()? {
            return Err(invalid(
                "registration journal ownership changed; preserving it",
            ));
        }
        if let Some(change) = &self.path_change {
            change.validate()?;
        }
        if let Some(change) = &self.process_path {
            change.validate()?;
        }
        if let Some(path) = &self.retire_configuration
            && self
                .configurations
                .iter()
                .filter(|c| &c.path == path)
                .count()
                != 1
        {
            return Err(invalid("retirement must bind one existing configuration"));
        }
        let mut seen = BTreeSet::new();
        for record in &self.records {
            record.verify()?;
            if !seen.insert(&record.path) {
                return Err(invalid("duplicate journal destination; preserving it"));
            }
        }
        for (index, record) in self.configurations.iter().enumerate() {
            refuse_escape(&record.path)?;
            record.validate()?;
            if self.configurations[..index]
                .iter()
                .any(|other| record.same_object(other))
            {
                return Err(invalid(
                    "duplicate configuration object identity; preserving journal",
                ));
            }
            for other in self.records.iter().map(|record| &record.path).chain(
                self.configurations[..index]
                    .iter()
                    .map(|record| &record.path),
            ) {
                if paths_overlap(&record.path, other)? {
                    return Err(invalid(
                        "overlapping configuration destination; preserving journal",
                    ));
                }
            }
        }
        for (index, record) in self.creations.iter().enumerate() {
            refuse_escape(&record.path)?;
            refuse_escape(&record.stage)?;
            record.validate()?;
            for other in self
                .records
                .iter()
                .map(|r| &r.path)
                .chain(self.configurations.iter().map(|r| &r.path))
                .chain(self.creations[..index].iter().map(|r| &r.path))
            {
                if paths_overlap(&record.path, other)? {
                    return Err(invalid(
                        "overlapping created configuration; preserving journal",
                    ));
                }
            }
        }
        for (index, record) in self.link_changes.iter().enumerate() {
            record.validate()?;
            for other in self
                .records
                .iter()
                .map(|r| &r.path)
                .chain(self.configurations.iter().map(|r| &r.path))
                .chain(self.creations.iter().map(|r| &r.path))
                .chain(self.link_changes[..index].iter().map(|r| &r.path))
            {
                if paths_overlap(&record.path, other)? {
                    return Err(invalid("overlapping changed link; preserving journal"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    schema: u32,
    journal_sha256: String,
}

struct Snapshot {
    journal: Journal,
    bytes: Vec<u8>,
    completion: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Created,
    Reused,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Applied {
    pub destination: PathBuf,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplyReport {
    pub links: Vec<Applied>,
    pub configurations: Vec<PathBuf>,
    pub changed_links: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndoReport {
    pub removed: Vec<PathBuf>,
    pub restored: Vec<PathBuf>,
    /// True means a durable installation decision was retained and only
    /// temporary rollback state was cleaned. No candidates were undone.
    pub committed: bool,
}

#[derive(Debug)]
pub struct Registration {
    state: PathBuf,
    _lock: ExclusiveFileLock,
}

impl Registration {
    /// Open explicit owned state. A populated unowned root is never adopted.
    pub fn open(state: &Path) -> io::Result<Self> {
        let state = resolve_state(state)?;
        ensure_owner(&state)?;
        let lock_path = state.join("registration.lock");
        inventory::ordinary_parents(&lock_path)?;
        let lock = ExclusiveFileLock::try_acquire(&lock_path)?.ok_or_else(|| {
            invalid("Another native registration operation owns this state; wait for it to finish.")
        })?;
        Ok(Self { state, _lock: lock })
    }

    /// Read-only entry for preview. Neither an owner marker nor a lock file is
    /// created; an incomplete/foreign state remains available for explicit repair.
    pub(crate) fn open_existing(state: &Path) -> io::Result<Option<Self>> {
        let state = resolve_state(state)?;
        match fs::symlink_metadata(&state) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
            Ok(_) => {}
        }
        let _owner = FileGuard::open_regular(&state.join("owner"), OWNER)?;
        let lock_path = state.join("registration.lock");
        inventory::ordinary_parents(&lock_path)?;
        build_identity::ordinary(&lock_path)?;
        let lock = ExclusiveFileLock::try_acquire_existing(&lock_path)?.ok_or_else(|| {
            invalid("Another native registration operation owns this state; wait for it to finish.")
        })?;
        Ok(Some(Self { state, _lock: lock }))
    }

    pub fn state(&self) -> &Path {
        &self.state
    }

    pub fn journal_path(&self) -> PathBuf {
        self.state.join(JOURNAL)
    }

    pub fn apply(&self, links: &[Link]) -> io::Result<ApplyReport> {
        self.apply_with_configs(links, &[])
    }

    /// Journals exact existing-file changes alongside direct source links.
    /// Completed disconnect restores these files; later foreign edits stop
    /// automatic recovery. Callers must explicitly choose this rollback policy.
    pub fn apply_with_configs(
        &self,
        links: &[Link],
        configurations: &[ConfigChange],
    ) -> io::Result<ApplyReport> {
        self.apply_with_files(links, configurations, &[])
    }

    /// Adds exclusively created ordinary files to the same recovery journal.
    /// Disconnect removes only the recorded object with its exact original data.
    pub fn apply_with_files(
        &self,
        links: &[Link],
        configurations: &[ConfigChange],
        creations: &[ConfigCreation],
    ) -> io::Result<ApplyReport> {
        self.apply_with_changes(links, configurations, creations, &[])
    }

    /// Adds explicitly authorized replacement/removal of captured existing
    /// links. Their original objects remain available for guarded rollback.
    pub fn apply_with_changes(
        &self,
        links: &[Link],
        configurations: &[ConfigChange],
        creations: &[ConfigCreation],
        link_changes: &[LinkChange],
    ) -> io::Result<ApplyReport> {
        self.apply_inner(links, configurations, creations, link_changes, || Ok(()))
    }

    fn apply_inner(
        &self,
        links: &[Link],
        configurations: &[ConfigChange],
        creations: &[ConfigCreation],
        link_changes: &[LinkChange],
        checkpoint: impl FnMut() -> io::Result<()>,
    ) -> io::Result<ApplyReport> {
        self.apply_prepared(
            links,
            configurations,
            creations,
            link_changes,
            None,
            None,
            checkpoint,
        )
    }

    #[allow(clippy::too_many_arguments)] // Typed publication categories share one producer.
    fn apply_prepared(
        &self,
        links: &[Link],
        configurations: &[ConfigChange],
        creations: &[ConfigCreation],
        link_changes: &[LinkChange],
        metadata: Option<metadata::Builder<'_>>,
        activation: Option<metadata::Activation<'_>>,
        mut checkpoint: impl FnMut() -> io::Result<()>,
    ) -> io::Result<ApplyReport> {
        let _owner_guard = FileGuard::open_regular(&self.state.join("owner"), OWNER)?;
        finish::reject_pending(self)?;
        // The owner's verified parent gives the physical state directory name
        // for conflict checks, while all mutation paths remain as requested.
        let state_name = native::destination_name(&self.state.join("owner"))?
            .parent()
            .expect("owner has a parent")
            .to_owned();
        let mut planned = plan(links)?;
        let mut configurations = configurations
            .iter()
            .map(|change| change.record.clone())
            .collect::<Vec<_>>();
        let mut creations = creations.to_vec();
        let metadata_slot = metadata
            .as_ref()
            .map(|builder| builder.reserve(&mut configurations, &mut creations));
        for (index, config) in configurations.iter().enumerate() {
            refuse_escape(&config.path)?;
            config.validate()?;
            if paths_overlap(&native::destination_name(&config.path)?, &state_name)? {
                return Err(invalid(
                    "configuration destination overlaps its recovery state",
                ));
            }
            if configurations[..index]
                .iter()
                .any(|other| config.same_object(other))
            {
                return Err(invalid(
                    "duplicate configuration object identity; no registration changes made",
                ));
            }
            for other in std::iter::once(&self.state)
                .chain(planned.iter().map(|record| &record.path))
                .chain(configurations[..index].iter().map(|record| &record.path))
            {
                if paths_overlap(&config.path, other)? {
                    return Err(invalid(
                        "configuration destination overlaps registration state or another destination",
                    ));
                }
            }
        }
        for record in &planned {
            if paths_overlap(&record.path, &self.state)?
                || paths_overlap(&native::destination_name(&record.path)?, &state_name)?
            {
                return Err(invalid(
                    "registration destination overlaps its recovery state",
                ));
            }
        }
        let mut names = Vec::new();
        for path in planned
            .iter()
            .map(|r| &r.path)
            .chain(configurations.iter().map(|r| &r.path))
            .chain(creations.iter().map(|r| &r.path))
            .chain(link_changes.iter().map(|r| &r.path))
        {
            refuse_escape(path)?;
            let name = native::destination_name(path)?;
            if paths_overlap(&name, &state_name)? {
                return Err(invalid(
                    "configuration destination overlaps its recovery state",
                ));
            }
            reject_destination_overlap(&names, &name)?;
            names.push(name);
        }
        match self.load_journal()? {
            Some(_) if metadata.is_some() => {
                return Err(invalid(
                    "metadata preparation requires an empty journal; finish or disconnect it first",
                ));
            }
            Some(existing) if existing.completion.is_some() => {
                same_plan(&existing.journal, &planned)?;
                if existing.journal.configurations.len() != configurations.len()
                    || !existing
                        .journal
                        .configurations
                        .iter()
                        .zip(&configurations)
                        .all(|(old, next)| old.same_candidate(next))
                {
                    return Err(invalid(
                        "A different configuration journal already exists; recover or disconnect it first.",
                    ));
                }
                live_matches(&existing.journal.records)?;
                for record in &existing.journal.configurations {
                    record.check_published()?;
                }
                if existing.journal.creations.len() != creations.len()
                    || !existing
                        .journal
                        .creations
                        .iter()
                        .zip(&creations)
                        .all(|(record, plan)| record.same_candidate(plan))
                {
                    return Err(invalid(
                        "A different creation journal already exists; recover or disconnect it first.",
                    ));
                }
                for record in &existing.journal.creations {
                    record.check_published()?;
                }
                if existing.journal.link_changes.len() != link_changes.len()
                    || !existing
                        .journal
                        .link_changes
                        .iter()
                        .zip(link_changes)
                        .all(|(record, plan)| record.same_candidate(plan))
                {
                    return Err(invalid(
                        "A different link-change journal already exists; recover or disconnect it first.",
                    ));
                }
                for record in &existing.journal.link_changes {
                    record.check_published()?;
                }
                return Ok(report(&existing.journal, false));
            }
            Some(_) => {
                return Err(invalid(
                    "An interrupted registration needs recovery before apply.",
                ));
            }
            None if planned.is_empty()
                && configurations.is_empty()
                && creations.is_empty()
                && link_changes.is_empty()
                && activation.is_none() =>
            {
                return Ok(ApplyReport {
                    links: Vec::new(),
                    configurations: Vec::new(),
                    changed_links: Vec::new(),
                });
            }
            None => {}
        }
        for config in &configurations {
            config.check_before()?;
        }
        for creation in &creations {
            creation.check_absent()?;
        }
        let held_metadata = match metadata_slot {
            Some(metadata::Slot::Configuration(index)) => {
                Some(configurations[index].hold_before()?)
            }
            _ => None,
        };
        let mut held_reused = Vec::new();
        let mut metadata_links = Vec::new();
        let mut previous_links = Vec::new();
        if metadata.is_some() {
            for record in planned.iter_mut().filter(|record| !record.created) {
                let guard = FileGuard::capture_link(&record.path)?;
                let captured = metadata::guarded(&record.path, &guard)?;
                if captured.link_type != record.link_type
                    || !same_target(&captured.target, &record.target)
                {
                    return Err(invalid("reused link changed during metadata preparation"));
                }
                // Persist the actual stored target too. Finish verifies literals,
                // whereas the older public planner permits canonical aliases.
                *record = Record::new(
                    record.path.clone(),
                    record.link_type,
                    captured.target.clone(),
                    false,
                )?;
                record.reused_identity = Some(captured.identity.clone());
                previous_links.push(captured.clone());
                metadata_links.push(captured);
                held_reused.push(guard);
            }
        }
        let held_changes = link_changes
            .iter()
            .map(LinkChange::hold)
            .collect::<io::Result<Vec<_>>>()?;
        let mut change_records = Vec::new();
        let mut change_stages = Vec::new();
        for (index, change) in link_changes.iter().enumerate() {
            let (record, stage) = change.stage(index)?;
            if metadata.is_some() {
                previous_links.push(metadata::guarded(&change.path, &held_changes[index])?);
                if let Some(stage) = &stage {
                    metadata_links.push(metadata::staged(&change.path, stage)?);
                }
            }
            change_records.push(record);
            change_stages.extend(stage);
        }
        // Keep creation uncommitted until its exact object identity is durable.
        // Dropping these guards (including a failed prepare) rolls back staging.
        let mut staged_links = Vec::new();
        for (index, record) in planned.iter_mut().enumerate() {
            if record.created {
                prepare_parent(&record.path)?;
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(io::Error::other)?
                    .as_nanos();
                let path = record
                    .path
                    .parent()
                    .expect("validated destination")
                    .join(format!(
                        ".codex-harness-link-{}-{stamp}-{index}",
                        std::process::id()
                    ));
                let staged = StagedLink::create(
                    &path,
                    &record.target,
                    record.link_type == LinkType::Directory,
                )?;
                record.staged = Some(Staged {
                    path,
                    identity: staged.identity(),
                });
                if metadata.is_some() {
                    metadata_links.push(metadata::staged(&record.path, &staged)?);
                }
                staged_links.push(staged);
            }
        }
        // Parent creation or a namespace change since planning may expose an
        // alias. All new stages remain uncommitted and pin these parent names;
        // a conflict here drops every stage without leaving recovery intent.
        let mut created_configs = Vec::new();
        let mut staged_files = Vec::new();
        for (index, creation) in creations.iter().enumerate() {
            prepare_parent(&creation.path)?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos();
            let path = creation
                .path
                .parent()
                .expect("validated destination")
                .join(format!(
                    ".codex-harness-config-{}-{stamp}-{index}",
                    std::process::id(),
                ));
            let stage = StagedFile::create(&path, creation.bytes())?;
            created_configs.push(CreatedConfig::new(creation, path, stage.identity()));
            staged_files.push(stage);
        }
        if let Some(builder) = metadata {
            let slot = metadata_slot.expect("reserved metadata destination");
            let (path, identity) = match slot {
                metadata::Slot::Creation(index) => (
                    creations[index].path.clone(),
                    staged_files[index].identity(),
                ),
                metadata::Slot::Configuration(index) => (
                    configurations[index].path.clone(),
                    held_metadata
                        .as_ref()
                        .expect("held metadata snapshot")
                        .object_identity()?,
                ),
            };
            let previous_metadata_sha256 = builder.baseline_sha256();
            let bytes = builder.build(&metadata::MetadataView {
                links: metadata_links,
                previous: previous_links,
                path,
                identity,
                previous_metadata_sha256,
            })?;
            match slot {
                metadata::Slot::Creation(index) => {
                    let plan = ConfigCreation::new(&creations[index].path, &bytes)?;
                    staged_files[index].set_bytes(&bytes)?;
                    created_configs[index] = CreatedConfig::new(
                        &plan,
                        created_configs[index].stage.clone(),
                        staged_files[index].identity(),
                    );
                    creations[index] = plan;
                }
                metadata::Slot::Configuration(index) => {
                    configurations[index].set_candidate(bytes)?;
                }
            }
        }
        let mut names = validate_staged_destinations(&planned, &staged_links, &state_name)?;
        for config in &configurations {
            let name = native::destination_name(&config.path)?;
            reject_destination_overlap(&names, &name)?;
            if paths_overlap(&name, &state_name)? {
                return Err(invalid(
                    "configuration destination overlaps its recovery state",
                ));
            }
            names.push(name);
        }
        for (creation, stage) in creations.iter().zip(&staged_files) {
            let name = stage
                .destination_name(creation.path.file_name().expect("validated destination"))?;
            reject_destination_overlap(&names, &name)?;
            if paths_overlap(&name, &state_name)? {
                return Err(invalid(
                    "configuration destination overlaps its recovery state",
                ));
            }
            names.push(name);
        }
        for change in link_changes {
            let name = native::destination_name(&change.path)?;
            reject_destination_overlap(&names, &name)?;
            if paths_overlap(&name, &state_name)? {
                return Err(invalid("changed link destination overlaps recovery state"));
            }
            names.push(name);
        }
        if metadata_slot.is_some() {
            // The builder returns bytes only. Recheck unguarded destinations
            // after it returns; a foreign arrival must not create durable intent.
            for record in planned.iter().filter(|record| record.created) {
                if !matches!(inspect(&record.path)?, Presence::Missing) {
                    return Err(invalid(
                        "new link destination changed during metadata preparation",
                    ));
                }
            }
            for creation in &creations {
                creation.check_absent()?;
            }
            for (index, config) in configurations.iter().enumerate() {
                if !matches!(metadata_slot, Some(metadata::Slot::Configuration(held)) if held == index)
                {
                    config.check_before()?;
                }
            }
        }
        let mut journal = Journal::new(planned, configurations, created_configs, change_records)?;
        journal.path_change = activation.as_ref().and_then(|a| a.path_change.clone());
        journal.retire_configuration = activation
            .as_ref()
            .and_then(|a| a.retire_configuration.clone());
        journal.process_path = activation.as_ref().and_then(|a| a.process_path.clone());
        journal.checksum = journal.calculate_checksum()?;
        journal.verify()?;
        let bytes = encode(&journal)?;
        write_bytes(&self.journal_path(), &bytes)?;
        let _journal_guard = FileGuard::open_regular(&self.journal_path(), &bytes)?;
        for staged in staged_links {
            staged.commit()?;
        }
        for staged in staged_files {
            staged.commit()?;
        }
        for staged in change_stages {
            staged.commit()?;
        }
        drop(held_changes);
        drop(held_reused);
        drop(held_metadata);
        // This intent is immutable. Completion is a separate exclusively created
        // marker bound to the exact journal, so interruption never truncates it.
        for record in &journal.records {
            if let Some(staged) = &record.staged {
                native::publish_link(
                    &staged.path,
                    &record.path,
                    &record.target,
                    record.link_type == LinkType::Directory,
                    &staged.identity,
                )?;
            }
        }
        for config in &journal.configurations {
            config.publish()?;
            checkpoint()?;
        }
        for config in &journal.creations {
            config.publish()?;
            checkpoint()?;
        }
        for change in &journal.link_changes {
            change.publish(&mut checkpoint)?;
            checkpoint()?;
        }
        if let Some(change) = &journal.path_change {
            crate::environment_path::receipt::PathReceipts::held_registration(
                &self.journal_path(),
                &bytes,
                &_journal_guard,
                change,
            )
            .apply_registration()?;
        }
        if let Some(activation) = activation {
            if let Some(change) = &journal.process_path {
                change.publish()?;
            }
            (activation.verify)()?;
        }
        live_matches(&journal.records)?;
        for config in &journal.configurations {
            config.check_published()?;
        }
        for config in &journal.creations {
            config.check_published()?;
        }
        for change in &journal.link_changes {
            change.check_published()?;
        }
        if let Some(change) = &journal.path_change {
            change.verify_published()?;
        }
        if let Some(change) = &journal.process_path {
            change.verify_published()?;
        }
        let completion = Completion {
            schema: SCHEMA,
            journal_sha256: build_identity::hash_bytes(&bytes),
        };
        write_bytes(
            &self.state.join(COMPLETION),
            &serde_json::to_vec_pretty(&completion)?,
        )?;
        Ok(report(&journal, true))
    }

    pub fn recover(&self) -> io::Result<UndoReport> {
        let _owner_guard = FileGuard::open_regular(&self.state.join("owner"), OWNER)?;
        if let Some(report) = finish::resume(self, &_owner_guard)? {
            return Ok(report);
        }
        match self.load_journal()? {
            None => Ok(UndoReport {
                removed: Vec::new(),
                restored: Vec::new(),
                committed: false,
            }),
            Some(snapshot) if snapshot.completion.is_some() => Err(invalid(
                "No interrupted registration; disconnect owned links explicitly.",
            )),
            Some(snapshot) => self.undo_checked(snapshot, false, &_owner_guard),
        }
    }

    pub fn disconnect(&self) -> io::Result<UndoReport> {
        let _owner_guard = FileGuard::open_regular(&self.state.join("owner"), OWNER)?;
        if let Some(report) = finish::resume(self, &_owner_guard)? {
            return Ok(report);
        }
        match self.load_journal()? {
            None => Ok(UndoReport {
                removed: Vec::new(),
                restored: Vec::new(),
                committed: false,
            }),
            Some(snapshot) if snapshot.completion.is_none() => Err(invalid(
                "An interrupted registration needs recovery before disconnect.",
            )),
            Some(snapshot) => self.undo_checked(snapshot, false, &_owner_guard),
        }
    }

    fn load_journal(&self) -> io::Result<Option<Snapshot>> {
        let bytes = match read_bytes(&self.journal_path(), MAX_JOURNAL) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return match fs::symlink_metadata(self.state.join(COMPLETION)) {
                    Ok(_) => Err(invalid(
                        "Orphan registration completion marker; preserving it.",
                    )),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error),
                };
            }
            other => other?,
        };
        let journal: Journal = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("registration journal ownership changed; preserving it"))?;
        journal.verify()?;
        for record in &journal.configurations {
            if paths_overlap(&record.path, &self.state)? {
                return Err(invalid(
                    "configuration destination overlaps its recovery state",
                ));
            }
        }
        let completion = match read_bytes(&self.state.join(COMPLETION), 4096) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
            Ok(raw) => {
                let marker: Completion = serde_json::from_slice(&raw).map_err(|_| {
                    invalid("registration completion ownership changed; preserving it")
                })?;
                if marker.schema != journal.schema
                    || marker.journal_sha256 != build_identity::hash_bytes(&bytes)
                {
                    return Err(invalid(
                        "registration completion ownership changed; preserving it",
                    ));
                }
                Some(raw)
            }
        };
        Ok(Some(Snapshot {
            journal,
            bytes,
            completion,
        }))
    }

    fn undo_checked(
        &self,
        snapshot: Snapshot,
        preview: bool,
        owner: &FileGuard,
    ) -> io::Result<UndoReport> {
        let journal_guard = FileGuard::open_regular(&self.journal_path(), &snapshot.bytes)?;
        let completion_guard = snapshot
            .completion
            .as_ref()
            .map(|expected| FileGuard::open_regular(&self.state.join(COMPLETION), expected))
            .transpose()?;
        // These objects are kept, never removed. Their recorded ownership must
        // still hold before any undo, and their guards survive all undo cleanup.
        let _held_reused = snapshot
            .journal
            .records
            .iter()
            .filter(|record| record.reused_identity.is_some())
            .map(|record| {
                let guard = native::verified_link(
                    &record.path,
                    &record.target,
                    record.link_type == LinkType::Directory,
                )?;
                if Some(&guard.object_identity()?) != record.reused_identity.as_ref() {
                    return Err(invalid(
                        "reused link identity changed; preserving object and journal",
                    ));
                }
                Ok(guard)
            })
            .collect::<io::Result<Vec<_>>>()?;
        let mut removable = Vec::new();
        for record in &snapshot.journal.records {
            let Some(staged) = &record.staged else {
                continue;
            };
            for path in [&record.path, &staged.path] {
                match inspect(path)? {
                    Presence::Missing => {}
                    Presence::Owned(link_type, _) if link_type == record.link_type => {
                        let identity = native::link_identity(
                            path,
                            &record.target,
                            record.link_type == LinkType::Directory,
                        )?;
                        if identity != staged.identity {
                            return Err(invalid(
                                "destination ownership changed; preserving object and journal",
                            ));
                        }
                        removable.push((record, path));
                    }
                    _ => {
                        return Err(invalid(&format!(
                            "destination ownership changed; preserving destination and journal: {}",
                            path.display()
                        )));
                    }
                }
            }
        }
        for config in &snapshot.journal.configurations {
            config.check_undo()?;
        }
        for config in &snapshot.journal.creations {
            config.check_undo()?;
        }
        for change in &snapshot.journal.link_changes {
            change.check_undo()?;
        }
        if preview {
            if let Some(change) = &snapshot.journal.path_change {
                crate::environment_path::receipt::PathReceipts::held_registration(
                    &self.journal_path(),
                    &snapshot.bytes,
                    &journal_guard,
                    change,
                )
                .observe_undo_registration()?;
            }
            if let Some(change) = &snapshot.journal.process_path {
                change.check_undo()?;
            }
            return Ok(UndoReport {
                removed: Vec::new(),
                restored: Vec::new(),
                committed: false,
            });
        }
        if let Some(change) = &snapshot.journal.path_change {
            crate::environment_path::receipt::PathReceipts::held_registration(
                &self.journal_path(),
                &snapshot.bytes,
                &journal_guard,
                change,
            )
            .undo_registration()?;
        }
        if let Some(change) = &snapshot.journal.process_path {
            change.rollback()?;
        }
        let mut removed_configs = Vec::new();
        for config in snapshot.journal.creations.iter().rev() {
            if config.undo()? {
                removed_configs.push(config.path.clone());
            }
        }
        let mut restored = Vec::new();
        for change in snapshot.journal.link_changes.iter().rev() {
            if change.undo()? {
                restored.push(change.path.clone());
            }
        }
        for config in snapshot.journal.configurations.iter().rev() {
            if config.undo()? {
                restored.push(config.path.clone());
            }
        }
        for (record, path) in &removable {
            native::remove_link(
                path,
                &record.target,
                record.link_type == LinkType::Directory,
                &record.staged.as_ref().expect("verified staging").identity,
            )?;
        }
        let mut cleanup = if let Some(change) = &snapshot.journal.path_change {
            crate::environment_path::receipt::PathReceipts::held_registration(
                &self.journal_path(),
                &snapshot.bytes,
                &journal_guard,
                change,
            )
            .cleanup_undo_registration()?
        } else {
            Vec::new()
        };
        cleanup.extend(completion_guard);
        cleanup.push(journal_guard);
        owner.remove_siblings(cleanup, |_, _| Ok(()))?;
        Ok(UndoReport {
            restored,
            committed: false,
            removed: removable
                .into_iter()
                .filter(|(record, path)| *path == &record.path)
                .map(|(_, path)| path.clone())
                .chain(removed_configs)
                .collect(),
        })
    }
}

#[derive(Debug)]
enum Presence {
    Missing,
    Owned(LinkType, PathBuf),
    Foreign,
}

pub(crate) fn plan(links: &[Link]) -> io::Result<Vec<Record>> {
    let mut seen = BTreeSet::new();
    let mut records: Vec<Record> = Vec::new();
    let mut names = Vec::new();
    for link in links {
        let destination = exact_destination(&link.destination)?;
        if !seen.insert(destination.clone()) {
            return Err(invalid("duplicate registration destination"));
        }
        for previous in &records {
            if paths_overlap(&previous.path, &destination)? {
                return Err(invalid("overlapping registration destinations"));
            }
        }
        let name = native::destination_name(&destination)?;
        reject_destination_overlap(&names, &name)?;
        names.push(name);
        match inspect(&destination)? {
            Presence::Owned(existing_type, target)
                if native::targets_match(&target, &link.source)? =>
            {
                // Reuse the live destination without opening its recorded
                // source. Relocation can leave that name dangling.
                let source = recorded_source(&link.source)?;
                records.push(Record::new(destination, existing_type, source, false)?);
            }
            Presence::Missing => {
                let source = exact_source(&link.source)?;
                let link_type = source_type(&source)?;
                records.push(Record::new(destination, link_type, source, true)?);
            }
            Presence::Owned(existing_type, target) => {
                let source = exact_source(&link.source)?;
                let link_type = source_type(&source)?;
                if existing_type == link_type && same_target(&target, &source) {
                    records.push(Record::new(destination, link_type, source, false)?);
                } else {
                    return Err(invalid(&format!(
                        "Foreign destination exists; preserving it: {}",
                        destination.display()
                    )));
                }
            }
            Presence::Foreign => {
                return Err(invalid(&format!(
                    "Foreign destination exists; preserving it: {}",
                    destination.display()
                )));
            }
        }
    }
    Ok(records)
}

fn reject_destination_overlap(names: &[PathBuf], next: &Path) -> io::Result<()> {
    for previous in names {
        if paths_overlap(previous, next)? {
            return Err(invalid("overlapping registration destinations"));
        }
    }
    Ok(())
}

fn validate_staged_destinations(
    records: &[Record],
    stages: &[StagedLink],
    state_name: &Path,
) -> io::Result<Vec<PathBuf>> {
    let mut stages = stages.iter();
    let mut names = Vec::new();
    for record in records {
        let name = if record.created {
            stages
                .next()
                .expect("one stage per created record")
                .destination_name(record.path.file_name().expect("validated destination"))?
        } else {
            native::destination_name(&record.path)?
        };
        if paths_overlap(&name, state_name)? {
            return Err(invalid(
                "registration destination overlaps its recovery state",
            ));
        }
        reject_destination_overlap(&names, &name)?;
        names.push(name);
    }
    Ok(names)
}

pub(crate) fn paths_overlap(left: &Path, right: &Path) -> io::Result<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
    fn spelling(path: &Path) -> Vec<u16> {
        let normalized: PathBuf = path.components().collect();
        let text: Vec<_> = normalized
            .as_os_str()
            .encode_wide()
            .map(|ch| {
                if ch == u16::from(b'/') {
                    u16::from(b'\\')
                } else {
                    ch
                }
            })
            .collect();
        let mut text = text
            .strip_prefix(&[92, 92, 63, 92])
            .unwrap_or(&text)
            .to_vec();
        while text.len() > 3 && text.last() == Some(&92) {
            text.pop();
        }
        text
    }
    let left = spelling(left);
    let right = spelling(right);
    let (short, long) = if left.len() <= right.len() {
        (&left, &right)
    } else {
        (&right, &left)
    };
    if short.len() != long.len() && short.last() != Some(&92) && long[short.len()] != 92 {
        return Ok(false);
    }
    let length = i32::try_from(short.len()).map_err(io::Error::other)?;
    // Conservatively reject overlaps even in a case-sensitive directory. This
    // uses Windows ordinal casing rather than a different Unicode folding rule.
    let result = unsafe { CompareStringOrdinal(short.as_ptr(), length, long.as_ptr(), length, 1) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(result == CSTR_EQUAL)
}

fn live_matches(records: &[Record]) -> io::Result<()> {
    for record in records {
        match inspect(&record.path)? {
            Presence::Owned(link_type, target)
                if link_type == record.link_type && same_target(&target, &record.target) => {}
            _ => {
                return Err(invalid(&format!(
                    "registered destination changed; preserving it: {}",
                    record.path.display()
                )));
            }
        }
        if let Some(expected) = record.expected_identity()
            && native::link_identity(
                &record.path,
                &record.target,
                record.link_type == LinkType::Directory,
            )? != *expected
        {
            return Err(invalid(
                "registered destination object changed; preserving it",
            ));
        }
    }
    Ok(())
}

fn same_plan(existing: &Journal, planned: &[Record]) -> io::Result<()> {
    if existing.records.len() != planned.len() {
        return Err(invalid(
            "A registration journal already exists; recover or disconnect it first.",
        ));
    }
    for (current, wanted) in existing.records.iter().zip(planned) {
        if current.path != wanted.path
            || current.link_type != wanted.link_type
            || current.target != wanted.target
        {
            return Err(invalid(
                "A registration journal already exists; recover or disconnect it first.",
            ));
        }
    }
    Ok(())
}

fn report(journal: &Journal, creating: bool) -> ApplyReport {
    ApplyReport {
        changed_links: journal
            .link_changes
            .iter()
            .map(|record| record.path.clone())
            .collect(),
        configurations: journal
            .configurations
            .iter()
            .map(|record| record.path.clone())
            .chain(journal.creations.iter().map(|record| record.path.clone()))
            .collect(),
        links: journal
            .records
            .iter()
            .map(|record| Applied {
                destination: record.path.clone(),
                outcome: if creating && record.created {
                    Outcome::Created
                } else {
                    Outcome::Reused
                },
            })
            .collect(),
    }
}

fn inspect(path: &Path) -> io::Result<Presence> {
    inventory::ordinary_parents(path)?;
    let meta = match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Presence::Missing),
        Err(e) => return Err(e),
        Ok(meta) => meta,
    };
    let ft = meta.file_type();
    if !ft.is_symlink() {
        return Ok(Presence::Foreign);
    }
    let raw = fs::read_link(path)?;
    if !raw.is_absolute()
        || raw.components().any(|c| {
            !matches!(
                c,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            )
        })
    {
        return Ok(Presence::Foreign);
    }
    let Some(link_type) = symlink_kind(ft) else {
        return Ok(Presence::Foreign);
    };
    Ok(Presence::Owned(link_type, raw))
}

fn same_target(live: &Path, expected: &Path) -> bool {
    if live == expected {
        return true;
    }
    match (live.canonicalize(), expected.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn source_type(source: &Path) -> io::Result<LinkType> {
    build_identity::ordinary(source)?;
    let meta = fs::symlink_metadata(source)?;
    if meta.is_dir() {
        Ok(LinkType::Directory)
    } else if meta.is_file() {
        Ok(LinkType::File)
    } else {
        Err(invalid(
            "registration source must be an ordinary file or directory",
        ))
    }
}

fn recorded_source(path: &Path) -> io::Result<PathBuf> {
    refuse_escape(path)?;
    inventory::ordinary_parents(path)?;
    std::path::absolute(path)
}

fn exact_source(path: &Path) -> io::Result<PathBuf> {
    let path = recorded_source(path)?;
    build_identity::ordinary(&path)?;
    Ok(path)
}

fn exact_destination(path: &Path) -> io::Result<PathBuf> {
    refuse_escape(path)?;
    inventory::ordinary_parents(path)?;
    Ok(path.to_owned())
}

fn refuse_escape(path: &Path) -> io::Result<()> {
    if !path.is_absolute()
        || path.components().any(|c| {
            !matches!(
                c,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            )
        })
    {
        return Err(invalid(
            "registration paths must be absolute without parent traversal",
        ));
    }
    Ok(())
}

fn prepare_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        inventory::ordinary_parents(path)?;
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
        build_identity::ordinary(parent)?;
        if !parent.is_dir() {
            return Err(invalid(
                "registration destination parent is not a directory",
            ));
        }
    }
    Ok(())
}

fn symlink_kind(ft: fs::FileType) -> Option<LinkType> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if ft.is_symlink_dir() {
            return Some(LinkType::Directory);
        }
        if ft.is_symlink_file() {
            return Some(LinkType::File);
        }
        None
    }
    #[cfg(not(windows))]
    {
        if ft.is_symlink() {
            Some(LinkType::File)
        } else {
            None
        }
    }
}

fn resolve_state(path: &Path) -> io::Result<PathBuf> {
    refuse_escape(path)?;
    inventory::ordinary_parents(path)?;
    let absolute = std::path::absolute(path)?;
    refuse_escape(&absolute)?;
    // Keep the requested namespace. Resolving an ancestor after checking it
    // could silently bind this operation to another installation after a
    // junction swap. Native guards validate this lexical path before mutations.
    Ok(absolute)
}

fn ensure_owner(state: &Path) -> io::Result<()> {
    inventory::ordinary_parents(state)?;
    let marker = state.join("owner");
    if state.exists() {
        build_identity::ordinary(state)?;
        if !state.is_dir() {
            return Err(invalid(
                "Native registration state is not a directory; preserving it.",
            ));
        }
        if marker.exists() {
            build_identity::ordinary(&marker)?;
            if read_bytes(&marker, OWNER.len() as u64)? != OWNER {
                return Err(invalid(
                    "Native registration state has foreign ownership; preserving it.",
                ));
            }
            return Ok(());
        }
        if fs::read_dir(state)?.next().is_some() {
            return Err(invalid(
                "Native registration state has no ownership record and is nonempty; preserving it.",
            ));
        }
    } else {
        fs::create_dir_all(state)?;
        build_identity::ordinary(state)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)?;
    file.write_all(OWNER)?;
    file.sync_all()
}

fn encode(journal: &Journal) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(journal).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_JOURNAL {
        return Err(invalid("registration intent exceeds its recovery bound"));
    }
    Ok(bytes)
}

fn read_bytes(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    inventory::ordinary_parents(path)?;
    build_identity::ordinary(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid(
            "registration metadata exceeds its bound; preserving it",
        ));
    }
    Ok(bytes)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    let dir = path
        .parent()
        .ok_or_else(|| invalid("registration journal path has no parent"))?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    let file = temp.persist_noclobber(path).map_err(|e| e.error)?;
    file.sync_all()
}

fn checksum(link_type: &LinkType, path: &Path, target: &Path, created: bool) -> io::Result<String> {
    let mut data = String::new();
    data.push_str(match link_type {
        LinkType::File => "file",
        LinkType::Directory => "directory",
    });
    data.push('\n');
    data.push_str(path_text(path)?);
    data.push('\n');
    data.push_str(path_text(target)?);
    data.push_str(if created {
        "\ncreated"
    } else {
        "\npreexisting"
    });
    Ok(build_identity::hash_bytes(data.as_bytes()))
}

fn path_text(path: &Path) -> io::Result<&str> {
    path.to_str()
        .ok_or_else(|| invalid("registration path must be valid unicode"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

#[cfg(test)]
mod destination_tests {
    use super::*;

    #[test]
    fn conflict_at_staged_validation_rolls_back_every_uncommitted_link() {
        let root = tempfile::Builder::new()
            .prefix("harness-staged-destination-conflict-")
            .tempdir()
            .unwrap()
            .keep();
        println!("staged destination evidence: {}", root.display());
        let source = root.join("source");
        fs::write(&source, b"preserve").unwrap();
        let destination = root.join("missing");
        let stages = [root.join("stage-a"), root.join("stage-b")];
        let records = [
            Record::new(destination.clone(), LinkType::File, source.clone(), true).unwrap(),
            Record::new(destination.clone(), LinkType::File, source.clone(), true).unwrap(),
        ];
        // Exercise the final defense directly, after distinct objects have
        // staged but before any intent or stage commit can authorize recovery.
        let guards: Vec<_> = stages
            .iter()
            .map(|path| StagedLink::create(path, &source, false).unwrap())
            .collect();
        assert_ne!(guards[0].identity(), guards[1].identity());
        let state_name = native::destination_name(&root.join("state")).unwrap();
        let error = validate_staged_destinations(&records, &guards, &state_name).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("overlapping registration destinations")
        );
        drop(guards);
        assert!(!destination.exists());
        assert!(
            stages
                .iter()
                .all(|path| fs::symlink_metadata(path).is_err())
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(fs::read(source).unwrap(), b"preserve");
    }

    #[test]
    fn plan_reuses_a_dangling_recorded_source_without_opening_it() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("link");
        let missing = root.path().join("old-source.txt");
        std::os::windows::fs::symlink_file(&missing, &destination).unwrap();
        let planned = plan(&[Link {
            kind: "profile".into(),
            name: "harness".into(),
            source: missing.clone(),
            destination: destination.clone(),
            connection: crate::inventory::Connection::Linked,
        }])
        .unwrap();
        assert_eq!(planned.len(), 1);
        assert!(!planned[0].created);
        assert_eq!(planned[0].target, std::path::absolute(&missing).unwrap());
        assert_eq!(fs::read_link(&destination).unwrap(), missing);
        assert!(!missing.exists());
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    use crate::config_file::ConfigSnapshot;
    use crate::inventory::Connection;

    fn fixture() -> PathBuf {
        let root = tempfile::Builder::new()
            .prefix("harness-config-journal-")
            .tempdir()
            .unwrap()
            .keep();
        for name in ["a.toml", "b.toml", "source.txt"] {
            fs::write(root.join(name), b"original").unwrap();
        }
        println!("configuration journal evidence: {}", root.display());
        root
    }

    fn inputs(root: &Path) -> (Link, Vec<ConfigChange>) {
        let link = Link {
            kind: "file".into(),
            name: "instructions".into(),
            source: root.join("source.txt"),
            destination: root.join("AGENTS.md"),
            connection: Connection::Missing,
        };
        let changes = ["a.toml", "b.toml"]
            .iter()
            .map(|name| {
                ConfigSnapshot::read(&root.join(name))
                    .unwrap()
                    .plan_replace(b"candidate")
                    .unwrap()
            })
            .collect();
        (link, changes)
    }

    #[test]
    fn duplicate_object_identity_is_rejected_in_plans_and_validly_checksummed_intent() {
        let root = fixture();
        let (link, changes) = inputs(&root);
        let first = changes[0].record.clone();
        let mut alias = first.clone();
        alias.path = root.join("different-spelling.toml");
        let reg = Registration::open(&root.join("state")).unwrap();
        let plans = [
            ConfigChange {
                record: first.clone(),
            },
            ConfigChange {
                record: alias.clone(),
            },
        ];
        let error = reg.apply_with_configs(&[link], &plans).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("duplicate configuration object identity")
        );
        assert!(!root.join("AGENTS.md").exists());
        assert!(!reg.journal_path().exists());
        let journal = Journal::new(vec![], vec![first, alias], vec![], vec![]).unwrap();
        let bytes = encode(&journal).unwrap();
        write_bytes(&reg.journal_path(), &bytes).unwrap();
        let error = reg.recover().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("duplicate configuration object identity")
        );
        assert_eq!(fs::read(reg.journal_path()).unwrap(), bytes);
        assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"original");
    }

    #[test]
    fn partial_configuration_apply_and_foreign_conflict_remain_recoverable() {
        for foreign_change in [false, true] {
            let root = fixture();
            let (link, changes) = inputs(&root);
            let reg = Registration::open(&root.join("state")).unwrap();
            let result = reg.apply_inner(&[link], &changes, &[], &[], || {
                if foreign_change {
                    fs::write(root.join("b.toml"), b"foreign").unwrap();
                    Ok(())
                } else {
                    Err(io::Error::other(
                        "injected failure after first configuration",
                    ))
                }
            });
            assert!(result.is_err());
            assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"candidate");
            assert!(root.join("AGENTS.md").is_symlink());
            assert!(!reg.state.join(COMPLETION).exists());
            if foreign_change {
                let journal = fs::read(reg.journal_path()).unwrap();
                assert!(reg.recover().is_err());
                assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"candidate");
                assert_eq!(fs::read(root.join("b.toml")).unwrap(), b"foreign");
                assert_eq!(fs::read(reg.journal_path()).unwrap(), journal);
                assert!(root.join("AGENTS.md").is_symlink());
                // Explicit resolution on this owned fixture only.
                fs::write(root.join("b.toml"), b"original").unwrap();
            }
            let undo = reg.recover().unwrap();
            assert_eq!(undo.restored, vec![root.join("a.toml")]);
            assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"original");
            assert_eq!(fs::read(root.join("b.toml")).unwrap(), b"original");
            assert!(!root.join("AGENTS.md").exists());
            assert!(!reg.journal_path().exists());
            assert!(reg.recover().unwrap().restored.is_empty());
        }
    }

    #[test]
    fn partial_new_configuration_and_foreign_conflict_preserve_recovery() {
        for foreign in [false, true] {
            let root = fixture();
            let (link, changes) = inputs(&root);
            let creations = ["new-a.toml", "new-b.toml"].map(|name| {
                ConfigCreation::new(&root.join(name), b"PRIVATE_CREATION_SENTINEL").unwrap()
            });
            let reg = Registration::open(&root.join("state")).unwrap();
            let mut count = 0;
            let result = reg.apply_inner(&[link], &changes[..1], &creations, &[], || {
                count += 1;
                if count != 2 {
                    return Ok(());
                }
                if foreign {
                    fs::write(root.join("new-b.toml"), b"foreign").unwrap();
                    Ok(())
                } else {
                    Err(io::Error::other("injected failure after first creation"))
                }
            });
            assert!(result.is_err());
            assert_eq!(
                fs::read(root.join("new-a.toml")).unwrap(),
                b"PRIVATE_CREATION_SENTINEL"
            );
            assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"candidate");
            let snapshot = reg.load_journal().unwrap().unwrap();
            assert!(!format!("{:?}", snapshot.journal).contains("PRIVATE_CREATION_SENTINEL"));
            if foreign {
                assert!(reg.recover().is_err());
                assert_eq!(fs::read(reg.journal_path()).unwrap(), snapshot.bytes);
                assert_eq!(fs::read(root.join("new-b.toml")).unwrap(), b"foreign");
                assert!(root.join("AGENTS.md").is_symlink());
                assert!(root.join("new-a.toml").exists());
                assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"candidate");
                fs::remove_file(root.join("new-b.toml")).unwrap();
            }
            reg.recover().unwrap();
            assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"original");
            for name in ["new-a.toml", "new-b.toml", "AGENTS.md"] {
                assert!(!root.join(name).exists());
            }
            for record in snapshot.journal.creations {
                assert!(!record.stage.exists());
            }
            assert!(!reg.journal_path().exists());
        }
    }

    #[test]
    #[ignore = "owned child of interrupted_configuration_process_recovers_exact_originals"]
    fn configuration_process_fixture() {
        let root = PathBuf::from(std::env::var_os("HARNESS_CONFIG_JOURNAL_ROOT").unwrap());
        let (link, changes) = inputs(&root);
        let reg = Registration::open(&root.join("state")).unwrap();
        let create = std::env::var_os("HARNESS_CONFIG_JOURNAL_CREATE").is_some();
        let creations = if create {
            ["new-a.toml", "new-b.toml"]
                .map(|name| ConfigCreation::new(&root.join(name), b"new").unwrap())
                .into_iter()
                .collect()
        } else {
            vec![]
        };
        let mut count = 0;
        reg.apply_inner(&[link], &changes, &creations, &[], || {
            count += 1;
            if create && count < 3 {
                return Ok(());
            }
            fs::write(root.join("published-first"), b"ready").unwrap();
            std::thread::sleep(std::time::Duration::from_secs(20));
            std::process::exit(79);
        })
        .unwrap();
    }

    #[test]
    fn interrupted_configuration_process_recovers_exact_originals() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        for create in [false, true] {
            let root = fixture();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--ignored",
                    "--exact",
                    "registration::configuration_tests::configuration_process_fixture",
                    "--nocapture",
                ])
                .env("HARNESS_CONFIG_JOURNAL_ROOT", &root)
                .env_remove("HARNESS_CONFIG_JOURNAL_CREATE")
                .stdin(Stdio::null())
                .stdout(fs::File::create(root.join("child.stdout")).unwrap())
                .stderr(fs::File::create(root.join("child.stderr")).unwrap());
            if create {
                command.env("HARNESS_CONFIG_JOURNAL_CREATE", "1");
            }
            let mut child = command.spawn().unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while !root.join("published-first").exists() && Instant::now() < until {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let observed = root.join("published-first").exists();
            // This exact Rust fixture creates no descendants; never search by PID
            // or executable name, and always reap a failed or timed-out setup too.
            let _ = child.kill();
            let exit = child.wait().unwrap();
            assert!(observed, "publication not observed: {}", root.display());
            assert!(!exit.success());
            assert_ne!(exit.code(), Some(79));
            let reg = Registration::open(&root.join("state")).unwrap();
            assert!(!reg.state.join(COMPLETION).exists());
            assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"candidate");
            assert_eq!(
                fs::read(root.join("b.toml")).unwrap(),
                if create {
                    &b"candidate"[..]
                } else {
                    &b"original"[..]
                }
            );
            let snapshot = reg.load_journal().unwrap().unwrap();
            if create {
                assert_eq!(fs::read(root.join("new-a.toml")).unwrap(), b"new");
                assert!(!root.join("new-b.toml").exists());
            }
            let undo = reg.recover().unwrap();
            assert_eq!(
                undo.restored,
                if create {
                    vec![root.join("b.toml"), root.join("a.toml")]
                } else {
                    vec![root.join("a.toml")]
                }
            );
            assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"original");
            assert_eq!(fs::read(root.join("b.toml")).unwrap(), b"original");
            assert!(!root.join("new-a.toml").exists());
            assert!(!root.join("new-b.toml").exists());
            for record in snapshot.journal.creations {
                assert!(!record.stage.exists());
            }
            assert!(!root.join("AGENTS.md").exists());
            assert!(!reg.journal_path().exists());
        }
    }
}
