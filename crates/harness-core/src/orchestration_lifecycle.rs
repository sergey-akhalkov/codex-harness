//! Role configuration and private orchestration state across lifecycle verbs.
//! The check is the loop-delivery report a fresh external session's install
//! is verified against: configured roles and limits, the guidance skills the
//! workflow is discovered through, and the board tool when it is connected.
use crate::{orchestration_config, task_store};
use serde::Serialize;
use std::{fs, io, path::Path};

/// Skills that publish the loop's operating guidance to a fresh session.
pub const LOOP_SKILLS: [&str; 2] = ["team-lead", "board-workflow"];

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub status: &'static str,
    pub model_calls: u32,
    pub mutated: bool,
    pub lead_profile: Option<String>,
    pub successor_lead_profile: Option<String>,
    pub executor_profiles: Vec<String>,
    pub vote_threshold: Option<u32>,
    pub worktree_limit: Option<u32>,
    /// Loop guidance skills delivered into the user skill root.
    pub guidance: Vec<String>,
    /// Guidance a fresh session would not discover; `--core-only Install`
    /// delivers the kit's own skill links.
    pub guidance_missing: Vec<String>,
    /// Board tool version when the board component is connected.
    pub board_version: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PreservationReport {
    pub status: &'static str,
    pub model_calls: u32,
    pub mutated: bool,
    /// Private loop evidence this rollback left in place.
    pub preserved: Vec<&'static str>,
}

pub fn check(
    source: &Path,
    codex_home: &Path,
    user_home: &Path,
    preview: bool,
) -> io::Result<CheckReport> {
    orchestration_config::check_installation(source, codex_home)?;
    let config = match fs::read(source.join("global/orchestration.toml")) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
        Ok(bytes) => Some(orchestration_config::parse(&bytes)?),
    };
    let root = user_home.join(".agents/skills");
    let mut guidance = Vec::new();
    let mut guidance_missing = Vec::new();
    for name in LOOP_SKILLS {
        if root.join(name).join("SKILL.md").is_file() {
            guidance.push(name.to_owned());
        } else {
            guidance_missing.push(name.to_owned());
        }
    }
    Ok(CheckReport {
        status: if preview {
            "Preview orchestration Check"
        } else if config.is_some() {
            "Orchestration connected"
        } else {
            "Orchestration not configured"
        },
        model_calls: 0,
        mutated: false,
        lead_profile: config.as_ref().map(|value| value.lead_profile.clone()),
        successor_lead_profile: config
            .as_ref()
            .map(|value| value.successor_lead_profile.clone()),
        executor_profiles: config
            .as_ref()
            .map(|value| value.executor_profiles.clone())
            .unwrap_or_default(),
        vote_threshold: config.as_ref().map(|value| value.vote_threshold),
        worktree_limit: config.as_ref().map(|value| value.worktree_limit),
        guidance,
        guidance_missing,
        board_version: board_version(codex_home)?,
    })
}

/// The board is a separate lifecycle component; its absence is reported and
/// left to `--board-only`, never substituted or invented here.
fn board_version(codex_home: &Path) -> io::Result<Option<String>> {
    let path = codex_home.join("harness/board.json");
    let bytes = match fs::read(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(bytes) => bytes,
    };
    let state: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::other("board state is not JSON; preserving it"))?;
    if state["enabled"] != true {
        return Ok(None);
    }
    Ok(state["bdVersion"].as_str().map(str::to_owned))
}

