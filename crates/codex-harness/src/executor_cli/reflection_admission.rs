//! Short-lived reflection admission in the existing repository run owner.
//! The lease covers one native stage and never allocates a worktree position.

use super::*;
use harness_core::process::ExclusiveFileLock;
use sha2::{Digest, Sha256};

const PREFIX: &str = "reflection-lease-";

pub(crate) struct Admission {
    path: PathBuf,
    record: SessionLease,
    _lock: ExclusiveFileLock,
}

pub(crate) fn acquire(
    home: &Path,
    repository: &Path,
    config: &orchestration_config::Orchestration,
    chain: &Path,
    stage: Role,
    owner: &str,
) -> io::Result<Admission> {
    if !matches!(
        stage,
        Role::ExecutorSelfReflection | Role::LeadSelfReflection
    ) {
        return Err(invalid("reflection admission requires a reflection role"));
    }
    let _admission = task_worktree::begin_pool_admission(home, repository)?;
    admit_active_run(
        home,
        repository,
        worktree_pool_size(config),
        config.max_concurrent_executors,
        None,
        stage,
    )?;
    let key = format!(
        "{:x}",
        Sha256::digest(format!("{}:{stage:?}", chain.display()).as_bytes())
    );
    let path = task_worktree::pool_state_dir(home, repository)?.join(format!("{PREFIX}{key}.json"));
    let lock = ExclusiveFileLock::try_acquire(&path.with_extension("lock"))?.ok_or_else(|| {
        invalid("this reflection stage already holds admission; preserve its current owner")
    })?;
    let program = std::env::current_exe()?.canonicalize()?;
    let user = harness_core::process_service::current_user()?;
    let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)?.identity();
    let record = SessionLease {
        schema: 1,
        owner: format!("{stage:?}: {owner}"),
        // Zero explicitly means no worktree position. These records are read
        // only as reflection stage leases, never as an executor slot record.
        index: 0,
        path: repository.to_owned(),
        pid: identity.pid,
        created: identity.creation_time,
        program,
    };
    control::write_staged(&path, &serde_json::to_vec(&record)?)?;
    Ok(Admission {
        path,
        record,
        _lock: lock,
    })
}

pub(super) fn active(home: &Path, repository: &Path) -> io::Result<Vec<String>> {
    let directory = task_worktree::pool_state_dir(home, repository)?;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut active = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(PREFIX) && name.ends_with(".json"))
        {
            continue;
        }
        let lock_path = path.with_extension("lock");
        // Stage lifetime is shorter than its hosting lead's process lifetime.
        // An unlocked stale receipt must not occupy capacity until lead exit.
        if !lock_path.is_file() {
            return Err(invalid(&format!(
                "reflection lease {} has no lifetime lock; reconcile its owner before dispatch",
                path.display()
            )));
        }
        if ExclusiveFileLock::try_acquire(&lock_path)?.is_some() {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let lease: SessionLease = serde_json::from_slice(&bytes).map_err(|error| invalid(&format!(
            "active reflection lease {} is unreadable: {error}; preserve active work and reconcile that receipt", path.display()
        )))?;
        if lease.schema != 1 || lease.index != 0 || lease.owner.trim().is_empty() {
            return Err(invalid(&format!(
                "active reflection lease {} has an unsupported identity",
                path.display()
            )));
        }
        active.push(format!("reflection '{}'", lease.owner));
    }
    active.sort();
    Ok(active)
}

