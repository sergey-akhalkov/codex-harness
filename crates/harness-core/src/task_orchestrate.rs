//! Lead/executor merge, reassignment, view-loss suspension and recovery rules.
use crate::{
    task_failure::FailureCause,
    task_store::{self, Assignment, TaskRecord},
    task_worktree::{self, LaneDisposition},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
    process::Command,
};

/// Board metadata holding the exact source revisions accepted together. The
/// board remains their owner; this is evidence attached to the acceptance,
/// not a second task store.
pub const ACCEPTANCE_REVISIONS_METADATA: &str = "harness.acceptanceRevisions";
const MAX_ACCEPTANCE_REPOSITORIES: usize = 16;

/// One repository and managed checkout at the revision the reviewer accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryRevision {
    pub repository: String,
    pub checkout: String,
    pub revision: String,
}

/// Refuses an acceptance that does not follow the recorded hierarchy. An
/// executor may be accepted by its immediate Lead or the Director; a Lead may
/// be accepted only by the Director. A recorded parent must be the accepting
/// conversation, never a sibling or an inspecting ancestor.
pub fn validate_acceptance_relation(
    output_role: &str,
    accepting_role: &str,
    run_parent: Option<&str>,
    accepting_thread: &str,
) -> io::Result<()> {
    if !matches!(accepting_role, "lead" | "director") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("accepting role '{accepting_role}' is not a Lead or Director"),
        ));
    }
    match (output_role, accepting_role) {
        ("executor", "lead" | "director") | ("lead", "director") => {}
        ("lead", "lead") => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "a delegated Lead cannot accept another Lead; the Director owns that acceptance",
            ));
        }
        (role, _) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("run role '{role}' is not an executor or Lead output"),
            ));
        }
    }
    match run_parent {
        Some(parent) if parent == accepting_thread => Ok(()),
        Some(parent) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "run parent {parent} is not the accepting conversation {accepting_thread}; only the immediate parent accepts"
            ),
        )),
        None if accepting_role == "director" && output_role == "executor" => Ok(()),
        None => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the run has no recorded immediate parent; hierarchical acceptance cannot establish that this conversation owns the result",
        )),
    }
}

/// Unclosed board children that gate a Lead block. Feedback cards are durable
/// observations rather than pending delivery work, so they do not hold the
/// block open.
pub fn unfinished_children(children: &[Value]) -> Vec<String> {
    children
        .iter()
        .filter(|child| child["status"].as_str() != Some("closed"))
        .filter(|child| {
            !child["labels"].as_array().is_some_and(|labels| {
                labels
                    .iter()
                    .any(|label| label.as_str() == Some("feedback"))
            })
        })
        .filter_map(|child| child["id"].as_str().map(str::to_owned))
        .collect()
}

pub fn refuse_unfinished_parent(task: &str, unfinished: &[String]) -> io::Result<()> {
    if unfinished.is_empty() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "parent task {task} still has unfinished board children: {}; no acceptance metadata or closure was written",
            unfinished.join(", ")
        ),
    ))
}

/// Reads the exact clean HEAD of every repository/checkout pair. Duplicate
/// repository declarations are refused and the returned vector is sorted, so
/// one workspace cannot stand in for a missing repository or be counted twice.
pub fn repository_revisions(
    repositories: &[(PathBuf, PathBuf)],
) -> io::Result<Vec<RepositoryRevision>> {
    if repositories.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "acceptance requires at least one repository revision",
        ));
    }
    if repositories.len() > MAX_ACCEPTANCE_REPOSITORIES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "acceptance names {} repositories; the limit is {MAX_ACCEPTANCE_REPOSITORIES}",
                repositories.len()
            ),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut evidence = Vec::with_capacity(repositories.len());
    for (repository, checkout) in repositories {
        let repository = repository.canonicalize().map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "accepted repository {} is unavailable: {error}",
                    repository.display()
                ),
            )
        })?;
        let checkout = checkout.canonicalize().map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "accepted checkout {} is unavailable: {error}",
                    checkout.display()
                ),
            )
        })?;
        let key = repository.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "accepted repository {} is declared more than once; one revision cannot be counted twice",
                    repository.display()
                ),
            ));
        }
        let revision = git_text(&checkout, &["rev-parse", "HEAD"])?.to_owned();
        if !(revision.len() == 40 || revision.len() == 64)
            || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "checkout {} did not resolve HEAD to an exact commit identifier",
                    checkout.display()
                ),
            ));
        }
        let dirty = git_text(
            &checkout,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?;
        if !dirty.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "checkout {} is not clean at {revision}; acceptance requires exact committed revisions, not working-tree state",
                    checkout.display()
                ),
            ));
        }
        evidence.push(RepositoryRevision {
            repository: repository.display().to_string(),
            checkout: checkout.display().to_string(),
            revision,
        });
    }
    evidence.sort_by(|left, right| left.repository.cmp(&right.repository));
    Ok(evidence)
}

