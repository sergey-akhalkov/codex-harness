//! One temporary, self-contained decision embedding the schema-8 undo intent.
//! Schema 8 also binds reused link identities across the irreversible decision.
//! Publication is irreversible. Its last deletion retires all recovery state.

use super::*;

pub const COMMIT: &str = "commit.json";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
enum MetadataRecord {
    Configuration(usize),
    Creation(usize),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataWitness {
    record: MetadataRecord,
    path: PathBuf,
    identity: LinkIdentity,
    sha256: String,
}

impl MetadataWitness {
    fn new(journal: &Journal, path: &Path) -> io::Result<Self> {
        let mut matches = Vec::new();
        for (index, config) in journal.configurations.iter().enumerate() {
            if config.path == path {
                matches.push(Self {
                    record: MetadataRecord::Configuration(index),
                    path: config.path.clone(),
                    identity: config.published_identity().clone(),
                    sha256: build_identity::hash_bytes(config.published_bytes()),
                });
            }
        }
        for (index, config) in journal.creations.iter().enumerate() {
            if config.path == path {
                matches.push(Self {
                    record: MetadataRecord::Creation(index),
                    path: config.path.clone(),
                    identity: config.published_identity().clone(),
                    sha256: build_identity::hash_bytes(config.published_bytes()),
                });
            }
        }
        if matches.len() != 1 {
            return Err(invalid(
                "finish metadata must nominate exactly one journaled configuration",
            ));
        }
        Ok(matches.remove(0))
    }
}

// Deliberately no Debug: the embedded intent contains private configuration.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Commitment {
    schema: u32,
    state: PathBuf,
    identity: LinkIdentity,
    journal: String,
    journal_sha256: String,
    journal_identity: LinkIdentity,
    completion: String,
    completion_identity: LinkIdentity,
    metadata: MetadataWitness,
    checksum: String,
}

impl Commitment {
    fn checksum(&self) -> io::Result<String> {
        Ok(build_identity::hash_bytes(&serde_json::to_vec(&(
            self.schema,
            &self.state,
            &self.identity,
            &self.journal,
            &self.journal_sha256,
            &self.journal_identity,
            &self.completion,
            &self.completion_identity,
            &self.metadata,
        ))?))
    }

    fn verify(&self) -> io::Result<Journal> {
        if self.schema != SCHEMA
            || self.journal.len() as u64 > MAX_JOURNAL
            || self.completion.len() > 4096
            || self.checksum != self.checksum()?
            || self.journal_sha256 != build_identity::hash_bytes(self.journal.as_bytes())
        {
            return Err(conflict());
        }
        let journal: Journal = serde_json::from_str(&self.journal).map_err(|_| conflict())?;
        journal.verify()?;
        let completion: Completion =
            serde_json::from_str(&self.completion).map_err(|_| conflict())?;
        if completion.schema != SCHEMA
            || completion.journal_sha256 != self.journal_sha256
            || self.metadata != MetadataWitness::new(&journal, &self.metadata.path)?
        {
            return Err(conflict());
        }
        Ok(journal)
    }
}

struct FinishGuards {
    // Retain every candidate, not only the nominated metadata, until all
    // cleanup commits. Each guard protects an exact object and its namespace.
    _published: Vec<FileGuard>,
    backups: Vec<FileGuard>,
    journal: Option<FileGuard>,
    completion: Option<FileGuard>,
}

pub(super) fn require_absent(path: &Path) -> io::Result<()> {
    match inspect(path)? {
        Presence::Missing => Ok(()),
        _ => Err(conflict()),
    }
}