impl Drop for Admission {
    fn drop(&mut self) {
        if read_lease(&self.path).as_ref() == Some(&self.record)
            && let Err(error) = fs::remove_file(&self.path)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!(
                "reflection admission ended, but its receipt remains at {}: {error}; the released OS lock makes it inactive",
                self.path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (
        tempfile::TempDir,
        PathBuf,
        PathBuf,
        orchestration_config::Orchestration,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let repository = temp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&repository).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let mut config = orchestration_config::load(root).unwrap();
        config.max_concurrent_executors = 1;
        config.worktree_pool_size = Some(2);
        (temp, home, repository, config)
    }

    #[test]
    fn reflection_and_ordinary_dispatch_share_capacity_without_a_worktree() {
        let (_temp, home, repository, config) = setup();
        let chain = home.join("chain");
        let guard = acquire(
            &home,
            &repository,
            &config,
            &chain,
            Role::ExecutorSelfReflection,
            "accepted task",
        )
        .unwrap();
        assert!(!owner_live(&home, &repository, &guard.record.owner));
        let error = admit_active_run(&home, &repository, 2, 1, None, Role::Executor).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("reflection 'ExecutorSelfReflection: accepted task'"),
            "{error}"
        );
        for index in 1..=2 {
            assert!(
                task_worktree::load_slot_record(&home, &repository, index)
                    .unwrap()
                    .is_none()
            );
        }
        let mut ordinary = guard.record.clone();
        ordinary.index = 1;
        ordinary.owner = "ordinary native host".to_owned();
        drop(guard);
        admit_active_run(&home, &repository, 2, 1, None, Role::Executor).unwrap();
        let path = lease_path(&home, &repository, 1).unwrap();
        control::write_staged(&path, &serde_json::to_vec(&ordinary).unwrap()).unwrap();
        let denied = acquire(
            &home,
            &repository,
            &config,
            &chain,
            Role::LeadSelfReflection,
            "accepted task",
        );
        assert!(denied.is_err());
        assert!(
            denied
                .err()
                .unwrap()
                .to_string()
                .contains("ordinary native host")
        );
        fs::remove_file(&path).unwrap();
        acquire(
            &home,
            &repository,
            &config,
            &chain,
            Role::LeadSelfReflection,
            "accepted task",
        )
        .unwrap();
    }

    #[test]
    fn released_stage_receipt_does_not_reserve_a_live_host_until_process_exit() {
        let (_temp, home, repository, config) = setup();
        let chain = home.join("chain");
        let guard = acquire(
            &home,
            &repository,
            &config,
            &chain,
            Role::ExecutorSelfReflection,
            "accepted task",
        )
        .unwrap();
        let path = guard.path.clone();
        let receipt = serde_json::to_vec(&guard.record).unwrap();
        drop(guard);
        fs::write(&path, receipt).unwrap();
        assert!(active(&home, &repository).unwrap().is_empty());
        acquire(
            &home,
            &repository,
            &config,
            &chain,
            Role::ExecutorSelfReflection,
            "accepted task",
        )
        .unwrap();
    }

    #[test]
    fn concurrent_reflection_requests_cannot_overbook_the_shared_allowance() {
        let (_temp, home, repository, config) = setup();
        let boundary = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let run = |name: &str| {
                let result = acquire(
                    &home,
                    &repository,
                    &config,
                    &home.join(name),
                    Role::ExecutorSelfReflection,
                    name,
                );
                boundary.wait();
                result.is_ok()
            };
            let first = scope.spawn(move || run("first"));
            let second = scope.spawn(move || run("second"));
            [first.join().unwrap(), second.join().unwrap()]
        });
        assert_eq!(results.into_iter().filter(|admitted| *admitted).count(), 1);
        assert!(active(&home, &repository).unwrap().is_empty());
    }

    #[test]
    fn unreadable_active_receipt_refuses_instead_of_granting_extra_capacity() {
        let (_temp, home, repository, config) = setup();
        let guard = acquire(
            &home,
            &repository,
            &config,
            &home.join("chain"),
            Role::ExecutorSelfReflection,
            "accepted task",
        )
        .unwrap();
        fs::write(&guard.path, b"{}").unwrap();
        assert!(
            admit_active_run(&home, &repository, 2, 2, None, Role::Executor)
                .unwrap_err()
                .to_string()
                .contains("unreadable")
        );
        fs::write(&guard.path, serde_json::to_vec(&guard.record).unwrap()).unwrap();
    }
}
