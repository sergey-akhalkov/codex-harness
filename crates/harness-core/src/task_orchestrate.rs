//! Lead/executor dispatch, steering, merge, recovery and stop.
use crate::{
    orchestration_config::Orchestration,
    task_failure::FailureCause,
    task_store::{self, Assignment, TaskRecord},
    task_worktree::{self, LaneDisposition, Mapping},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{io, path::Path, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutorView {
    pub profile: String,
    pub thread_id: String,
    pub slot: usize,
    pub bounds: Bounds,
    pub mapping: Mapping,
    pub remote_args: Vec<String>,
}

pub fn plan_executors(
    config: &Orchestration,
    mappings: Vec<(String, Mapping)>,
) -> io::Result<Vec<ExecutorView>> {
    if mappings.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "two executors are required for simultaneous dispatch",
        ));
    }
    if mappings.len() as u32 > config.max_concurrent_executors {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "executor concurrency exceeds configuration",
        ));
    }
    let layout: Vec<Bounds> = (0..=mappings.len())
        .map(|slot| Bounds {
            x: (slot as i32) * 40,
            y: 0,
            width: 400,
            height: 300,
        })
        .collect();
    let mut views = Vec::new();
    for (slot, (profile, mapping)) in mappings.into_iter().enumerate() {
        if !config.executor_profiles.iter().any(|name| name == &profile) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("orchestration profile '{profile}' is not an executor"),
            ));
        }
        let thread_id = mapping
            .owner_thread
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "executor thread missing"))?;
        let remote = task_worktree::remote_tui_args(
            &mapping,
            &[
                "--remote".into(),
                "ws://127.0.0.1:1".into(),
                "resume".into(),
            ],
        )?;
        views.push(ExecutorView {
            profile,
            thread_id,
            slot: slot + 1,
            bounds: layout.get(slot + 1).copied().unwrap_or(Bounds {
                x: ((slot as i32) + 1) * 40,
                y: 0,
                width: 400,
                height: 300,
            }),
            mapping,
            remote_args: remote
                .into_iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
        });
    }
    let paths: Vec<_> = views.iter().map(|view| &view.mapping.path).collect();
    if paths[0] == paths[1] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "executors must use distinct worktrees",
        ));
    }
    if views[0].thread_id == views[1].thread_id || views[0].profile == views[1].profile {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "executors must have distinct live identities",
        ));
    }
    Ok(views)
}

pub fn steer(thread_id: &str, text: &str, worktree: &Path) -> Value {
    json!({
        "schema": 1,
        "method": "turn/start",
        "hiddenModelCall": false,
        "statusPoll": false,
        "worktree": worktree,
        "params": {
            "threadId": thread_id,
            "input": [{"type": "text", "text": text}]
        }
    })
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
    Ok(())
}

pub fn seed_assignment(id: &str, profile: &str, thread: &str, worktree: &Path) -> Assignment {
    Assignment {
        id: id.into(),
        profile: profile.into(),
        owner_thread: Some(thread.into()),
        worktree: Some(worktree.to_path_buf()),
        attempt: 1,
        accepted: None,
        defect: None,
    }
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
            remote_tui_omits_worktree_flag: true,
        }
    }

    fn config() -> Orchestration {
        Orchestration {
            schema: 1,
            lead_profile: "default".into(),
            successor_lead_profile: "xai".into(),
            executor_profiles: vec!["xai".into(), "zai".into()],
            max_concurrent_executors: 2,
            vote_threshold: 3,
            incubator_size_cap: 32,
            feedback_batch_limit: 8,
            worktree_limit: 6,
        }
    }

    #[test]
    fn two_executors_get_distinct_windows_worktrees_and_identities() {
        let views = plan_executors(
            &config(),
            vec![
                ("xai".into(), mapping("exec-xai", r"D:\wt\xai")),
                ("zai".into(), mapping("exec-zai", r"D:\wt\zai")),
            ],
        )
        .unwrap();
        assert_eq!(views.len(), 2);
        assert_ne!(views[0].bounds, views[1].bounds);
        assert_ne!(views[0].mapping.path, views[1].mapping.path);
        assert_ne!(views[0].profile, views[1].profile);
        assert_ne!(views[0].thread_id, views[1].thread_id);
        assert!(views[0].remote_args.contains(&"-C".into()));
        assert!(
            !views
                .iter()
                .any(|view| view.remote_args.iter().any(|arg| arg == "--worktree"))
        );
    }

    #[test]
    fn steering_is_visible_turn_start_without_polling() {
        let payload = steer("exec-xai", "use the fixture path", Path::new(r"D:\wt\xai"));
        assert_eq!(payload["method"], "turn/start");
        assert_eq!(payload["hiddenModelCall"], false);
        assert_eq!(payload["statusPoll"], false);
        assert_eq!(payload["params"]["threadId"], "exec-xai");
        assert_eq!(payload["worktree"], r"D:\wt\xai");
    }

    #[test]
    fn view_loss_suspends_without_replay() {
        let report = view_loss(&["exec-xai"]);
        assert_eq!(report["suspendDispatch"], true);
        assert_eq!(report["replay"], false);
        assert_eq!(report["closed"][0], "exec-xai");
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
            remote_tui_omits_worktree_flag: true,
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
            assignments: vec![seed_assignment("a1", "xai", "exec-xai", &tree)],
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
            assignments: vec![seed_assignment(
                "a1",
                "xai",
                "exec-xai",
                &root.path().join("missing-tree"),
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
            assignments: vec![seed_assignment("a1", "xai", "exec-xai", Path::new("wt"))],
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
            assignments: vec![seed_assignment(
                "lead",
                "default",
                "lead-1",
                Path::new("ws"),
            )],
        };
        record.assignments.push(seed_assignment(
            "worker",
            "xai",
            "exec-xai",
            Path::new("wt"),
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