fn git_text(cwd: &Path, args: &[&str]) -> io::Result<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git {} failed in {}: {}",
            args.join(" "),
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn view_loss(closed: &[&str]) -> Value {
    json!({
        "schema": 1,
        "closed": closed,
        "suspendDispatch": true,
        "replay": false,
        "reason": "conversation view closed"
    })
}

pub fn merge_accepted(
    record: &mut TaskRecord,
    assignment: &str,
    mapping: &task_worktree::Mapping,
    current: &Path,
) -> io::Result<LaneDisposition> {
    task_store::accept(record, assignment)?;
    task_store::validate_purpose(record)?;
    if mapping.path.is_dir() && current.is_dir() {
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&mapping.path)
            .output()?;
        if head.status.success() {
            let sha = String::from_utf8_lossy(&head.stdout).trim().to_owned();
            let merge = Command::new("git")
                .args(["merge", "--no-edit", &sha])
                .current_dir(current)
                .output()?;
            if !merge.status.success() {
                return Err(io::Error::other(
                    "lead merge failed; preserving the executor worktree",
                ));
            }
        }
    }
    if let Some(active) = record.assignments.iter().find(|item| {
        item.id != assignment
            && item.accepted != Some(true)
            && item
                .worktree
                .as_deref()
                .is_some_and(|worktree| same_workspace(worktree, &mapping.path))
    }) {
        return Ok(LaneDisposition::Preserved {
            limitation: format!(
                "workspace {} is still needed by unaccepted assignment {}; acceptance is recorded but the lane was not released",
                mapping.path.display(),
                active.id
            ),
        });
    }
    // Lanes are reused, not retired: reset the lane to the base the lead just
    // merged so the next task in this lane starts from it with its build
    // caches. Unresettable state is preserved for the lane retirement decision.
    let base = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(current)
        .output()?;
    if !base.status.success() {
        return Ok(LaneDisposition::Preserved {
            limitation: "merged base is unavailable; preserving the lane for retirement".into(),
        });
    }
    let base = String::from_utf8_lossy(&base.stdout).trim().to_owned();
    task_worktree::reset_for_reuse(mapping, current, &base)
}

fn same_workspace(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
        _ => left
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(right.to_string_lossy().trim_end_matches(['\\', '/'])),
    }
}

pub fn reconcile_before_replace(record: &TaskRecord, assignment: &str) -> io::Result<()> {
    let item = record
        .assignments
        .iter()
        .find(|item| item.id == assignment)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "assignment missing"))?;
    if item.owner_thread.is_some() && item.accepted != Some(true) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "surviving executor still owns the assignment; not replacing",
        ));
    }
    Ok(())
}

pub fn reassign(
    record: &mut TaskRecord,
    assignment: &str,
    cause: FailureCause,
    pool: &[String],
) -> io::Result<Option<String>> {
    if record.stopped {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stopped task stays stopped",
        ));
    }
    match cause {
        FailureCause::Quota | FailureCause::Authentication | FailureCause::Throttle => {}
        FailureCause::Unknown | FailureCause::IncompleteOutput => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "uncertain or incomplete output is not a provider failure; preserving work",
            ));
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cause does not authorize reassignment",
            ));
        }
    }
    task_store::validate_purpose(record)?;
    let current = record
        .assignments
        .iter()
        .find(|item| item.id == assignment)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "assignment missing"))?
        .profile
        .clone();
    let next = pool.iter().find(|profile| *profile != &current).cloned();
    if next.is_none() {
        return Ok(None);
    }
    let item = record
        .assignments
        .iter_mut()
        .find(|item| item.id == assignment)
        .unwrap();
    item.profile = next.clone().unwrap();
    item.attempt += 1;
    Ok(next)
}

pub fn resume(record: &TaskRecord) -> io::Result<()> {
    if record.stopped {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stopped task stays stopped after restart",
        ));
    }
    task_store::validate_purpose(record)?;
    Ok(())
}

