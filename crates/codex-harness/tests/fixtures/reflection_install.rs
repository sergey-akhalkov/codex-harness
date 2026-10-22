//! Immutable installed bundle for the reflection acceptance slice.
//!
//! The construction mirrors the isolated install used by
//! `orchestration_isolated_install.rs`, but the selected upstream is a small
//! model-free executable compiled beside the native control fixture. It answers
//! the install-time CLI probes, edits only the private `hooks` feature,
//! forwards app-server traffic to the existing fixture, and forwards the visible
//! frontend to the existing owned frontend double. No live home, global PATH
//! scope or model route participates.

use super::{LEAD_THREAD, PROFILE, Workspace};
use harness_core::{build_identity, core_disconnect, core_install, installation_state::PathScope};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(super) const PRESERVED: &str = "reflection-install-preservation";

#[derive(Debug)]
pub(super) struct Invocation {
    pub cwd: PathBuf,
    pub args: Vec<String>,
}

pub(super) struct InstalledReflection {
    _root: IsolatedRoot,
    source: PathBuf,
    build: PathBuf,
    home: PathBuf,
    user_home: PathBuf,
    dependency_user_home: PathBuf,
    upstream: PathBuf,
    transport: PathBuf,
    pub manager: PathBuf,
    _path: PathRestore,
}

