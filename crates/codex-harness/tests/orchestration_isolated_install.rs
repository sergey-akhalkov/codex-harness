//! Isolated core/board install from this checkout, then an ordinary Codex
//! start outside the checkout. Does not touch the live user home. The loop
//! acceptance is included: guidance skills, orchestration configuration, the
//! board tool with real board evidence, and rollback that removes the loop
//! without losing unrelated configuration or archived evidence.
#![cfg(windows)]

use harness_core::{
    board_lifecycle, build_identity, core_disconnect, core_install, installation_state::PathScope,
    orchestration_lifecycle,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; isolated homes only"]
fn isolated_core_and_board_install_loads_orchestration_outside_checkout() {
    let source = disk_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap(),
    );
    let upstream = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    let root = tempfile::Builder::new()
        .prefix("osa-isolated-")
        .tempdir()
        .unwrap();
    let root_path = disk_path(root.path());
    eprintln!("isolated orchestration evidence: {}", root_path.display());
    let build = root_path.join("build");
    let codex_home = root_path.join("codex");
    let user_home = root_path.join("user");
    let workspace = root_path.join("outside");
    fs::create_dir_all(&build).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    for name in build_identity::BINARIES {
        let from = Path::new(env!("CARGO_BIN_EXE_codex")).with_file_name(name);
        fs::copy(&from, build.join(name))
            .unwrap_or_else(|error| panic!("copy {name} from {}: {error}", from.display()));
    }
    let record = build_identity::BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.clone(),
        source: build_identity::source_identity(&source).unwrap(),
        rustc: "isolated orchestration install".into(),
        cargo: "isolated orchestration install".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries: build_identity::BINARIES
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    build_identity::hash_file(&build.join(name)).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    };
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();

    let request = core_install::Request {
        source: source.clone(),
        build,
        codex_home: codex_home.clone(),
        user_home: user_home.clone(),
        dependency_user_home: user_home.clone(),
        upstream: Some(upstream.clone()),
        timeout: Duration::from_secs(45),
        path_scope: Some(PathScope::Process),
    };
    let _path = PathRestore::capture();
    let preview = core_install::connect(&request, true).unwrap();
    assert_eq!(preview.status, "preview");
    assert!(preview.runtime.is_none());
    assert!(!user_home.join(".agents/skills/team-lead").exists());
    assert!(!codex_home.join("AGENTS.md").exists());

    // Unrelated configuration a fresh install must preserve.
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(codex_home.join("auth.json"), b"unrelated-auth").unwrap();
    fs::write(
        codex_home.join("config.toml"),
        b"model = 'unrelated-kept'\n",
    )
    .unwrap();
    let foreign = user_home.join(".agents/skills/foreign/SKILL.md");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::write(&foreign, b"---\nname: foreign\ndescription: Kept.\n---\n").unwrap();

    let report = core_install::connect(&request, false).unwrap();
    assert_eq!(report.status, "connected");
    assert!(report.runtime.unwrap().passed);
    assert_eq!(
        fs::read_link(codex_home.join("AGENTS.md")).unwrap(),
        source.join("global/principles-of-work.md")
    );
    assert_eq!(
        fs::read_link(user_home.join(".agents/skills/team-lead")).unwrap(),
        source.join(".agents/skills/team-lead")
    );
    assert_eq!(
        fs::read_link(user_home.join(".agents/skills/board-workflow")).unwrap(),
        source.join(".agents/skills/board-workflow")
    );
    let skill = fs::read_to_string(user_home.join(".agents/skills/team-lead/SKILL.md")).unwrap();
    assert!(skill.contains("name: team-lead"));
    // A fresh session discovers the delivered workflow, not a stub: the
    // guidance it loads names the loop's board and pacing contract.
    let board_skill =
        fs::read_to_string(user_home.join(".agents/skills/board-workflow/SKILL.md")).unwrap();
    assert!(board_skill.contains("lead_review"), "{board_skill}");
    assert!(board_skill.contains("feedback-route v1"), "{board_skill}");
    assert!(skill.contains("vote_threshold"), "{skill}");
    assert!(skill.contains("board-workflow"), "{skill}");
    let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert!(
        config.contains("model = 'unrelated-kept'"),
        "unrelated configuration survives the core install: {config}"
    );
    assert_eq!(
        fs::read_to_string(&foreign).unwrap(),
        "---\nname: foreign\ndescription: Kept.\n---\n",
        "foreign skills are preserved"
    );

    // The orchestration configuration names machine-local profiles; kit
    // install does not create them, so the isolated home supplies the same
    // profile files a real installation would have.
    fs::write(
        codex_home.join("ds.config.toml"),
        "model = 'deepseek-flash'\n",
    )
    .unwrap();
    fs::write(codex_home.join("zai.config.toml"), "model = 'glm-5.3'\n").unwrap();
    let delivery = orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap();
    eprintln!("loop delivery report: {delivery:?}");
    assert_eq!(delivery.status, "Orchestration connected");
    assert_eq!(delivery.guidance, ["team-lead", "board-workflow"]);
    assert!(delivery.guidance_missing.is_empty());
    assert_eq!(delivery.executor_profiles, ["ds"]);
    assert_eq!(
        delivery.board_version, None,
        "the board is a separate component"
    );
    assert_eq!(delivery.model_calls, 0);
    assert!(!delivery.mutated);

    let board = board_lifecycle::Request {
        source: source.clone(),
        codex_home: codex_home.clone(),
        user_home: user_home.clone(),
        preview: true,
    };
    let board_preview = board_lifecycle::install(&board).unwrap();
    assert_eq!(board_preview.status, "Preview board Install");
    assert!(!codex_home.join("harness/bin/bd.exe").exists());
    let board_on = board_lifecycle::Request {
        preview: false,
        ..board
    };
    let board_report = board_lifecycle::install(&board_on).unwrap();
    assert_eq!(board_report.status, "Board connected");
    assert!(codex_home.join("harness/bin/bd.exe").is_file());
    let connected =
        orchestration_lifecycle::check(&source, &codex_home, &user_home, false).unwrap();
    assert_eq!(
        connected.board_version.as_deref(),
        Some(board_report.bd_version.as_deref().unwrap()),
        "the delivered board is visible to the loop check"
    );
    let bd = codex_home.join("harness/bin/bd.exe");
    let version =
        String::from_utf8_lossy(&run(&bd, &workspace, &["--version"]).stdout).into_owned();
    assert!(version.contains("1.3.0"), "{version}");

    // Real board evidence through the installed tool, then read it back after
    // rollback: archived loop evidence must outlive the installation.
    let project = root_path.join("loop-project");
    fs::create_dir_all(&project).unwrap();
    board_git(&project, &["init", "-q"]);
    board_git(&project, &["config", "user.email", "loop@example.test"]);
    board_git(&project, &["config", "user.name", "Loop Fixture"]);
    let init = run(
        &bd,
        &project,
        &[
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "loop",
        ],
    );
    assert!(init.status.success(), "{}", failed("init", &init));
    let epic = json(
        &bd,
        &project,
        &["create", "Stage: loop evidence", "--type", "epic", "--json"],
    );
    let epic_id = field(&epic, "id");
    let item = json(
        &bd,
        &project,
        &[
            "create",
            "Loop evidence item",
            "--type",
            "task",
            "--parent",
            &epic_id,
            "--json",
        ],
    );
    let item_id = field(&item, "id");
    let closed = run(
        &bd,
        &project,
        &[
            "close",
            &item_id,
            "--reason",
            "archived: synthetic evidence",
        ],
    );
    assert!(closed.status.success(), "{}", failed("close", &closed));

    prepare_user_profile(&user_home);
    let version = Command::new(codex_home.join("harness/bin/codex.exe"))
        .current_dir(&workspace)
        .env("CODEX_HOME", &codex_home)
        .env("USERPROFILE", &user_home)
        .env("HOME", &user_home)
        .env("APPDATA", user_home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", user_home.join("AppData/Local"))
        .env("TEMP", user_home.join("AppData/Local/Temp"))
        .env("TMP", user_home.join("AppData/Local/Temp"))
        .args(["--version"])
        .output()
        .unwrap();
    assert!(
        version.status.success(),
        "ordinary installed launcher failed outside checkout: {}{}",
        String::from_utf8_lossy(&version.stderr),
        String::from_utf8_lossy(&version.stdout)
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
    assert!(
        text.contains("codex"),
        "ordinary session must start the installed launcher: {text}"
    );

    board_lifecycle::disconnect(&board_on).unwrap();
    assert!(
        !codex_home.join("harness/bin/bd.exe").exists(),
        "rollback removes the board link"
    );
    let package = codex_home
        .join("harness/board/packages")
        .join(board_report.bd_version.as_deref().unwrap())
        .join("bd.exe");
    assert!(package.is_file(), "the pinned board package stays in place");
    let evidence = json(&package, &project, &["show", &item_id, "--json"]);
    let evidence = evidence
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or(evidence);
    assert_eq!(field(&evidence, "status"), "closed");
    assert!(
        field(&evidence, "close_reason").contains("archived: synthetic evidence"),
        "archived board evidence survives rollback: {evidence}"
    );

    let disconnected =
        core_disconnect::disconnect(&codex_home, &user_home, &user_home, false).unwrap();
    assert_eq!(disconnected.status, "disconnected");
    assert!(
        disconnected.orchestration.preserved.contains(&"skills"),
        "the skill source stays readable for the next install"
    );
    assert!(
        !user_home.join(".agents/skills/team-lead").exists(),
        "rollback removes the loop guidance links"
    );
    assert!(user_home.join(".agents/skills/foreign/SKILL.md").is_file());
    let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert!(
        config.contains("model = 'unrelated-kept'"),
        "unrelated configuration survives rollback: {config}"
    );
    assert_eq!(
        fs::read(codex_home.join("auth.json")).unwrap(),
        b"unrelated-auth",
        "credentials are unrelated configuration"
    );
    eprintln!(
        "rollback preserved: {:?}",
        disconnected.orchestration.preserved
    );
}

fn run(program: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", "isolated-loop-check")
        .output()
        .unwrap()
}

fn json(program: &Path, cwd: &Path, args: &[&str]) -> serde_json::Value {
    let out = run(program, cwd, args);
    assert!(out.status.success(), "{}", failed(args[0], &out));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn field(value: &serde_json::Value, name: &str) -> String {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("missing field {name} in {value}"))
        .to_owned()
}

fn failed(command: &str, out: &Output) -> String {
    format!(
        "bd {command} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn board_git(cwd: &Path, args: &[&str]) {
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

fn disk_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text.as_ref()))
}

fn prepare_user_profile(user: &Path) {
    for dir in [
        user.join("AppData/Roaming"),
        user.join("AppData/Local/Temp"),
        user.join(".agents/skills"),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
}

struct PathRestore(Option<std::ffi::OsString>);
impl PathRestore {
    fn capture() -> Self {
        Self(std::env::var_os("PATH"))
    }
}
impl Drop for PathRestore {
    fn drop(&mut self) {
        unsafe {
            match &self.0 {
                Some(path) => std::env::set_var("PATH", path),
                None => std::env::remove_var("PATH"),
            }
        }
    }
}
