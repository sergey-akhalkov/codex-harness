//! Isolated core/board install from this checkout, then an ordinary Codex
//! start outside the checkout. Does not touch the live user home.
#![cfg(windows)]

use harness_core::{
    board_lifecycle, build_identity, core_disconnect, core_install, installation_state::PathScope,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
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
    core_disconnect::disconnect(&codex_home, &user_home, &user_home, false).unwrap();
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
