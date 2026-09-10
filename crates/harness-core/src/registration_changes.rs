//! Reversible replacement/removal of explicitly captured existing links.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Object {
    target: PathBuf,
    link_type: LinkType,
    identity: LinkIdentity,
}

impl Object {
    fn hold(&self, path: &Path) -> io::Result<FileGuard> {
        let guard =
            native::verified_link(path, &self.target, self.link_type == LinkType::Directory)?;
        if guard.object_identity()? != self.identity {
            return Err(invalid(
                "link change object identity changed; preserving it",
            ));
        }
        Ok(guard)
    }

    fn read(path: &Path) -> io::Result<Option<Self>> {
        match inspect(path)? {
            Presence::Missing => Ok(None),
            Presence::Owned(link_type, target) => {
                let identity =
                    native::link_identity(path, &target, link_type == LinkType::Directory)?;
                Ok(Some(Self {
                    target,
                    link_type,
                    identity,
                }))
            }
            Presence::Foreign => Err(invalid("link change found a foreign object; preserving it")),
        }
    }

    fn matches(&self, other: &Self) -> io::Result<bool> {
        Ok(self.identity == other.identity
            && self.link_type == other.link_type
            && native::targets_match(&self.target, &other.target)?)
    }

    fn publish(&self, from: &Path, to: &Path) -> io::Result<()> {
        native::publish_link(
            from,
            to,
            &self.target,
            self.link_type == LinkType::Directory,
            &self.identity,
        )
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        native::remove_link(
            path,
            &self.target,
            self.link_type == LinkType::Directory,
            &self.identity,
        )
    }
}

/// Explicit authority to change a captured link. An installer must first
/// establish its ownership from installation metadata; capture alone does not
/// establish that the kit owns an arbitrary matching link.
#[derive(Debug)]
pub struct LinkChange {
    pub(super) path: PathBuf,
    previous: Object,
    next: Option<(PathBuf, LinkType)>,
}

impl LinkChange {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub fn replace(path: &Path, expected_target: &Path, next_target: &Path) -> io::Result<Self> {
        let mut change = Self::remove(path, expected_target)?;
        let next = exact_source(next_target)?;
        let kind = source_type(&next)?;
        change.next = Some((next, kind));
        Ok(change)
    }

    pub fn remove(path: &Path, expected_target: &Path) -> io::Result<Self> {
        let path = exact_destination(path)?;
        // The old checkout may already be unavailable after relocation. Its
        // recorded target is a name to verify, never a path to follow or read.
        refuse_escape(expected_target)?;
        let expected = std::path::absolute(expected_target)?;
        let previous = Object::read(&path)?.ok_or_else(|| invalid("captured link is missing"))?;
        if !native::targets_match(&previous.target, &expected)? {
            return Err(invalid("captured link target differs; preserving it"));
        }
        Ok(Self {
            path,
            previous,
            next: None,
        })
    }

    pub(super) fn hold(&self) -> io::Result<FileGuard> {
        let guard = native::verified_link(
            &self.path,
            &self.previous.target,
            self.previous.link_type == LinkType::Directory,
        )?;
        if guard.object_identity()? != self.previous.identity {
            return Err(invalid("captured link identity changed; preserving it"));
        }
        Ok(guard)
    }