/// Rollback removes the loop's installed pieces; its private evidence stays.
/// A missing optional root is not an error, but an unreadable one is: the
/// receipt must not claim preservation it did not observe.
pub fn disconnect_preserves_private(
    codex_home: &Path,
    user_home: &Path,
) -> io::Result<PreservationReport> {
    let store = task_store::store_dir(codex_home);
    let worktrees = codex_home.join("worktrees");
    let mut preserved = Vec::new();
    for (path, name) in [
        (&store, "task-store"),
        (&worktrees, "worktrees"),
        (&user_home.join(".agents/skills"), "skills"),
    ] {
        match fs::metadata(path) {
            Ok(_) => preserved.push(name),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(PreservationReport {
        status: "Orchestration private state preserved",
        model_calls: 0,
        mutated: false,
        preserved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_store::{self, Assignment, TaskRecord};
    use std::path::{Path, PathBuf};

    fn kit_source(root: &Path) -> PathBuf {
        let source = root.join("source");
        fs::create_dir_all(source.join("global")).unwrap();
        fs::write(
            source.join("global/orchestration.toml"),
            include_bytes!("../../../global/orchestration.toml"),
        )
        .unwrap();
        source
    }

    fn installed_profiles(home: &Path) {
        fs::create_dir_all(home).unwrap();
        fs::write(home.join("xai.config.toml"), "model = 'grok-4.7'\n").unwrap();
        fs::write(home.join("zai.config.toml"), "model = 'glm-5.3'\n").unwrap();
    }

    fn guidance(user: &Path, names: &[&str]) {
        for name in names {
            let dir = user.join(".agents/skills").join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: Loop guidance.\n---\n"),
            )
            .unwrap();
        }
    }

    #[test]
    fn preview_check_does_not_write_and_disconnect_keeps_stopped_checkpoints() {
        let root = tempfile::tempdir().unwrap();
        let source = kit_source(root.path());
        let home = root.path().join("home");
        let user = root.path().join("user");
        installed_profiles(&home);
        fs::create_dir_all(home.join("harness/bin")).unwrap();
        guidance(&user, &["team-lead", "board-workflow"]);
        let before: Vec<_> = walkdir(&home);
        let preview = check(&source, &home, &user, true).unwrap();
        assert!(!preview.mutated);
        assert_eq!(preview.status, "Preview orchestration Check");
        assert_eq!(walkdir(&home), before);
        let connected = check(&source, &home, &user, false).unwrap();
        assert_eq!(connected.status, "Orchestration connected");
        assert_eq!(connected.executor_profiles, ["xai"]);
        assert_eq!(connected.vote_threshold, Some(3));
        assert_eq!(connected.guidance, ["team-lead", "board-workflow"]);
        assert!(connected.guidance_missing.is_empty());
        assert_eq!(connected.board_version, None);
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
        let preserved = disconnect_preserves_private(&home, &user).unwrap();
        assert!(preserved.preserved.contains(&"task-store"));
        assert!(preserved.preserved.contains(&"worktrees"));
        assert!(preserved.preserved.contains(&"skills"));
        record = task_store::load(&home, "live").unwrap();
        assert!(record.stopped);
        assert_eq!(
            fs::read(home.join("worktrees/exec/repo/partial.txt")).unwrap(),
            b"keep"
        );
        assert!(home.join("xai.config.toml").is_file());
    }

    #[test]
    fn check_reports_guidance_gaps_and_the_connected_board() {
        let root = tempfile::tempdir().unwrap();
        let source = kit_source(root.path());
        let home = root.path().join("home");
        let user = root.path().join("user");
        installed_profiles(&home);
        guidance(&user, &["team-lead"]);
        fs::create_dir_all(home.join("harness")).unwrap();
        fs::write(
            home.join("harness/board.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "enabled": true,
                "bdVersion": crate::board_cli::VERSION,
                "links": []
            }))
            .unwrap(),
        )
        .unwrap();
        let report = check(&source, &home, &user, false).unwrap();
        assert_eq!(report.guidance, ["team-lead"]);
        assert_eq!(report.guidance_missing, ["board-workflow"]);
        assert_eq!(
            report.board_version.as_deref(),
            Some(crate::board_cli::VERSION)
        );
        assert_eq!(report.model_calls, 0);
        assert!(!report.mutated);
    }

    #[test]
    fn check_fails_explicitly_for_a_missing_profile_and_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let source = kit_source(root.path());
        let home = root.path().join("home");
        let user = root.path().join("user");
        installed_profiles(&home);
        fs::remove_file(home.join("zai.config.toml")).unwrap();
        let before = fs::read(home.join("xai.config.toml")).unwrap();
        let error = check(&source, &home, &user, false).unwrap_err();
        assert!(error.to_string().contains("profile 'zai' is not installed"));
        assert_eq!(fs::read(home.join("xai.config.toml")).unwrap(), before);
        assert!(!user.exists());
    }

    #[test]
    fn this_checkout_delivers_the_loop_guidance_skills() {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in LOOP_SKILLS {
            let skill = checkout.join(".agents/skills").join(name).join("SKILL.md");
            let text = fs::read_to_string(&skill)
                .unwrap_or_else(|error| panic!("{}: {error}", skill.display()));
            assert!(text.contains(&format!("name: {name}")), "{name}");
        }
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
