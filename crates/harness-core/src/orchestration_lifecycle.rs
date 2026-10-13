//! Role configuration and private orchestration state across lifecycle verbs.
//! The check is the loop-delivery report a fresh external session's install
//! is verified against: configured roles and limits, the guidance skills the
//! workflow is discovered through, and the board tool when it is connected.
use crate::role_context::Role;
use crate::{orchestration_config, role_context, task_store};
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
    pub max_concurrent_executors: Option<u32>,
    pub worktree_pool_size: Option<u32>,
    pub vote_threshold: Option<u32>,
    pub worktree_limit: Option<u32>,
    /// Responsibility roles resolved from the kit checkout's owned sources.
    pub roles: Vec<RoleState>,
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

/// One responsibility role in the lifecycle report. A gap names its exact
/// cause and fails nothing else: the check reports what the installation
/// would deliver, and a missing role input is a delivery gap for its owner,
/// not a reason to withhold the other facts. Successive dispatches resolve
/// the then-current sources; a resolved revision here is this checkout's, not
/// a retroactive claim about earlier sessions.
#[derive(Debug, Serialize)]
pub struct RoleState {
    pub role: &'static str,
    /// `resolved` when the role's owned inputs all read, `gap` otherwise.
    pub status: &'static str,
    /// The owned revision of exactly the inputs this role resolved.
    pub revision: Option<String>,
    /// The exact cause when the role could not resolve.
    pub cause: Option<String>,
}