impl InstalledReflection {
    pub(super) fn connect(workspace: &Workspace) -> Self {
        // `kit_root()` canonicalizes to a verbatim `\\?\` path on Windows.
        // The ownership records accept ordinary drive paths, exactly as the
        // existing isolated install does.
        let source = disk_path(&super::kit_root());
        let root = IsolatedRoot::new();
        let root_path = root.path();
        let build = root_path.join("build");
        let transport = root_path.join("transport");
        let user_home = root_path.join("user");
        let dependency_user_home = root_path.join("dependency-user");
        fs::create_dir_all(&build).unwrap();
        fs::create_dir_all(&transport).unwrap();
        for dir in [
            user_home.join("AppData/Roaming"),
            user_home.join("AppData/Local/Temp"),
            user_home.join(".agents/skills"),
            dependency_user_home.join(".agents/skills"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        write_build(&source, &build);

        let fixture = transport.join("harness-executor-fixture.exe");
        fs::copy(super::fixture(), &fixture).unwrap();
        fs::write(
            transport.join("transport.txt"),
            fixture.to_string_lossy().as_bytes(),
        )
        .unwrap();
        let frontend = transport.join("frontend-double.exe");
        fs::copy(super::frontend_double(), &frontend).unwrap();
        fs::write(
            transport.join("frontend.txt"),
            frontend.to_string_lossy().as_bytes(),
        )
        .unwrap();
        let upstream = compile_upstream(&transport);

        // The standalone workspace writes a placeholder launch file. A fresh
        // owned install replaces it; it must not be adopted as prior state.
        let placeholder = workspace.home.join("harness/native-launch.json");
        if placeholder.exists() {
            fs::remove_file(&placeholder).unwrap();
        }
        fs::write(
            workspace.home.join("config.toml"),
            format!(
                "model = \"deepseek-flash\"\nmodel_provider = \"deepseek\"\nmodel_reasoning_effort = \"max\"\n\n[synthetic]\npreserved = \"{PRESERVED}\"\n"
            ),
        )
        .unwrap();

        let installed = Self {
            _root: root,
            source,
            build,
            home: workspace.home.clone(),
            user_home,
            dependency_user_home,
            upstream,
            transport,
            manager: workspace.home.join("harness/bin/codex-harness.exe"),
            _path: PathRestore::capture(),
        };
        let preview = installed.install(true);
        assert_eq!(preview.status, "preview");
        assert!(preview.runtime.is_none());
        installed.install(false);
        installed.register_lead(workspace);
        installed.assert_installed(workspace);
        installed
    }

    fn request(&self) -> core_install::Request {
        self.request_at(&self.build)
    }

    fn request_at(&self, build: &Path) -> core_install::Request {
        core_install::Request {
            source: self.source.clone(),
            build: build.to_path_buf(),
            codex_home: self.home.clone(),
            user_home: self.user_home.clone(),
            dependency_user_home: self.dependency_user_home.clone(),
            upstream: Some(self.upstream.clone()),
            timeout: Duration::from_secs(120),
            path_scope: Some(PathScope::Process),
            goal_recovery: None,
        }
    }

    fn install(&self, preview: bool) -> core_install::Report {
        core_install::connect(&self.request(), preview).unwrap()
    }

    fn register_lead(&self, workspace: &Workspace) {
        let registry = self.home.join("harness/lead-endpoints");
        fs::create_dir_all(&registry).unwrap();
        fs::copy(
            &workspace.endpoint,
            registry.join(format!("{LEAD_THREAD}.json")),
        )
        .unwrap();
    }

    pub(super) fn reconnect(&self) -> core_install::Report {
        self.install(false)
    }

    pub(super) fn launcher(&self) -> PathBuf {
        self.home.join("harness/bin/codex.exe")
    }

    /// Move the owned installed links through the ordinary core update path.
    pub(super) fn update_to_new_build(&self) -> PathBuf {
        let build = self._root.path().join("build-updated");
        write_build(&self.source, &build);
        let report = core_install::connect(&self.request_at(&build), false).unwrap();
        assert_eq!(report.status, "connected");
        build
    }

    /// Temporarily select a foreign target while preserving the installed
    /// owner's original link object and NTFS identity for restoration.
    pub(super) fn replace_launcher_with(&self, target: &Path) -> PathBuf {
        let launcher = self.launcher();
        let retained = launcher.with_file_name("codex-owned.exe");
        assert!(!retained.exists());
        fs::rename(&launcher, &retained).unwrap();
        std::os::windows::fs::symlink_file(target, &launcher).unwrap();
        retained
    }

    pub(super) fn restore_launcher(&self, retained: &Path) {
        let launcher = self.launcher();
        fs::remove_file(&launcher).unwrap();
        fs::rename(retained, &launcher).unwrap();
    }

    pub(super) fn disconnect(&self) -> core_disconnect::Report {
        let preview = core_disconnect::disconnect(
            &self.home,
            &self.user_home,
            &self.dependency_user_home,
            true,
        )
        .unwrap();
        assert_eq!(preview.status, "preview");
        core_disconnect::disconnect(
            &self.home,
            &self.user_home,
            &self.dependency_user_home,
            false,
        )
        .unwrap()
    }

    pub(super) fn assert_installed(&self, workspace: &Workspace) {
        for name in build_identity::BINARIES {
            let built = self.build.join(name);
            assert!(built.is_file(), "immutable build binary {name} is missing");
        }
        for name in core_install::core_linked_binaries() {
            let installed = self.home.join("harness/bin").join(name);
            assert!(installed.is_file(), "installed binary {name} is missing");
            assert_eq!(
                build_identity::hash_file(&installed).unwrap(),
                build_identity::hash_file(&self.build.join(name)).unwrap(),
                "installed binary {name} is not the immutable build"
            );
        }
        assert_eq!(
            self.manager.canonicalize().unwrap(),
            self.build.join("codex-harness.exe").canonicalize().unwrap()
        );
        assert_eq!(
            fs::read_link(self.home.join("harness/bin/codex.exe")).unwrap(),
            self.build.join("codex.exe")
        );
        let registration: serde_json::Value = serde_json::from_slice(
            &fs::read(self.home.join("harness/native-launch.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(registration["schema"], 2);
        assert_eq!(registration["build"], serde_json::json!(self.build));
        let registered = registration["upstream"]["executable"]
            .as_str()
            .expect("registered upstream path");
        assert_eq!(disk_path(Path::new(registered)), self.upstream);
        assert_eq!(
            registration["upstream"]["sha256"],
            serde_json::json!(build_identity::hash_file(&self.upstream).unwrap())
        );
        let config = fs::read_to_string(self.home.join("config.toml")).unwrap();
        assert!(config.contains(PRESERVED), "{config}");
        let profile = fs::read_to_string(self.home.join(format!("{PROFILE}.config.toml"))).unwrap();
        assert!(profile.contains("deepseek-flash"), "{profile}");
        assert!(
            !workspace.request.exists(),
            "installed inputs never use a caller-authored request"
        );
    }

    pub(super) fn assert_app_servers(&self, cwd: &Path) {
        let app_servers = self
            .invocations()
            .into_iter()
            .filter(|invocation| {
                invocation
                    .args
                    .first()
                    .is_some_and(|arg| arg == "app-server")
            })
            .collect::<Vec<_>>();
        assert!(
            app_servers.len() >= 2,
            "both reflection forks must reach the installed launcher: {app_servers:?}"
        );
        for invocation in app_servers {
            assert_eq!(invocation.cwd, cwd);
            assert!(invocation.args.iter().any(|arg| arg == "--listen"));
            assert!(invocation.args.iter().any(|arg| arg == "--ws-token-file"));
        }
    }

    fn invocations(&self) -> Vec<Invocation> {
        let dir = self.transport.join("invocations");
        if !dir.exists() {
            return Vec::new();
        }
        let mut paths = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let text = fs::read_to_string(&path).unwrap();
                let mut lines = text.lines();
                let cwd = lines
                    .next()
                    .and_then(|line| line.strip_prefix("cwd="))
                    .map(PathBuf::from)
                    .unwrap();
                let args = lines
                    .next()
                    .and_then(|line| line.strip_prefix("args="))
                    .map(|line| line.split('\u{1f}').map(str::to_owned).collect::<Vec<_>>())
                    .unwrap();
                Invocation { cwd, args }
            })
            .collect()
    }
}

fn write_build(source: &Path, build: &Path) {
    fs::create_dir_all(build).unwrap();
    for name in build_identity::BINARIES {
        let from = Path::new(env!("CARGO_BIN_EXE_codex")).with_file_name(name);
        fs::copy(&from, build.join(name))
            .unwrap_or_else(|error| panic!("copy {name} from {}: {error}", from.display()));
    }
    let record = build_identity::BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.to_path_buf(),
        source: build_identity::source_identity(source).unwrap(),
        rustc: "reflection installed acceptance".into(),
        cargo: "reflection installed acceptance".into(),
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
}

fn compile_upstream(root: &Path) -> PathBuf {
    let source = root.join("upstream.rs");
    let output = root.join("upstream.exe");
    fs::write(&source, UPSTREAM).unwrap();
    let status = Command::new("rustc.exe")
        .arg(&source)
        .arg("--edition=2021")
        .arg("-o")
        .arg(&output)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "compile {} for {}",
        source.display(),
        output.display()
    );
    output
}

fn disk_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text.as_ref()))
}