    pub(super) fn stage(&self, index: usize) -> io::Result<(ChangeRecord, Option<StagedLink>)> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let parent = self.path.parent().expect("validated destination");
        let backup = parent.join(format!(
            ".codex-harness-old-{}-{stamp}-{index}",
            std::process::id()
        ));
        if !matches!(inspect(&backup)?, Presence::Missing) {
            return Err(invalid("link rollback name already exists; preserving it"));
        }
        let mut staged = None;
        let next = if let Some((target, kind)) = &self.next {
            let path = parent.join(format!(
                ".codex-harness-new-{}-{stamp}-{index}",
                std::process::id()
            ));
            let guard = StagedLink::create(&path, target, *kind == LinkType::Directory)?;
            let object = Object {
                target: target.clone(),
                link_type: *kind,
                identity: guard.identity(),
            };
            staged = Some(guard);
            Some((path, object))
        } else {
            None
        };
        Ok((
            ChangeRecord {
                path: self.path.clone(),
                previous: self.previous.clone(),
                backup,
                next,
            },
            staged,
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChangeRecord {
    pub(super) path: PathBuf,
    previous: Object,
    backup: PathBuf,
    next: Option<(PathBuf, Object)>,
}

impl ChangeRecord {
    pub(super) fn backup_path(&self) -> &Path {
        &self.backup
    }

    pub(super) fn stage_path(&self) -> Option<&Path> {
        self.next.as_ref().map(|(path, _)| path.as_path())
    }

    /// Keep live candidates and every remaining rollback object protected until
    /// the irreversible finish decision and cleanup have completed.
    pub(super) fn hold_finished(
        &self,
        committed: bool,
    ) -> io::Result<(Option<FileGuard>, Option<FileGuard>)> {
        let live = match &self.next {
            Some((stage, next)) => {
                super::finish::require_absent(stage)?;
                Some(next.hold(&self.path)?)
            }
            None => {
                super::finish::require_absent(&self.path)?;
                None
            }
        };
        let backup = match self.previous.hold(&self.backup) {
            Ok(guard) => Some(guard),
            Err(error) if committed && error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok((live, backup))
    }

    pub(super) fn validate(&self) -> io::Result<()> {
        refuse_escape(&self.path)?;
        refuse_escape(&self.previous.target)?;
        for path in std::iter::once(&self.backup).chain(self.next.iter().map(|(path, _)| path)) {
            refuse_escape(path)?;
            if path == &self.path || path.parent() != self.path.parent() {
                return Err(invalid("link change staging namespace is inconsistent"));
            }
        }
        if let Some((path, object)) = &self.next {
            refuse_escape(&object.target)?;
            if path == &self.backup || object.identity == self.previous.identity {
                return Err(invalid("link change staging identity is inconsistent"));
            }
        }
        Ok(())
    }

    pub(super) fn same_candidate(&self, plan: &LinkChange) -> bool {
        self.path == plan.path
            && self.previous == plan.previous
            && self
                .next
                .as_ref()
                .map(|(_, object)| (&object.target, object.link_type))
                == plan.next.as_ref().map(|(target, kind)| (target, *kind))
    }

    pub(super) fn publish(&self, checkpoint: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
        self.previous.publish(&self.path, &self.backup)?;
        checkpoint()?;
        if let Some((stage, next)) = &self.next {
            next.publish(stage, &self.path)?;
        }
        Ok(())
    }

    pub(super) fn check_published(&self) -> io::Result<()> {
        let backup = Object::read(&self.backup)?
            .ok_or_else(|| invalid("link rollback object is missing"))?;
        if !self.previous.matches(&backup)? {
            return Err(invalid("link rollback object changed"));
        }
        match (&self.next, Object::read(&self.path)?) {
            (None, None) => Ok(()),
            (Some((_, expected)), Some(actual)) if expected.matches(&actual)? => Ok(()),
            _ => Err(invalid("changed link does not match the published state")),
        }
    }

    /// Returns whether the old object is at its rollback name. Missing or
    /// foreign originals cannot be reconstructed from a target string alone.
    pub(super) fn check_undo(&self) -> io::Result<bool> {
        let current = Object::read(&self.path)?;
        let backup = Object::read(&self.backup)?;
        if let Some((stage, expected)) = &self.next
            && let Some(actual) = Object::read(stage)?
            && !expected.matches(&actual)?
        {
            return Err(invalid("link staging object changed; preserving journal"));
        }
        match (current, backup) {
            (Some(current), None) if self.previous.matches(&current)? => Ok(false),
            (None, Some(backup)) if self.previous.matches(&backup)? => Ok(true),
            (Some(current), Some(backup)) if self.previous.matches(&backup)? => {
                if let Some((_, next)) = &self.next
                    && next.matches(&current)?
                {
                    Ok(true)
                } else {
                    Err(invalid("link destination changed; preserving journal"))
                }
            }
            _ => Err(invalid(
                "link rollback ownership changed; preserving journal",
            )),
        }
    }

    pub(super) fn undo(&self) -> io::Result<bool> {
        let restore = self.check_undo()?;
        if let Some((stage, next)) = &self.next {
            if Object::read(stage)?.is_some() {
                next.remove(stage)?;
            }
            if restore && Object::read(&self.path)?.is_some() {
                next.remove(&self.path)?;
            }
        }
        if restore {
            self.previous.publish(&self.backup, &self.path)?;
        }
        Ok(restore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::symlink_file;

    fn fixture() -> PathBuf {
        let root = tempfile::Builder::new()
            .prefix("harness-link-change-recovery-")
            .tempdir()
            .unwrap()
            .keep();
        fs::write(root.join("old"), b"old source").unwrap();
        fs::write(root.join("new"), b"new source").unwrap();
        fs::write(root.join("config.toml"), b"before").unwrap();
        symlink_file(root.join("old"), root.join("link")).unwrap();
        println!("link change recovery evidence: {}", root.display());
        root
    }

    #[test]
    fn failure_between_old_and_new_preserves_foreign_actor_and_all_rollback_data() {
        for foreign in [false, true] {
            let root = fixture();
            let path = root.join("link");
            let before = Object::read(&path).unwrap().unwrap();
            let change = LinkChange::replace(&path, &root.join("old"), &root.join("new")).unwrap();
            let config = crate::config_file::ConfigSnapshot::read(&root.join("config.toml"))
                .unwrap()
                .plan_replace(b"after")
                .unwrap();
            let reg = Registration::open(&root.join("state")).unwrap();
            let mut count = 0;
            let result = reg.apply_inner(&[], &[config], &[], &[change], || {
                count += 1;
                if count != 2 {
                    return Ok(());
                }
                if foreign {
                    // Same target as the candidate, but another object identity.
                    symlink_file(root.join("new"), &path).unwrap();
                    Ok(())
                } else {
                    Err(io::Error::other(
                        "injected interruption after old-link move",
                    ))
                }
            });
            assert!(result.is_err());
            let saved = reg.load_journal().unwrap().unwrap();
            let record = &saved.journal.link_changes[0];
            assert_eq!(Object::read(&record.backup).unwrap(), Some(before.clone()));
            assert!(record.next.as_ref().unwrap().0.exists());
            if foreign {
                let actor = Object::read(&path).unwrap().unwrap();
                assert!(reg.recover().is_err());
                assert_eq!(Object::read(&path).unwrap(), Some(actor));
                assert_eq!(fs::read(root.join("config.toml")).unwrap(), b"after");
                assert_eq!(fs::read(reg.journal_path()).unwrap(), saved.bytes);
                fs::remove_file(&path).unwrap();
            }
            reg.recover().unwrap();
            assert_eq!(Object::read(&path).unwrap(), Some(before));
            assert_eq!(fs::read(root.join("config.toml")).unwrap(), b"before");
            assert!(!record.backup.exists());
            assert!(!record.next.as_ref().unwrap().0.exists());
            assert!(!reg.journal_path().exists());
        }
    }

    #[test]
    #[ignore = "owned child of actual_process_interruption_restores_original_link_identity"]
    fn process_fixture() {
        let root = PathBuf::from(std::env::var_os("HARNESS_LINK_CHANGE_ROOT").unwrap());
        let stop_at: usize = std::env::var("HARNESS_LINK_CHANGE_PHASE")
            .unwrap()
            .parse()
            .unwrap();
        let change =
            LinkChange::replace(&root.join("link"), &root.join("old"), &root.join("new")).unwrap();
        let reg = Registration::open(&root.join("state")).unwrap();
        let mut count = 0;
        reg.apply_inner(&[], &[], &[], &[change], || {
            count += 1;
            if count == stop_at {
                fs::write(root.join("paused"), b"ready").unwrap();
                std::thread::sleep(std::time::Duration::from_secs(20));
                std::process::exit(79);
            }
            Ok(())
        })
        .unwrap();
        panic!("requested interruption phase was not reached");
    }

    #[test]
    fn missing_or_replaced_old_object_cannot_be_recreated_from_its_target_name() {
        for replace in [false, true] {
            let root = fixture();
            let reg = Registration::open(&root.join("state")).unwrap();
            let change =
                LinkChange::replace(&root.join("link"), &root.join("old"), &root.join("new"))
                    .unwrap();
            reg.apply_with_changes(&[], &[], &[], &[change]).unwrap();
            let snapshot = reg.load_journal().unwrap().unwrap();
            let record = &snapshot.journal.link_changes[0];
            let retained = root.join("retained-original");
            fs::rename(&record.backup, &retained).unwrap();
            if replace {
                symlink_file(root.join("old"), &record.backup).unwrap();
            }
            assert!(reg.disconnect().is_err());
            assert_eq!(fs::read_link(root.join("link")).unwrap(), root.join("new"));
            assert_eq!(fs::read(reg.journal_path()).unwrap(), snapshot.bytes);
            assert_eq!(
                Object::read(&retained).unwrap(),
                Some(record.previous.clone())
            );
            if replace {
                fs::remove_file(&record.backup).unwrap();
            }
            // Put this owned actor's original object back, not a recreated link.
            fs::rename(retained, &record.backup).unwrap();
            reg.disconnect().unwrap();
            assert_eq!(
                Object::read(&root.join("link")).unwrap(),
                Some(record.previous.clone())
            );
        }
    }

    #[test]
    fn actual_process_interruption_restores_original_link_identity() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        for phase in [1, 2] {
            let root = fixture();
            let original = Object::read(&root.join("link")).unwrap().unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "registration::changes::tests::process_fixture",
                    "--nocapture",
                ])
                .env("HARNESS_LINK_CHANGE_ROOT", &root)
                .env("HARNESS_LINK_CHANGE_PHASE", phase.to_string())
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
            let reached = root.join("paused").exists();
            // Retained handle, no descendant creation, and cleanup on timeout.
            let _ = child.kill();
            let status = child.wait().unwrap();
            assert!(
                reached,
                "phase {phase} was not observed: {}",
                root.display()
            );
            assert!(!status.success());
            assert_ne!(status.code(), Some(79));
            let reg = Registration::open(&root.join("state")).unwrap();
            assert!(!reg.state.join(COMPLETION).exists());
            let snapshot = reg.load_journal().unwrap().unwrap();
            let record = &snapshot.journal.link_changes[0];
            assert_eq!(
                Object::read(&record.backup).unwrap(),
                Some(original.clone())
            );
            if phase == 1 {
                assert!(Object::read(&root.join("link")).unwrap().is_none());
            } else {
                assert_eq!(fs::read_link(root.join("link")).unwrap(), root.join("new"));
            }
            reg.recover().unwrap();
            assert_eq!(Object::read(&root.join("link")).unwrap(), Some(original));
            assert!(!record.backup.exists());
            assert!(!record.next.as_ref().unwrap().0.exists());
            assert_eq!(fs::read(root.join("old")).unwrap(), b"old source");
            assert_eq!(fs::read(root.join("new")).unwrap(), b"new source");
        }
    }
}