fn optional_regular(
    path: &Path,
    expected: &[u8],
    identity: &LinkIdentity,
    committed: bool,
) -> io::Result<Option<FileGuard>> {
    match FileGuard::open_regular(path, expected) {
        Ok(guard) => {
            if &guard.object_identity()? != identity {
                return Err(conflict());
            }
            Ok(Some(guard))
        }
        Err(error) if committed && error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn hold_candidates(
    reg: &Registration,
    journal: &Journal,
    committed: bool,
) -> io::Result<(Vec<FileGuard>, Vec<FileGuard>)> {
    let mut published = Vec::new();
    let mut backups = Vec::new();
    let mut paths = Vec::new();
    for record in &journal.records {
        let guard = native::verified_link(
            &record.path,
            &record.target,
            record.link_type == LinkType::Directory,
        )?;
        paths.push(record.path.as_path());
        if let Some(expected) = record.expected_identity()
            && guard.object_identity()? != *expected
        {
            return Err(conflict());
        }
        if let Some(stage) = &record.staged {
            require_absent(&stage.path)?;
            paths.push(&stage.path);
        }
        published.push(guard);
    }
    for config in &journal.configurations {
        published.push(config.hold_published()?);
        paths.push(&config.path);
    }
    for config in &journal.creations {
        published.push(config.hold_published()?);
        require_absent(&config.stage)?;
        paths.extend([config.path.as_path(), config.stage.as_path()]);
    }
    for change in &journal.link_changes {
        let (live, backup) = change.hold_finished(committed)?;
        published.extend(live);
        backups.extend(backup);
        paths.extend([change.path.as_path(), change.backup_path()]);
        paths.extend(change.stage_path());
    }
    // Reject aliases and overlapping cleanup/candidate names, including those
    // introduced in an altered intent. These names never authorize mutations.
    let state_name = native::destination_name(&reg.state.join("owner"))?
        .parent()
        .expect("owner has a parent")
        .to_owned();
    let mut names = Vec::new();
    for path in paths {
        let name = native::destination_name(path)?;
        if paths_overlap(&name, &state_name)? {
            return Err(conflict());
        }
        reject_destination_overlap(&names, &name)?;
        names.push(name);
    }
    Ok((published, backups))
}

fn load_commit(reg: &Registration) -> io::Result<Option<(Commitment, Journal, FileGuard)>> {
    let (guard, bytes) = match FileGuard::read_regular(&reg.state.join(COMMIT)) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    // read_regular enforces the native 16-MiB outer bound before allocation;
    // verify separately enforces the 4-MiB bound on the embedded schema-8 intent.
    let commitment: Commitment = serde_json::from_slice(&bytes).map_err(|_| conflict())?;
    if commitment.state != reg.state || guard.object_identity()? != commitment.identity {
        return Err(conflict());
    }
    let journal = commitment.verify()?;
    Ok(Some((commitment, journal, guard)))
}

pub(super) fn reject_pending(reg: &Registration) -> io::Result<()> {
    if load_commit(reg)?.is_some() {
        return Err(invalid(
            "committed registration needs cleanup before another apply",
        ));
    }
    Ok(())
}

fn preflight_committed(
    reg: &Registration,
    commitment: &Commitment,
    journal: &Journal,
) -> io::Result<FinishGuards> {
    let original = optional_regular(
        &reg.journal_path(),
        commitment.journal.as_bytes(),
        &commitment.journal_identity,
        true,
    )?;
    let completion = optional_regular(
        &reg.state.join(COMPLETION),
        commitment.completion.as_bytes(),
        &commitment.completion_identity,
        true,
    )?;
    let (published, backups) = hold_candidates(reg, journal, true)?;
    Ok(FinishGuards {
        _published: published,
        backups,
        journal: original,
        completion,
    })
}

fn clean(
    guards: FinishGuards,
    commit: FileGuard,
    checkpoint: &mut impl FnMut(&'static str, usize) -> io::Result<()>,
) -> io::Result<UndoReport> {
    for (index, backup) in guards.backups.into_iter().enumerate() {
        backup.remove()?;
        checkpoint("backup", index)?;
    }
    if let Some(completion) = guards.completion {
        completion.remove()?;
        checkpoint("completion", 0)?;
    }
    if let Some(journal) = guards.journal {
        journal.remove()?;
        checkpoint("journal", 0)?;
    }
    commit.remove()?;
    checkpoint("retired", 0)?;
    Ok(UndoReport {
        removed: Vec::new(),
        restored: Vec::new(),
        committed: true,
    })
}

pub(super) fn resume(reg: &Registration) -> io::Result<Option<UndoReport>> {
    let Some((commitment, journal, commit)) = load_commit(reg)? else {
        return Ok(None);
    };
    let guards = preflight_committed(reg, &commitment, &journal)?;
    clean(guards, commit, &mut |_, _| Ok(())).map(Some)
}

impl Registration {
    /// Irreversibly accept a completed operation and retire its rollback data.
    /// `metadata` must exactly nominate a configuration record in that intent;
    /// the installer is responsible for the metadata's installation semantics
    /// and for holding InstallationLock. No extra marker path grants authority.
    /// Once commit.json is published, errors require cleanup, never rollback.
    /// An empty state is an idempotent no-op (committed=false, no undo performed).
    pub fn finish(&self, metadata: &Path) -> io::Result<UndoReport> {
        self.finish_inner(metadata, &mut |_, _| Ok(()))
    }

    fn finish_inner(
        &self,
        metadata: &Path,
        checkpoint: &mut impl FnMut(&'static str, usize) -> io::Result<()>,
    ) -> io::Result<UndoReport> {
        let _owner = FileGuard::open_regular(&self.state.join("owner"), OWNER)?;
        if let Some((commitment, journal, commit)) = load_commit(self)? {
            if commitment.metadata.path != metadata {
                return Err(conflict());
            }
            let guards = preflight_committed(self, &commitment, &journal)?;
            return clean(guards, commit, checkpoint);
        }
        let Some(snapshot) = self.load_journal()? else {
            return Ok(UndoReport {
                removed: Vec::new(),
                restored: Vec::new(),
                committed: false,
            });
        };
        let completion = snapshot
            .completion
            .ok_or_else(|| invalid("finish requires completed publication"))?;
        let witness = MetadataWitness::new(&snapshot.journal, metadata)?;
        let original = FileGuard::open_regular(&self.journal_path(), &snapshot.bytes)?;
        let completed = FileGuard::open_regular(&self.state.join(COMPLETION), &completion)?;
        let (published, backups) = hold_candidates(self, &snapshot.journal, false)?;
        let mut staged_commit = StagedFile::create(&self.state.join(COMMIT), &[])?;
        let mut commitment = Commitment {
            schema: SCHEMA,
            state: self.state.clone(),
            identity: staged_commit.identity(),
            journal_sha256: build_identity::hash_bytes(&snapshot.bytes),
            journal: String::from_utf8(snapshot.bytes).map_err(|_| conflict())?,
            journal_identity: original.object_identity()?,
            completion: String::from_utf8(completion).map_err(|_| conflict())?,
            completion_identity: completed.object_identity()?,
            metadata: witness,
            checksum: String::new(),
        };
        commitment.checksum = commitment.checksum()?;
        commitment.verify()?;
        let bytes = serde_json::to_vec(&commitment).map_err(|_| conflict())?;
        if bytes.len() > native::MAX_REGULAR_BYTES {
            return Err(invalid("commitment exceeds its recovery bound"));
        }
        staged_commit.set_bytes(&bytes)?;
        let guards = FinishGuards {
            _published: published,
            backups,
            journal: Some(original),
            completion: Some(completed),
        };
        checkpoint("before", 0)?;
        // Any error here may follow publication. Never infer permission to undo
        // from the error: every entry point checks this decision before intent.
        staged_commit.commit()?;
        checkpoint("decision", 0)?;
        let commit = FileGuard::open_regular(&self.state.join(COMMIT), &bytes)?;
        if commit.object_identity()? != commitment.identity {
            return Err(conflict());
        }
        checkpoint("committed", 0)?;
        clean(guards, commit, checkpoint)
    }
}

fn conflict() -> io::Error {
    invalid("registration commitment ownership conflict; preserving files and recovery state")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::ConfigSnapshot;
    use std::os::windows::fs::{symlink_dir, symlink_file};

    fn fixture() -> (PathBuf, Registration) {
        let root = tempfile::Builder::new()
            .prefix("harness-finish-recovery-")
            .tempdir()
            .unwrap()
            .keep();
        println!("finish recovery evidence: {}", root.display());
        fs::write(root.join("old"), b"old source").unwrap();
        fs::write(root.join("new"), b"new source").unwrap();
        fs::create_dir(root.join("old-dir")).unwrap();
        fs::write(root.join("old-dir/keep"), b"preserve directory").unwrap();
        symlink_file(root.join("old"), root.join("replace")).unwrap();
        symlink_dir(root.join("old-dir"), root.join("remove")).unwrap();
        fs::write(root.join("config"), b"before").unwrap();
        let reg = Registration::open(&root.join("state")).unwrap();
        reg.apply_with_changes(
            &[Link {
                kind: "instructions".into(),
                name: "fresh".into(),
                source: root.join("new"),
                destination: root.join("fresh"),
                connection: inventory::Connection::Missing,
            }],
            &[ConfigSnapshot::read(&root.join("config"))
                .unwrap()
                .plan_replace(b"after")
                .unwrap()],
            &[ConfigCreation::new(&root.join("metadata"), b"PRIVATE_FINISH_SENTINEL").unwrap()],
            &[
                LinkChange::replace(&root.join("replace"), &root.join("old"), &root.join("new"))
                    .unwrap(),
                LinkChange::remove(&root.join("remove"), &root.join("old-dir")).unwrap(),
            ],
        )
        .unwrap();
        (root, reg)
    }

    fn candidates(root: &Path) {
        assert_eq!(fs::read(root.join("config")).unwrap(), b"after");
        assert_eq!(
            fs::read(root.join("metadata")).unwrap(),
            b"PRIVATE_FINISH_SENTINEL"
        );
        assert_eq!(
            fs::read_link(root.join("replace")).unwrap(),
            root.join("new")
        );
        assert_eq!(fs::read_link(root.join("fresh")).unwrap(), root.join("new"));
        assert!(fs::symlink_metadata(root.join("remove")).is_err());
        assert_eq!(fs::read(root.join("old")).unwrap(), b"old source");
        assert_eq!(fs::read(root.join("new")).unwrap(), b"new source");
        assert_eq!(
            fs::read(root.join("old-dir/keep")).unwrap(),
            b"preserve directory"
        );
    }

    fn commit_only(root: &Path, reg: &Registration) {
        let result = reg.finish_inner(&root.join("metadata"), &mut |phase, _| {
            if phase == "committed" {
                Err(io::Error::other("owned interruption"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert!(reg.state.join(COMMIT).exists());
    }

    // This explicit mutation oracle needs an existing account privilege beyond
    // Developer Mode's unprivileged CreateSymbolicLink flag. It enables it only in
    // the short-lived Rust test process, never in an account or global policy.
    fn retarget_backup(path: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
        use std::os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        };
        use windows_sys::Win32::{
            Foundation::{GetLastError, INVALID_HANDLE_VALUE, SetLastError},
            Security::{
                AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
                SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
            },
            Storage::FileSystem::{
                CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
                OPEN_EXISTING,
            },
            System::{
                IO::DeviceIoControl,
                Threading::{GetCurrentProcess, OpenProcessToken},
            },
        };
        let mut token = std::ptr::null_mut();
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let privilege: Vec<u16> = "SeCreateSymbolicLinkPrivilege"
            .encode_utf16()
            .chain([0])
            .collect();
        let mut entry = LUID_AND_ATTRIBUTES::default();
        if unsafe { LookupPrivilegeValueW(std::ptr::null(), privilege.as_ptr(), &mut entry.Luid) }
            == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        entry.Attributes = SE_PRIVILEGE_ENABLED;
        let state = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [entry],
        };
        unsafe {
            SetLastError(0);
        }
        if unsafe {
            AdjustTokenPrivileges(
                token.as_raw_handle(),
                0,
                &state,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let error = unsafe { GetLastError() };
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error as i32));
        }
        let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let file = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_WRITE_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if file == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let file = unsafe { OwnedHandle::from_raw_handle(file) };
        let substitute: Vec<u16> = "\\??\\"
            .encode_utf16()
            .chain(target.as_os_str().encode_wide())
            .collect();
        let print: Vec<u16> = target.as_os_str().encode_wide().collect();
        let length = u16::try_from(12 + (substitute.len() + print.len() + 2) * 2)
            .map_err(std::io::Error::other)?;
        let mut data = Vec::new();
        data.extend(0xa000000cu32.to_le_bytes());
        data.extend(length.to_le_bytes());
        data.extend(0u16.to_le_bytes());
        data.extend(0u16.to_le_bytes());
        data.extend(((substitute.len() * 2) as u16).to_le_bytes());
        data.extend((((substitute.len() + 1) * 2) as u16).to_le_bytes());
        data.extend(((print.len() * 2) as u16).to_le_bytes());
        data.extend(0u32.to_le_bytes());
        for unit in substitute.into_iter().chain([0]).chain(print).chain([0]) {
            data.extend(unit.to_le_bytes());
        }
        let mut used = 0;
        if unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                0x900a4,
                data.as_ptr().cast(),
                data.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut used,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn assert_pending(root: &Path, reg: &Registration, journal: &[u8]) {
        candidates(root);
        assert_eq!(fs::read(reg.journal_path()).unwrap(), journal);
        let parsed: Journal = serde_json::from_slice(journal).unwrap();
        for change in &parsed.link_changes {
            assert!(fs::symlink_metadata(change.backup_path()).is_ok());
        }
    }

    #[test]
    fn a_committed_object_cannot_be_moved_into_another_registration_state() {
        let (root, reg) = fixture();
        let journal = fs::read(reg.journal_path()).unwrap();
        commit_only(&root, &reg);
        let other = Registration::open(&root.join("other-state")).unwrap();
        fs::rename(reg.state.join(COMMIT), other.state.join(COMMIT)).unwrap();
        let moved = fs::read(other.state.join(COMMIT)).unwrap();
        assert!(other.recover().is_err());
        assert!(other.disconnect().is_err());
        assert!(other.finish(&root.join("metadata")).is_err());
        assert_eq!(fs::read(other.state.join(COMMIT)).unwrap(), moved);
        assert_pending(&root, &reg, &journal);
    }

    #[test]
    #[ignore = "requires existing SeCreateSymbolicLinkPrivilege; explicit owned reparse mutation"]
    fn retargeted_backup_alias_stops_finish_and_committed_cleanup_before_any_deletion() {
        for committed in [false, true] {
            let (root, reg) = fixture();
            let journal = fs::read(reg.journal_path()).unwrap();
            let parsed: Journal = serde_json::from_slice(&journal).unwrap();
            let backup = parsed.link_changes[0].backup_path();
            if committed {
                commit_only(&root, &reg);
            }
            let commit = fs::read(reg.state.join(COMMIT)).ok();
            let alias = root.join("alias");
            symlink_file(root.join("old"), &alias).unwrap();
            retarget_backup(backup, &alias).expect("owned process privilege must be available");
            assert_eq!(
                backup.canonicalize().unwrap(),
                root.join("old").canonicalize().unwrap()
            );
            assert!(reg.finish(&root.join("metadata")).is_err());
            if committed {
                assert!(reg.recover().is_err());
                assert!(reg.disconnect().is_err());
            }
            assert_pending(&root, &reg, &journal);
            assert_eq!(fs::read_link(backup).unwrap(), alias);
            assert_eq!(fs::read(reg.state.join(COMMIT)).ok(), commit);
            retarget_backup(backup, &root.join("old")).unwrap();
            let report = if committed {
                reg.recover().unwrap()
            } else {
                reg.finish(&root.join("metadata")).unwrap()
            };
            assert!(report.committed);
            candidates(&root);
        }
    }

    #[test]
    fn every_candidate_and_backup_remains_guarded_through_cleanup() {
        let (root, reg) = fixture();
        let journal = reg.load_journal().unwrap().unwrap().journal;
        let report = reg
            .finish_inner(&root.join("metadata"), &mut |phase, index| {
                assert!(fs::write(root.join("metadata"), b"foreign").is_err());
                assert!(fs::write(root.join("config"), b"foreign").is_err());
                assert!(fs::rename(root.join("fresh"), root.join("moved-fresh")).is_err());
                assert!(fs::rename(root.join("replace"), root.join("moved-replace")).is_err());
                if phase == "before" || phase == "committed" || (phase == "backup" && index == 0) {
                    let backup = journal.link_changes.last().unwrap().backup_path();
                    assert!(fs::rename(backup, root.join("moved-backup")).is_err());
                }
                Ok(())
            })
            .unwrap();
        assert!(report.committed);
        candidates(&root);
    }

    #[test]
    fn foreign_commit_replacement_at_the_publication_boundary_is_preserved() {
        let (root, reg) = fixture();
        let journal = fs::read(reg.journal_path()).unwrap();
        let result = reg.finish_inner(&root.join("metadata"), &mut |phase, _| {
            if phase == "decision" {
                let path = reg.state.join(COMMIT);
                let bytes = fs::read(&path).unwrap();
                fs::rename(&path, root.join("retained-decision")).unwrap();
                fs::write(&path, bytes).unwrap();
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_pending(&root, &reg, &journal);
        assert_eq!(
            fs::read(reg.state.join(COMMIT)).unwrap(),
            fs::read(root.join("retained-decision")).unwrap()
        );
        assert!(reg.recover().is_err());
        assert!(reg.disconnect().is_err());
    }

    #[test]
    fn postcommit_foreign_or_missing_live_objects_block_every_cleanup() {
        for changed in [
            "metadata",
            "fresh",
            "config",
            "backup",
            "commit",
            "journal",
            "completion",
            "missing-metadata",
        ] {
            let (root, reg) = fixture();
            let journal = fs::read(reg.journal_path()).unwrap();
            commit_only(&root, &reg);
            let commit = fs::read(reg.state.join(COMMIT)).unwrap();
            let parsed: Journal = serde_json::from_slice(&journal).unwrap();
            let path = match changed {
                "commit" => reg.state.join(COMMIT),
                "journal" => reg.journal_path(),
                "completion" => reg.state.join(COMPLETION),
                "backup" => parsed.link_changes[0].backup_path().to_owned(),
                "missing-metadata" => root.join("metadata"),
                _ => root.join(changed),
            };
            let bytes = if changed == "fresh" || changed == "backup" {
                None
            } else {
                Some(fs::read(&path).unwrap())
            };
            fs::rename(&path, root.join("retained-actor-object")).unwrap();
            if changed != "missing-metadata" {
                match bytes {
                    Some(bytes) => fs::write(&path, bytes).unwrap(),
                    None => symlink_file(
                        if changed == "fresh" {
                            root.join("new")
                        } else {
                            root.join("old")
                        },
                        &path,
                    )
                    .unwrap(),
                }
            }
            for result in [
                reg.recover(),
                reg.disconnect(),
                reg.finish(&root.join("metadata")),
            ] {
                assert!(result.is_err(), "foreign {changed} accepted");
            }
            assert_eq!(fs::read(reg.journal_path()).unwrap(), journal);
            assert_eq!(fs::read(reg.state.join(COMMIT)).unwrap(), commit);
            for change in &parsed.link_changes {
                assert!(fs::symlink_metadata(change.backup_path()).is_ok());
            }
            assert_eq!(fs::read(root.join("config")).unwrap(), b"after");
            assert_eq!(
                fs::read_link(root.join("replace")).unwrap(),
                root.join("new")
            );
        }
    }

    #[test]
    fn schema6_and_schema7_intent_is_unsupported_and_preserved_by_every_current_entrypoint() {
        for older in [6, 7] {
            let (root, reg) = fixture();
            let mut journal = reg.load_journal().unwrap().unwrap().journal;
            journal.schema = older;
            // The predecessor's actual checksum tuple: make valid old intent,
            // rather than relying on corruption to stop the current reader.
            journal.checksum = build_identity::hash_bytes(
                &serde_json::to_vec(&(
                    journal.schema,
                    &journal.records,
                    &journal.configurations,
                    &journal.creations,
                    &journal.link_changes,
                ))
                .unwrap(),
            );
            let bytes = serde_json::to_vec(&journal).unwrap();
            let completion = serde_json::to_vec_pretty(&Completion {
                schema: older,
                journal_sha256: build_identity::hash_bytes(&bytes),
            })
            .unwrap();
            fs::write(reg.journal_path(), &bytes).unwrap();
            fs::write(reg.state.join(COMPLETION), &completion).unwrap();
            for result in [
                reg.recover(),
                reg.disconnect(),
                reg.finish(&root.join("metadata")),
            ] {
                assert!(result.unwrap_err().to_string().contains("unsupported"));
            }
            assert!(
                reg.apply(&[])
                    .unwrap_err()
                    .to_string()
                    .contains("unsupported")
            );
            assert_pending(&root, &reg, &bytes);
            assert_eq!(fs::read(reg.state.join(COMPLETION)).unwrap(), completion);
            assert!(!reg.state.join(COMMIT).exists());
        }
    }

    #[test]
    fn reused_or_metadata_identity_conflicts_stop_finish_cleanup_and_undo() {
        use super::super::metadata::MetadataDestination;
        for committed in [false, true] {
            for mutation in [
                "file",
                "directory",
                "missing-file",
                "missing-directory",
                "metadata",
            ] {
                let (root, reg) = fixture();
                reg.disconnect().unwrap();
                let directory = mutation.contains("directory");
                let target = root.join(if directory { "old-dir" } else { "old" });
                let reused = root.join("reused");
                if directory {
                    symlink_dir(&target, &reused).unwrap();
                } else {
                    symlink_file(&target, &reused).unwrap();
                }
                let reused_id = native::link_identity(&reused, &target, directory).unwrap();
                reg.apply_with_metadata(
                    &[
                        Link {
                            kind: "fixture".into(),
                            name: "fresh".into(),
                            source: root.join("new"),
                            destination: root.join("fresh"),
                            connection: inventory::Connection::Missing,
                        },
                        Link {
                            kind: "fixture".into(),
                            name: "reused".into(),
                            source: target.clone(),
                            destination: reused.clone(),
                            connection: inventory::Connection::Missing,
                        },
                    ],
                    &[ConfigSnapshot::read(&root.join("config"))
                        .unwrap()
                        .plan_replace(b"after")
                        .unwrap()],
                    &[],
                    &[
                        LinkChange::replace(
                            &root.join("replace"),
                            &root.join("old"),
                            &root.join("new"),
                        )
                        .unwrap(),
                        LinkChange::remove(&root.join("remove"), &root.join("old-dir")).unwrap(),
                    ],
                    MetadataDestination::absent(&root.join("metadata")).unwrap(),
                    |_| Ok(b"PRIVATE_FINISH_SENTINEL".to_vec()),
                )
                .unwrap();
                let journal = fs::read(reg.journal_path()).unwrap();
                let parsed: Journal = serde_json::from_slice(&journal).unwrap();
                assert_eq!(parsed.records[1].reused_identity, Some(reused_id.clone()));
                let metadata_id = parsed.creations[0].published_identity().clone();
                if committed {
                    commit_only(&root, &reg);
                }
                let commit = fs::read(reg.state.join(COMMIT)).ok();
                let completion = fs::read(reg.state.join(COMPLETION)).unwrap();
                let path = if mutation == "metadata" {
                    root.join("metadata")
                } else {
                    reused.clone()
                };
                let retained = root.join("retained-original");
                fs::rename(&path, &retained).unwrap();
                if mutation == "metadata" {
                    fs::write(&path, b"PRIVATE_FINISH_SENTINEL").unwrap();
                    let foreign =
                        FileGuard::open_regular(&path, b"PRIVATE_FINISH_SENTINEL").unwrap();
                    assert_ne!(foreign.object_identity().unwrap(), metadata_id);
                } else if !mutation.starts_with("missing") {
                    if directory {
                        symlink_dir(&target, &path).unwrap();
                    } else {
                        symlink_file(&target, &path).unwrap();
                    }
                    assert_ne!(
                        native::link_identity(&path, &target, directory).unwrap(),
                        reused_id
                    );
                }
                for result in [
                    reg.finish(&root.join("metadata")),
                    reg.disconnect(),
                    reg.recover(),
                ] {
                    assert!(result.is_err(), "{mutation}, committed={committed}");
                }
                assert_pending(&root, &reg, &journal);
                assert_eq!(fs::read(reg.state.join(COMPLETION)).unwrap(), completion);
                assert_eq!(fs::read(reg.state.join(COMMIT)).ok(), commit);
                assert!(fs::symlink_metadata(&retained).is_ok());
                // Keep the foreign version as evidence. Republish only the exact
                // retained original, preserving its ID despite NTFS tunneling.
                if !mutation.starts_with("missing") {
                    fs::rename(&path, root.join("retained-foreign")).unwrap();
                }
                if mutation == "metadata" {
                    native::publish_regular(
                        &retained,
                        &path,
                        b"PRIVATE_FINISH_SENTINEL",
                        &metadata_id,
                    )
                    .unwrap();
                } else {
                    native::publish_link(&retained, &path, &target, directory, &reused_id).unwrap();
                }
                if committed {
                    assert!(reg.recover().unwrap().committed);
                    candidates(&root);
                } else {
                    assert!(!reg.disconnect().unwrap().committed);
                    assert_eq!(fs::read(root.join("config")).unwrap(), b"before");
                    assert!(!root.join("metadata").exists());
                    assert!(!root.join("fresh").exists());
                }
                assert_eq!(
                    native::link_identity(&reused, &target, directory).unwrap(),
                    reused_id
                );
                assert!(!reg.journal_path().exists());
                assert!(!reg.state.join(COMMIT).exists());
            }
        }
    }

    #[test]
    fn malformed_or_rebound_commitment_preserved_even_without_original_journal() {
        for absent in [false, true] {
            for alteration in [
                "syntax",
                "checksum",
                "witness",
                "completion",
                "digest",
                "bound",
            ] {
                let (root, reg) = fixture();
                commit_only(&root, &reg);
                let journal = fs::read(reg.journal_path()).unwrap();
                let mut decision: Commitment =
                    serde_json::from_slice(&fs::read(reg.state.join(COMMIT)).unwrap()).unwrap();
                match alteration {
                    "witness" => decision.metadata.sha256 = "wrong".into(),
                    "completion" => decision.completion = "{}".into(),
                    "digest" => decision.journal_sha256 = "wrong".into(),
                    "bound" => decision.journal = " ".repeat(MAX_JOURNAL as usize + 1),
                    _ => {}
                }
                decision.checksum = decision.checksum().unwrap();
                if alteration == "checksum" {
                    decision.checksum = "wrong".into();
                }
                let bytes = if alteration == "syntax" {
                    b"{PRIVATE_FINISH_SENTINEL".to_vec()
                } else {
                    serde_json::to_vec(&decision).unwrap()
                };
                // Modify the owned decision object in place: its ID is unchanged,
                // so the intended binding/bound check, not a replacement, fails.
                fs::write(reg.state.join(COMMIT), &bytes).unwrap();
                if absent {
                    fs::remove_file(reg.journal_path()).unwrap();
                }
                for result in [
                    reg.recover(),
                    reg.disconnect(),
                    reg.finish(&root.join("metadata")),
                ] {
                    let error = result.unwrap_err().to_string();
                    assert!(!error.contains("PRIVATE_FINISH_SENTINEL"));
                }
                assert_eq!(fs::read(reg.state.join(COMMIT)).unwrap(), bytes);
                candidates(&root);
                let parsed: Journal = serde_json::from_slice(&journal).unwrap();
                for change in &parsed.link_changes {
                    assert!(fs::symlink_metadata(change.backup_path()).is_ok());
                }
                if !absent {
                    assert_eq!(fs::read(reg.journal_path()).unwrap(), journal);
                }
            }
        }
    }

    #[test]
    #[ignore = "owned child of killed_finish_at_each_boundary_never_undoes_committed_candidates"]
    fn finish_process_fixture() {
        let root = PathBuf::from(std::env::var_os("HARNESS_FINISH_ROOT").unwrap());
        let requested = std::env::var("HARNESS_FINISH_PHASE").unwrap();
        let reg = Registration::open(&root.join("state")).unwrap();
        reg.finish_inner(&root.join("metadata"), &mut |phase, index| {
            if format!("{phase}-{index}") == requested {
                fs::write(root.join("paused"), requested.as_bytes()).unwrap();
                std::thread::sleep(std::time::Duration::from_secs(20));
                std::process::exit(79);
            }
            Ok(())
        })
        .unwrap();
        panic!("requested finish boundary not reached");
    }

    #[test]
    fn killed_finish_at_each_boundary_never_undoes_committed_candidates() {
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        for phase in [
            "before-0",
            "decision-0",
            "committed-0",
            "backup-0",
            "backup-1",
            "completion-0",
            "journal-0",
            "retired-0",
        ] {
            let (root, reg) = fixture();
            let journal = reg.load_journal().unwrap().unwrap().journal;
            drop(reg);
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "registration::finish::tests::finish_process_fixture",
                    "--nocapture",
                ])
                .env("HARNESS_FINISH_ROOT", &root)
                .env("HARNESS_FINISH_PHASE", phase)
                .stdin(Stdio::null())
                .stdout(fs::File::create(root.join("child.stdout")).unwrap())
                .stderr(fs::File::create(root.join("child.stderr")).unwrap())
                .spawn()
                .unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while !root.join("paused").exists() && Instant::now() < until {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let reached = fs::read(root.join("paused")).ok().as_deref() == Some(phase.as_bytes());
            // Retained exact child handle; no descendants, PID/name searches,
            // Rust unwinding, or Drop cleanup are involved in this oracle.
            let _ = child.kill();
            let exit = child.wait().unwrap();
            assert!(reached, "boundary {phase} not observed: {}", root.display());
            assert!(!exit.success());
            assert_ne!(exit.code(), Some(79));
            let reg = Registration::open(&root.join("state")).unwrap();
            if phase == "before-0" {
                assert!(!reg.state.join(COMMIT).exists());
                assert!(reg.recover().is_err());
                assert!(!reg.disconnect().unwrap().committed);
                assert_eq!(fs::read(root.join("config")).unwrap(), b"before");
                assert!(!root.join("metadata").exists());
                assert!(!root.join("fresh").exists());
                for change in &journal.link_changes {
                    assert!(!change.check_undo().unwrap());
                }
            } else {
                candidates(&root);
                let report = if phase == "backup-0" {
                    reg.disconnect().unwrap()
                } else {
                    reg.recover().unwrap()
                };
                assert_eq!(report.committed, phase != "retired-0");
                assert!(report.restored.is_empty() && report.removed.is_empty());
                candidates(&root);
            }
            for change in &journal.link_changes {
                assert!(!change.backup_path().exists());
            }
            assert!(!reg.journal_path().exists());
            assert!(!reg.state.join(COMPLETION).exists());
            assert!(!reg.state.join(COMMIT).exists());
        }
    }
}