/// Seeds one assignment that continues the owning task's exact purpose. The
/// purpose is copied from the record, never inferred.
pub fn seed_assignment(
    id: &str,
    profile: &str,
    thread: &str,
    worktree: &Path,
    purpose: Option<&str>,
) -> Assignment {
    task_store::seed_assignment(id, profile, thread, worktree, purpose)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_worktree::Mapping;
    use std::fs;
    use std::path::PathBuf;

    fn mapping(thread: &str, path: &str) -> Mapping {
        Mapping {
            schema: 1,
            path: PathBuf::from(path),
            source: PathBuf::from(r"D:\repo"),
            head: "abc".into(),
            owner_thread: Some(thread.into()),
            archived: false,
            unavailable: false,
        }
    }

    #[test]
    fn view_loss_suspends_without_replay() {
        let report = view_loss(&["exec-xai"]);
        assert_eq!(report["suspendDispatch"], true);
        assert_eq!(report["replay"], false);
        assert_eq!(report["closed"][0], "exec-xai");
    }

    #[test]
    fn acceptance_relation_follows_the_recorded_hierarchy() {
        validate_acceptance_relation("executor", "lead", Some("lead-a"), "lead-a").unwrap();
        validate_acceptance_relation("executor", "director", Some("director"), "director").unwrap();
        validate_acceptance_relation("lead", "director", Some("director"), "director").unwrap();
        assert!(
            validate_acceptance_relation("executor", "lead", Some("lead-b"), "lead-a")
                .unwrap_err()
                .to_string()
                .contains("not the accepting conversation")
        );
        assert!(
            validate_acceptance_relation("lead", "lead", Some("director"), "lead-a")
                .unwrap_err()
                .to_string()
                .contains("cannot accept another Lead")
        );
        assert!(validate_acceptance_relation("executor", "lead", None, "lead-a").is_err());
        assert!(
            validate_acceptance_relation("lead", "director", Some("lead-a"), "director")
                .unwrap_err()
                .to_string()
                .contains("not the accepting conversation")
        );
    }

    #[test]
    fn an_unfinished_parent_names_its_open_children() {
        let children = vec![
            json!({"id": "task-1", "status": "closed"}),
            json!({"id": "task-2", "status": "lead_review"}),
            json!({"id": "task-3", "status": "in_progress"}),
            json!({"id": "feedback-1", "status": "open", "labels": ["feedback"]}),
        ];
        assert_eq!(
            unfinished_children(&children),
            vec!["task-2".to_owned(), "task-3".to_owned()]
        );
        let error =
            refuse_unfinished_parent("feature-1", &["task-2".to_owned(), "task-3".to_owned()])
                .unwrap_err();
        assert!(error.to_string().contains("feature-1"), "{error}");
        assert!(error.to_string().contains("task-2"), "{error}");
        refuse_unfinished_parent("feature-1", &[]).unwrap();
    }

    #[test]
    fn revision_evidence_is_exact_clean_and_deduplicated() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        for repository in [&first, &second] {
            fs::create_dir_all(repository).unwrap();
            git(repository, &["init", "-q"]);
            git(repository, &["config", "user.email", "accept@example.test"]);
            git(repository, &["config", "user.name", "Accept"]);
            fs::write(repository.join("README.md"), "base\n").unwrap();
            git(repository, &["add", "README.md"]);
            git(repository, &["commit", "-qm", "base"]);
        }
        let first_revision = git_output(&first, &["rev-parse", "HEAD"]);
        let second_revision = git_output(&second, &["rev-parse", "HEAD"]);
        let evidence = repository_revisions(&[
            (first.clone(), first.clone()),
            (second.clone(), second.clone()),
        ])
        .unwrap();
        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0].revision, first_revision);
        assert_eq!(evidence[1].revision, second_revision);
        let duplicate = repository_revisions(&[
            (first.clone(), first.clone()),
            (first.clone(), first.clone()),
        ])
        .unwrap_err();
        assert!(
            duplicate.to_string().contains("more than once"),
            "{duplicate}"
        );
        fs::write(first.join("dirty.txt"), "uncommitted\n").unwrap();
        let dirty = repository_revisions(&[(first.clone(), first)]).unwrap_err();
        assert!(dirty.to_string().contains("not clean"), "{dirty}");
    }

    #[test]
    fn lead_merges_executor_commit_into_the_shared_checkout() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("repo");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "-q"]);
        git(&source, &["config", "user.email", "lead@example.test"]);
        git(&source, &["config", "user.name", "Lead"]);
        fs::write(source.join("README.md"), "base\n").unwrap();
        git(&source, &["add", "README.md"]);
        git(&source, &["commit", "-qm", "base"]);
        let tree = root.path().join("wt");
        git(
            &source,
            &[
                "worktree",
                "add",
                "--detach",
                tree.to_str().unwrap(),
                "HEAD",
            ],
        );
        fs::write(tree.join("done.txt"), "merged\n").unwrap();
        git(&tree, &["add", "done.txt"]);
        git(&tree, &["commit", "-qm", "executor work"]);
        let mapping = Mapping {
            schema: 1,
            path: tree.clone(),
            source: source.clone(),
            head: "HEAD".into(),
            owner_thread: Some("exec-xai".into()),
            archived: false,
            unavailable: false,
        };
        let mut record = TaskRecord {
            schema: 1,
            id: "t".into(),
            workspace: source.clone(),
            worktree: Some(tree.clone()),
            lead_profile: "default".into(),
            authorization: "full".into(),
            requirements: "done.txt exists".into(),
            stopped: false,
            active_lead: "default".into(),
            purpose: Some("ordinary".into()),
            assignments: vec![seed_assignment(
                "a1",
                "xai",
                "exec-xai",
                &tree,
                Some("ordinary"),
            )],
        };
        let disposition = merge_accepted(&mut record, "a1", &mapping, &source).unwrap();
        assert_eq!(record.assignments[0].accepted, Some(true));
        assert!(
            fs::read_to_string(source.join("done.txt"))
                .unwrap()
                .contains("merged")
        );
        let base = git_output(&source, &["rev-parse", "HEAD"]);
        assert_eq!(disposition, LaneDisposition::Reused { base: base.clone() });
        assert_eq!(git_output(&tree, &["rev-parse", "HEAD"]), base);
        assert!(
            git_output(
                &tree,
                &["status", "--porcelain=v1", "--untracked-files=all"]
            )
            .is_empty(),
            "the reused lane starts clean at the merged base"
        );
        assert!(tree.join("done.txt").exists(), "lane keeps the merged work");
    }

    fn git(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn git_output(cwd: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    #[test]
    fn merge_records_acceptance_and_preserves_ineligible_worktree() {
        let root = tempfile::tempdir().unwrap();
        let mut record = TaskRecord {
            schema: 1,
            id: "t".into(),
            workspace: root.path().to_path_buf(),
            worktree: None,
            lead_profile: "default".into(),
            authorization: "full".into(),
            requirements: "checks pass".into(),
            stopped: false,
            active_lead: "default".into(),
            purpose: Some("ordinary".into()),
            assignments: vec![seed_assignment(
                "a1",
                "xai",
                "exec-xai",
                &root.path().join("missing-tree"),
                Some("ordinary"),
            )],
        };
        let mapping = mapping(
            "exec-xai",
            root.path().join("missing-tree").to_str().unwrap(),
        );
        let retired = merge_accepted(&mut record, "a1", &mapping, root.path()).unwrap();
        assert_eq!(record.assignments[0].accepted, Some(true));
        match retired {
            LaneDisposition::Preserved { limitation } => assert!(!limitation.is_empty()),
            LaneDisposition::Reused { .. } => panic!("missing tree must be preserved"),
        }
    }

    #[test]
    fn acceptance_does_not_release_a_workspace_another_active_assignment_needs() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("missing-shared-tree");
        let mut record = TaskRecord {
            schema: 1,
            id: "t".into(),
            workspace: root.path().to_path_buf(),
            worktree: None,
            lead_profile: "default".into(),
            authorization: "full".into(),
            requirements: "checks pass".into(),
            stopped: false,
            active_lead: "default".into(),
            purpose: Some("ordinary".into()),
            assignments: vec![
                seed_assignment("accepted", "ds", "exec-a", &shared, Some("ordinary")),
                seed_assignment("active", "ds", "exec-b", &shared, Some("ordinary")),
            ],
        };
        let mapping = mapping("exec-a", shared.to_str().unwrap());
        let disposition = merge_accepted(&mut record, "accepted", &mapping, root.path()).unwrap();
        assert_eq!(record.assignments[0].accepted, Some(true));
        match disposition {
            LaneDisposition::Preserved { limitation } => {
                assert!(limitation.contains("assignment active"), "{limitation}");
                assert!(limitation.contains("was not released"), "{limitation}");
            }
            LaneDisposition::Reused { .. } => {
                panic!("a workspace needed by another active assignment must be preserved")
            }
        }
    }

    #[test]
    fn surviving_owner_is_not_replaced_and_uncertain_output_is_not_quota() {
        let record = TaskRecord {
            schema: 1,
            id: "t".into(),
            workspace: PathBuf::from("ws"),
            worktree: None,
            lead_profile: "default".into(),
            authorization: "full".into(),
            requirements: "synthetic".into(),
            stopped: false,
            active_lead: "default".into(),
            purpose: Some("ordinary".into()),
            assignments: vec![seed_assignment(
                "a1",
                "xai",
                "exec-xai",
                Path::new("wt"),
                Some("ordinary"),
            )],
        };
        let error = reconcile_before_replace(&record, "a1").unwrap_err();
        assert!(error.to_string().contains("still owns"));
        let mut next = record.clone();
        let error = reassign(
            &mut next,
            "a1",
            FailureCause::Unknown,
            &["xai".into(), "zai".into()],
        )
        .unwrap_err();
        assert!(error.to_string().contains("preserving work"));
        let profile = reassign(
            &mut next,
            "a1",
            FailureCause::Quota,
            &["xai".into(), "zai".into()],
        )
        .unwrap();
        assert_eq!(profile.as_deref(), Some("zai"));
        assert_eq!(next.assignments[0].attempt, 2);
        assert!(
            reassign(&mut next, "a1", FailureCause::Quota, &["zai".into()])
                .unwrap()
                .is_none()
        );
        next.stopped = true;
        assert!(
            resume(&next)
                .unwrap_err()
                .to_string()
                .contains("stays stopped")
        );
    }

    #[test]
    fn failure_matrix_preserves_spec_outcomes_without_live_accounts() {
        use crate::board_cli;
        use crate::task_failure::classify;
        let quota = classify(
            &serde_json::json!({"message":"limited","codexErrorInfo":"usageLimitExceeded"}),
        );
        let throttle =
            classify(&serde_json::json!({"message":"slow","codexErrorInfo":"rateLimitExceeded"}));
        let auth =
            classify(&serde_json::json!({"message":"denied","codexErrorInfo":"unauthorized"}));
        let transport = classify(
            &serde_json::json!({"message":"stream failed","codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":500}}}),
        );
        let incomplete = classify(
            &serde_json::json!({"message":"cut","codexErrorInfo":{"responseStreamDisconnected":{"httpStatusCode":null}}}),
        );
        assert_eq!(quota, FailureCause::Quota);
        assert_eq!(throttle, FailureCause::Throttle);
        assert_eq!(auth, FailureCause::Authentication);
        assert_eq!(transport, FailureCause::Transport);
        assert_eq!(incomplete, FailureCause::IncompleteOutput);
        let mut record = TaskRecord {
            schema: 1,
            id: "matrix".into(),
            workspace: PathBuf::from("ws"),
            worktree: None,
            lead_profile: "default".into(),
            authorization: "full".into(),
            requirements: "synthetic".into(),
            stopped: false,
            active_lead: "default".into(),
            purpose: Some("ordinary".into()),
            assignments: vec![seed_assignment(
                "lead",
                "default",
                "lead-1",
                Path::new("ws"),
                Some("ordinary"),
            )],
        };
        record.assignments.push(seed_assignment(
            "worker",
            "xai",
            "exec-xai",
            Path::new("wt"),
            Some("ordinary"),
        ));
        reconcile_before_replace(&record, "worker").unwrap_err();
        reassign(
            &mut record,
            "lead",
            quota,
            &["default".into(), "xai".into()],
        )
        .unwrap();
        reassign(
            &mut record,
            "worker",
            throttle,
            &["xai".into(), "zai".into()],
        )
        .unwrap();
        assert!(reassign(&mut record, "worker", transport, &["zai".into()]).is_err());
        assert!(reassign(&mut record, "worker", incomplete, &["zai".into()]).is_err());
        assert!(
            board_cli::unavailable("bd missing")
                .to_string()
                .contains("board unavailable")
        );
        assert!(
            reassign(&mut record, "worker", quota, &["zai".into()])
                .unwrap()
                .is_none()
        );
        record.stopped = true;
        assert!(resume(&record).is_err());
    }
}