/// Resolve both roles read-only. A missing role source or manifest is
/// reported with the input it names; it never fails this check.
fn role_states(source: &Path) -> Vec<RoleState> {
    [Role::Lead, Role::Executor]
        .into_iter()
        .map(|role| match role_context::resolve(source, role) {
            Ok(resolved) => RoleState {
                role: role.label(),
                status: "resolved",
                revision: Some(resolved.revision),
                cause: None,
            },
            Err(error) => RoleState {
                role: role.label(),
                status: "gap",
                revision: None,
                cause: Some(error.to_string()),
            },
        })
        .collect()
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
        max_concurrent_executors: config.as_ref().map(|value| value.max_concurrent_executors),
        worktree_pool_size: config
            .as_ref()
            .map(orchestration_config::worktree_pool_size),
        vote_threshold: config.as_ref().map(|value| value.vote_threshold),
        worktree_limit: config.as_ref().map(|value| value.worktree_limit),
        roles: role_states(source),
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
    let pool = codex_home.join("harness/executor-pool");
    let mut preserved = Vec::new();
    for (path, name) in [
        (&store, "task-store"),
        (&worktrees, "worktrees"),
        (&pool, "executor-pool"),
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

/// Materialize the explicit `worktree_pool_size` setting for a configuration
/// written before the capacity split, so install and update preserve the
/// former executor-derived pool capacity without selecting the compatibility
/// `worktree_limit`. Only the missing setting is added: the executor limit,
/// every other user choice, comments and layout stay untouched. An explicit
/// setting is never rewritten, and a file this build cannot parse or an
/// effective value outside the supported pool range is left as it is for the
/// installation check to report. Returns the capacity written, if any.
pub fn materialize_pool_capacity(source: &Path) -> io::Result<Option<u32>> {
    let path = source.join("global/orchestration.toml");
    let bytes = match fs::read(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(bytes) => bytes,
    };
    let Ok(config) = orchestration_config::parse(&bytes) else {
        return Ok(None);
    };
    if config.worktree_pool_size.is_some() {
        return Ok(None);
    }
    let capacity = config.max_concurrent_executors;
    if capacity == 0 || capacity > crate::task_worktree::MAX_POOL_SLOTS {
        return Ok(None);
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let Some(candidate) = materialized_capacity_text(&text, capacity) else {
        return Ok(None);
    };
    stage_preserving(&path, candidate.as_bytes())?;
    Ok(Some(capacity))
}

/// Candidate texts that add the explicit setting with the least disturbance:
/// directly after the executor limit it derives from, else at the end of the
/// file. A candidate is used only after re-parsing to exactly the original
/// configuration plus the new setting.
fn materialized_capacity_text(text: &str, capacity: u32) -> Option<String> {
    let original = orchestration_config::parse(text.as_bytes()).ok()?;
    let mut expected = original;
    expected.worktree_pool_size = Some(capacity);
    let mut candidates = Vec::new();
    if let Some(inserted) = insert_after_executor_limit(text, capacity) {
        candidates.push(inserted);
    }
    candidates.push(append_pool_setting(text, capacity));
    candidates.into_iter().find(|candidate| {
        orchestration_config::parse(candidate.as_bytes())
            .map(|parsed| parsed == expected)
            .unwrap_or(false)
    })
}

fn insert_after_executor_limit(text: &str, capacity: u32) -> Option<String> {
    let mut candidate = String::with_capacity(text.len() + 256);
    let mut inserted = false;
    for line in text.split_inclusive('\n') {
        candidate.push_str(line);
        let is_executor_limit = !inserted
            && line
                .trim_start()
                .strip_prefix("max_concurrent_executors")
                .is_some_and(|rest| rest.trim_start().starts_with('='));
        if is_executor_limit {
            let ending = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
            if !line.ends_with('\n') {
                candidate.push_str(ending);
            }
            candidate.push_str(&pool_setting_block(capacity, ending));
            inserted = true;
        }
    }
    inserted.then_some(candidate)
}

fn append_pool_setting(text: &str, capacity: u32) -> String {
    let ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut candidate = text.to_owned();
    if !candidate.is_empty() && !candidate.ends_with('\n') {
        candidate.push_str(ending);
    }
    candidate.push_str(&format!("worktree_pool_size = {capacity}{ending}"));
    candidate
}

fn pool_setting_block(capacity: u32, ending: &str) -> String {
    let mut block = String::new();
    for comment in [
        "# Managed worktree capacity, materialized by installation from the former",
        "# executor-derived value; it no longer follows max_concurrent_executors.",
    ] {
        block.push_str(comment);
        block.push_str(ending);
    }
    block.push_str(&format!("worktree_pool_size = {capacity}"));
    block.push_str(ending);
    block
}

/// Replace one file through same-directory staged bytes so an interrupted
/// write cannot truncate the user's configuration.
fn stage_preserving(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("orchestration.toml");
    let staged = directory.join(format!("{name}.materialize-{}.tmp", std::process::id()));
    fs::write(&staged, bytes)?;
    match fs::rename(&staged, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&staged);
            Err(error)
        }
    }
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
        fs::write(home.join("ds.config.toml"), "model = 'deepseek-flash'\n").unwrap();
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
        let config_path = source.join("global/orchestration.toml");
        let mut config: toml::Table = fs::read_to_string(&config_path).unwrap().parse().unwrap();
        config.insert("max_concurrent_executors".into(), 2.into());
        config.insert("worktree_pool_size".into(), 4.into());
        fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
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
        assert_eq!(connected.executor_profiles, ["ds"]);
        assert_eq!(connected.max_concurrent_executors, Some(2));
        assert_eq!(connected.worktree_pool_size, Some(4));
        assert_eq!(connected.vote_threshold, Some(3));
        assert_eq!(connected.guidance, ["team-lead", "board-workflow"]);
        assert!(connected.guidance_missing.is_empty());
        assert_eq!(connected.board_version, None);
        config.remove("worktree_pool_size");
        fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
        let legacy = check(&source, &home, &user, true).unwrap();
        assert_eq!(legacy.worktree_pool_size, Some(2));
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
        fs::write(home.join("worktrees/exec/repo/partial.txt"), b"keep\n").unwrap();
        task_store::save(&home, &record).unwrap();
        let preserved = disconnect_preserves_private(&home, &user).unwrap();
        assert!(preserved.preserved.contains(&"task-store"));
        assert!(preserved.preserved.contains(&"worktrees"));
        assert!(preserved.preserved.contains(&"skills"));
        record = task_store::load(&home, "live").unwrap();
        assert!(record.stopped);
        assert_eq!(
            fs::read(home.join("worktrees/exec/repo/partial.txt")).unwrap(),
            b"keep\n"
        );
        assert!(home.join("ds.config.toml").is_file());
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
        fs::remove_file(home.join("ds.config.toml")).unwrap();
        let before = fs::read(home.join("zai.config.toml")).unwrap();
        let error = check(&source, &home, &user, false).unwrap_err();
        assert!(error.to_string().contains("profile 'ds' is not installed"));
        assert_eq!(fs::read(home.join("zai.config.toml")).unwrap(), before);
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

    fn role_kit(root: &Path) -> PathBuf {
        let source = root.join("source");
        for dir in [
            source.join("global"),
            source.join("skills/shared"),
            source.join("skills/coordinate"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(
            source.join("global/kit.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema": 1,
                "profile_name": "harness",
                "profile": "global/profile.toml",
                "instructions": "global/instructions.md",
                "skills": "skills",
                "agents": "global/agents",
                "hooks": "global/hooks.json",
                "token_hooks": "global/token-hooks.json"
            }))
            .unwrap(),
        )
        .unwrap();
        for (path, body) in [
            (
                "global/profile.toml",
                "developer_instructions = 'Fixture shared developer'\n",
            ),
            ("global/instructions.md", "Fixture common safeguards"),
            ("global/lead.md", "Fixture lead responsibility"),
            ("global/executor.md", "Fixture executor responsibility"),
            (
                "skills/shared/SKILL.md",
                "---\nname: shared\ndescription: fixture\n---\nFixture shared skill",
            ),
            (
                "skills/coordinate/SKILL.md",
                "---\nname: coordinate\ndescription: fixture\n---\nFixture lead skill",
            ),
            (
                "global/roles.toml",
                "schema = 1\nshared_skills = ['shared']\n[roles.lead]\ninstructions = 'global/lead.md'\nskills = ['coordinate']\n[roles.executor]\ninstructions = 'global/executor.md'\nskills = []\n",
            ),
        ] {
            fs::write(source.join(path), body).unwrap();
        }
        source
    }

    #[test]
    fn a_gap_role_reports_the_missing_input_without_failing_the_check() {
        let root = tempfile::tempdir().unwrap();
        let source = kit_source(root.path());
        let home = root.path().join("home");
        let user = root.path().join("user");
        installed_profiles(&home);
        let report = check(&source, &home, &user, true).unwrap();
        assert_eq!(
            report
                .roles
                .iter()
                .map(|state| (state.role, state.status))
                .collect::<Vec<_>>(),
            [("lead", "gap"), ("executor", "gap")]
        );
        for state in &report.roles {
            assert!(state.revision.is_none(), "{}", state.role);
            let cause = state.cause.as_deref().unwrap();
            assert!(cause.contains("global/kit.json"), "{}: {cause}", state.role);
        }
        assert_eq!(report.model_calls, 0);
        assert!(!report.mutated);
    }

    #[test]
    fn role_states_carry_each_role_owned_revision() {
        let root = tempfile::tempdir().unwrap();
        let source = role_kit(root.path());
        let states = role_states(&source);
        assert_eq!(
            states
                .iter()
                .map(|state| (state.role, state.status))
                .collect::<Vec<_>>(),
            [("lead", "resolved"), ("executor", "resolved")]
        );
        let lead = states[0].revision.clone().unwrap();
        let executor = states[1].revision.clone().unwrap();
        assert!(!lead.is_empty());
        assert!(!executor.is_empty());
        assert_ne!(lead, executor);
        assert!(states.iter().all(|state| state.cause.is_none()));
    }

    fn legacy_text(executor_limit: u32) -> String {
        format!(
            "schema = 1\n# user comment retained\nlead_profile = \"default\"\n\
             successor_lead_profile = \"default\"\nexecutor_profiles = [\"ds\"]\n\
             max_concurrent_executors = {executor_limit}\nvote_threshold = 3\n\
             incubator_size_cap = 32\nfeedback_batch_limit = 8\nworktree_limit = 6\n"
        )
    }

    #[test]
    fn installation_materializes_legacy_pool_capacity_without_touching_choices() {
        let root = tempfile::tempdir().unwrap();
        let source = kit_source(root.path());
        let path = source.join("global/orchestration.toml");
        fs::write(&path, legacy_text(2)).unwrap();
        assert_eq!(materialize_pool_capacity(&source).unwrap(), Some(2));
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("# user comment retained"), "{written}");
        let config = orchestration_config::parse(written.as_bytes()).unwrap();
        assert_eq!(config.worktree_pool_size, Some(2));
        assert_eq!(orchestration_config::worktree_pool_size(&config), 2);
        assert_eq!(config.max_concurrent_executors, 2);
        assert_eq!(
            config.worktree_limit, 6,
            "the compatibility worktree_limit is never selected as capacity"
        );
        assert_eq!(materialize_pool_capacity(&source).unwrap(), None);
        assert_eq!(fs::read_to_string(&path).unwrap(), written);
    }

    #[test]
    fn explicit_or_unreadable_capacity_configuration_is_left_as_it_is() {
        let root = tempfile::tempdir().unwrap();
        let source = kit_source(root.path());
        let path = source.join("global/orchestration.toml");
        let shipped = fs::read(&path).unwrap();
        assert_eq!(materialize_pool_capacity(&source).unwrap(), None);
        assert_eq!(fs::read(&path).unwrap(), shipped);
        for invalid in [0, 33] {
            let text = legacy_text(invalid);
            fs::write(&path, &text).unwrap();
            assert_eq!(materialize_pool_capacity(&source).unwrap(), None);
            assert_eq!(fs::read(&path).unwrap(), text.as_bytes());
        }
        fs::write(&path, b"not toml [").unwrap();
        assert_eq!(materialize_pool_capacity(&source).unwrap(), None);
        assert_eq!(fs::read(&path).unwrap(), b"not toml [");
        let empty = root.path().join("without-configuration");
        fs::create_dir_all(&empty).unwrap();
        assert_eq!(materialize_pool_capacity(&empty).unwrap(), None);
    }

    #[test]
    fn materialization_handles_crlf_and_a_missing_trailing_newline() {
        let root = tempfile::tempdir().unwrap();
        let source = kit_source(root.path());
        let path = source.join("global/orchestration.toml");
        fs::write(&path, legacy_text(3).replace('\n', "\r\n")).unwrap();
        assert_eq!(materialize_pool_capacity(&source).unwrap(), Some(3));
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("worktree_pool_size = 3\r\n"), "{written}");
        assert_eq!(
            orchestration_config::parse(written.as_bytes())
                .unwrap()
                .worktree_pool_size,
            Some(3)
        );
        fs::write(&path, legacy_text(4).trim_end_matches('\n')).unwrap();
        assert_eq!(materialize_pool_capacity(&source).unwrap(), Some(4));
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(
            orchestration_config::parse(written.as_bytes())
                .unwrap()
                .worktree_pool_size,
            Some(4)
        );
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