const UPSTREAM: &str = r#"
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let exe = env::current_exe().unwrap();
    let dir = exe.parent().unwrap().to_path_buf();
    let fixture = fs::read_to_string(dir.join("transport.txt"))
        .unwrap()
        .trim()
        .to_owned();
    let args: Vec<String> = env::args().skip(1).collect();
    record(&dir, &args);
    if args.len() == 1 && args[0] == "--version" {
        println!("codex-cli 0.160.0");
        return;
    }
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("Usage: codex [--profile PROFILE]");
        println!("Profiles are read from <name>.config.toml.");
        return;
    }
    if args.first().map(String::as_str) == Some("features") {
        match args.get(1).map(String::as_str) {
            Some("disable") if args.get(2).map(String::as_str) == Some("hooks") => {
                disable_hooks();
                return;
            }
            Some("list") => {
                println!("hooks stable false");
                println!("code_mode stable false");
                return;
            }
            _ => {}
        }
    }
    if args.first().map(String::as_str) == Some("--remote") {
        let frontend = fs::read_to_string(dir.join("frontend.txt"))
            .unwrap()
            .trim()
            .to_owned();
        let status = Command::new(&frontend).status().unwrap();
        std::process::exit(status.code().unwrap_or(1));
    }
    let start = args.iter().position(|arg| arg == "app-server").unwrap_or(0);
    let status = Command::new(fixture)
        .args(&args[start..])
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

fn record(dir: &Path, args: &[String]) {
    let log = dir.join("invocations");
    fs::create_dir_all(&log).unwrap();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let body = format!(
        "cwd={}\nargs={}\n",
        env::current_dir().unwrap().display(),
        args.join("\u{1f}")
    );
    fs::write(
        log.join(format!("{}-{}.txt", std::process::id(), nanos)),
        body,
    )
    .unwrap();
}

fn disable_hooks() {
    let home = PathBuf::from(env::var_os("CODEX_HOME").expect("CODEX_HOME"));
    let path = home.join("config.toml");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut kept = Vec::new();
    let mut features = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            features = trimmed == "[features]";
            kept.push(line.to_owned());
            continue;
        }
        if trimmed.starts_with("features.hooks")
            || (features && trimmed.starts_with("hooks"))
        {
            continue;
        }
        kept.push(line.to_owned());
    }
    let body = kept.join("\n");
    fs::write(&path, format!("features.hooks = false\n{body}\n")).unwrap();
}
"#;

struct PathRestore(Option<OsString>);

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

struct IsolatedRoot(Option<tempfile::TempDir>);

impl IsolatedRoot {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("reflection-installed-")
            .tempdir()
            .unwrap();
        Self(Some(dir))
    }

    fn path(&self) -> &Path {
        self.0.as_ref().unwrap().path()
    }
}

impl Drop for IsolatedRoot {
    fn drop(&mut self) {
        if std::env::var_os("HARNESS_KEEP_ISOLATED_EVIDENCE").is_none() {
            return;
        }
        if let Some(dir) = self.0.take() {
            eprintln!(
                "retained reflection install evidence: {}",
                dir.keep().display()
            );
        }
    }
}
