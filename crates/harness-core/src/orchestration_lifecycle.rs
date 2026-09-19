//! Role configuration and private orchestration state across lifecycle verbs.
use crate::{orchestration_config, task_store};
use serde::Serialize;
use std::{fs, io, path::Path};

#[derive(Debug, Serialize)]
pub struct Report {
    pub status: &'static str,
    pub model_calls: u32,
    pub mutated: bool,
}

pub fn check(source: &Path, codex_home: &Path, preview: bool) -> io::Result<Report> {
    orchestration_config::check_installation(source, codex_home)?;
    Ok(Report {
        status: if preview {
            "Preview orchestration Check"
        } else {
            "Orchestration connected"
        },
        model_calls: 0,
        mutated: false,
    })
}

pub fn disconnect_preserves_private(codex_home: &Path) -> io::Result<Report> {
    let store = task_store::store_dir(codex_home);
    let worktrees = codex_home.join("worktrees");
    for path in [&store, &worktrees] {
        if path.is_dir() {
            fs::metadata(path)?;
        }
    }
    Ok(Report {
        status: "Orchestration private state preserved",
        model_calls: 0,
        mutated: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_store::{self, Assignment, TaskRecord};
    use std::path::PathBuf;

    #[test]
    fn preview_check_does_not_write_and_disconnect_keeps_stopped_checkpoints() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let home = root.path().join("home");
        fs::create_dir_all(source.join("global")).unwrap();
        fs::create_dir_all(home.join("harness/bin")).unwrap();
        fs::write(
            source.join("global/orchestration.toml"),
            include_bytes!("../../../global/orchestration.toml"),
        )
        .unwrap();
        fs::write(home.join("ds.config.toml"), "model = 'deepseek-flash'\n").unwrap();
        let before: Vec<_> = walkdir(&home);
        let preview = check(&source, &home, true).unwrap();
        assert!(!preview.mutated);
        assert_eq!(preview.status, "Preview orchestration Check");
        assert_eq!(walkdir(&home), before);
        check(&source, &home, false).unwrap();
        check(&source, &home, false).unwrap();
        let mut record = TaskRecord {
            schema: 1,
            id: "live".into(),
            workspace: PathBuf::from("ws"),
            worktree: Some(home.join("worktrees/exec/repo")),
            lead_profile: "default".into(),
            authorization: "full".into(),
            requirements: "synthetic".into(),
            stopped: true,
            active_lead: "default".into(),
            assignments: vec![Assignment {
                id: "a1".into(),
                profile: "ds".into(),
                owner_thread: Some("t1".into()),
                worktree: Some(home.join("worktrees/exec/repo")),
                attempt: 1,
                accepted: None,
                defect: None,
            }],
        };
        fs::create_dir_all(home.join("worktrees/exec/repo")).unwrap();
        fs::write(home.join("worktrees/exec/repo/partial.txt"), b"keep").unwrap();
        task_store::save(&home, &record).unwrap();
        disconnect_preserves_private(&home).unwrap();
        record = task_store::load(&home, "live").unwrap();
        assert!(record.stopped);
        assert_eq!(
            fs::read(home.join("worktrees/exec/repo/partial.txt")).unwrap(),
            b"keep"
        );
        assert!(home.join("ds.config.toml").is_file());
    }

    fn walkdir(root: &Path) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        names
    }
}
