//! Native rust-analyzer component provisioning into an adopted rustup toolchain.
//!
//! Installs only the missing component of the already selected installed
//! toolchain. The cohort artifact is downloaded and verified before rustup is
//! invoked; active consumers, foreign receipts and an existing analyzer stop
//! the operation without mutation. Every step is journaled and reversible.
#![cfg(windows)]

use crate::{
    build_identity,
    dependency_discovery::local_path,
    dependency_fetch::Client,
    dependency_npm_install::{InstallationLock, replace_json},
    dependency_process,
    native_build::ordinary_ancestors,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, Outcome, StopReason},
    registration_native::StagedFile,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_ARCHIVE: u64 = 128 * 1024 * 1024;
const MAX_EXECUTABLE: u64 = 512 * 1024 * 1024;
const MAX_LISTING: u64 = 4 * 1024 * 1024;

pub struct Request {
    pub user_home: PathBuf,
    pub state: PathBuf,
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "rust component provisioning input was rejected",
    )
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

fn soft(state: &str, reason: &str) -> Value {
    json!({"id": "rust", "state": state, "reason": reason, "packages_acquired": false})
}

struct Cohort {
    installation: PathBuf,
    manifest: PathBuf,
    components: PathBuf,
    executable: PathBuf,
    component: String,
    toolchain: String,
    version: String,
    url: String,
    archive_sha256: String,
}

fn valid_toolchain(selected: &str) -> bool {
    !selected.is_empty()
        && selected
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    let text = fs::read_to_string(path)?;
    Ok(text.lines().map(str::to_owned).collect())
}

fn find_rustup(home: &Path) -> io::Result<Option<PathBuf>> {
    let mut candidates = vec![home.join(".cargo/bin/rustup.exe")];
    if let Some(path) = std::env::var_os("PATH") {
        for (index, entry) in std::env::split_paths(&path).enumerate() {
            if index == 256 {
                return Err(invalid());
            }
            if entry.is_absolute() {
                candidates.push(entry.join("rustup.exe"));
            }
        }
    }
    for candidate in candidates {
        if candidate.is_file() {
            return local_path(&candidate).map(Some);
        }
    }
    Ok(None)
}

fn system_tar() -> io::Result<PathBuf> {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or_else(|| conflict("Windows system tools are unavailable"))?;
    let tar = root.join("System32/tar.exe");
    if !tar.is_file() {
        return Err(conflict(
            "The documented Windows tar executable is unavailable",
        ));
    }
    local_path(&tar)
}

fn cohort(home: &Path) -> io::Result<Result<Cohort, Value>> {
    let rustup_root = home.join(".rustup");
    let settings = rustup_root.join("settings.toml");
    if !settings.is_file() {
        return Ok(Err(soft(
            "prerequisite-missing",
            "An existing rustup manager and installed selected toolchain are required; no toolchain will be installed or changed.",
        )));
    }
    let settings_text = fs::read_to_string(&settings)?;
    let settings: toml::Value = toml::from_str(&settings_text).map_err(|_| invalid())?;
    let Some(selected) = settings
        .get("default_toolchain")
        .and_then(toml::Value::as_str)
    else {
        return Err(invalid());
    };
    if !valid_toolchain(selected) {
        return Err(conflict("Invalid selected rustup toolchain"));
    }
    let installation = rustup_root.join("toolchains").join(selected);
    let manifest = installation.join("lib/rustlib/multirust-channel-manifest.toml");
    let components = installation.join("lib/rustlib/components");
    let executable = installation.join("bin/rust-analyzer.exe");
    if !manifest.is_file() || !components.is_file() {
        return Ok(Err(soft(
            "prerequisite-missing",
            "Selected installed toolchain lacks its rustup receipt; do not invoke a toolchain download.",
        )));
    }
    if fs::symlink_metadata(&executable).is_ok() {
        return Ok(Err(soft(
            "pending",
            "An existing analyzer needs explicit identity repair; do not overwrite it.",
        )));
    }
    let manifest_text = fs::read_to_string(&manifest)?;
    let data: toml::Value = toml::from_str(&manifest_text).map_err(|_| invalid())?;
    let preview = data
        .get("pkg")
        .and_then(|pkg| pkg.get("rust-analyzer-preview"))
        .and_then(|preview| preview.get("target"))
        .ok_or_else(invalid)?;
    let mut target = selected
        .strip_prefix("stable-")
        .or_else(|| selected.strip_prefix("beta-"))
        .unwrap_or(selected)
        .to_owned();
    if preview.get(&target).is_none() {
        target = preview
            .as_table()
            .into_iter()
            .flatten()
            .map(|(name, _)| name.clone())
            .find(|name| selected.ends_with(name.as_str()))
            .unwrap_or_default();
    }
    let package = preview.get(&target).ok_or_else(invalid)?;
    let (available, url, archive_sha256) = (
        package
            .get("available")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        package
            .get("xz_url")
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        package
            .get("xz_hash")
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    );
    if !available
        || !url.starts_with("https://static.rust-lang.org/")
        || archive_sha256.len() != 64
        || !archive_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Ok(Err(soft(
            "prerequisite-missing",
            "This installed compiler cohort has no available declared analyzer component.",
        )));
    }
    let version = data
        .get("pkg")
        .and_then(|pkg| pkg.get("rust"))
        .and_then(|rust| rust.get("version"))
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(Ok(Cohort {
        component: format!("rust-analyzer-preview-{target}"),
        installation,
        manifest,
        components,
        executable,
        toolchain: selected.to_owned(),
        version,
        url,
        archive_sha256,
    }))
}

/// Verify the cohort archive and return the hash of its single analyzer
/// executable. Uses the documented Windows tar for the xz member without
/// adding a decompression dependency.
fn artifact_hash(url: &str, archive_sha256: &str) -> io::Result<String> {
    let client = Client::new().map_err(io::Error::other)?;
    let raw = client
        .fetch_validated(url, MAX_ARCHIVE)
        .map_err(io::Error::other)?;
    if format!("{:x}", Sha256::digest(&raw)) != archive_sha256 {
        return Err(conflict(
            "Installed-cohort Rust analyzer archive checksum mismatch",
        ));
    }
    let tar = system_tar()?;
    let private = tempfile::Builder::new()
        .prefix("harness-rust-component-")
        .tempdir()?;
    let archive = private.path().join("component.tar.xz");
    fs::write(&archive, &raw)?;
    let listing = private.path().join("listing.txt");
    let mut command = CommandSpec::new(&tar);
    command.args = vec!["-tf".into(), archive.as_os_str().into()];
    command.stdout = Some(fs::File::create(&listing)?);
    command.stderr = Some(fs::File::create(private.path().join("list-error.txt"))?);
    run_bounded(command, &listing, MAX_LISTING, Duration::from_secs(60))?;
    let member = fs::read_to_string(&listing)?
        .lines()
        .map(|line| line.trim().trim_start_matches("./"))
        .filter(|line| {
            Path::new(line)
                .file_name()
                .is_some_and(|name| name == "rust-analyzer.exe")
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if member.len() != 1 {
        return Err(conflict(
            "Rust component executable is missing or ambiguous",
        ));
    }
    let extracted = private.path().join("rust-analyzer.exe");
    let mut command = CommandSpec::new(&tar);
    command.args = vec![
        "-xOf".into(),
        archive.as_os_str().into(),
        OsString::from(member[0].clone()),
    ];
    command.stdout = Some(fs::File::create(&extracted)?);
    command.stderr = Some(fs::File::create(private.path().join("extract-error.txt"))?);
    run_bounded(
        command,
        &extracted,
        MAX_EXECUTABLE,
        Duration::from_secs(180),
    )?;
    build_identity::hash_file(&extracted)
}

fn run_bounded(
    command: CommandSpec,
    output: &Path,
    limit: u64,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Deadline::after(timeout)?;
    let cancellation = Cancellation::default();
    let job = Job::new(Limits {
        memory_bytes: Some(512 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })?;
    let child = job.spawn(&command)?;
    while child.is_running()? && !deadline.expired() && !cancellation.is_cancelled() {
        if fs::metadata(output)
            .map(|meta| meta.len() > limit)
            .unwrap_or(false)
        {
            cancellation.cancel();
            return Err(conflict("Rust component extraction exceeded its bound"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let outcome = job.wait(&child, deadline, &cancellation, Duration::from_secs(5))?;
    ordinary(outcome)
}

fn ordinary(outcome: Outcome) -> io::Result<()> {
    match outcome.reason {
        StopReason::Exited if outcome.exit_code == 0 => Ok(()),
        StopReason::Exited => Err(conflict("Rust component command failed")),
        StopReason::Timeout => Err(conflict("Rust component command timed out")),
        StopReason::Cancelled => Err(conflict("Rust component command was cancelled")),
        StopReason::MemoryLimit => Err(conflict("Rust component command exceeded its limits")),
    }
}

fn rustup_command(rustup: &Path, arguments: &[&str], timeout: Duration) -> io::Result<()> {
    let private = tempfile::Builder::new()
        .prefix("harness-rustup-")
        .tempdir()?;
    let stdout = private.path().join("stdout.txt");
    let mut command = CommandSpec::new(rustup);
    command
        .args
        .extend(arguments.iter().map(|argument| OsString::from(*argument)));
    command.stdout = Some(fs::File::create(&stdout)?);
    command.stderr = Some(fs::File::create(private.path().join("stderr.txt"))?);
    run_bounded(command, &stdout, MAX_LISTING, timeout)
}

trait Ops {
    fn artifact_hash(&mut self, url: &str, archive_sha256: &str) -> io::Result<String>;
    fn rustup(&mut self, rustup: &Path, arguments: &[&str]) -> io::Result<()>;
    fn consumers_clear(&mut self, installation: &Path) -> io::Result<bool>;
}

struct RealOps;

impl Ops for RealOps {
    fn artifact_hash(&mut self, url: &str, archive_sha256: &str) -> io::Result<String> {
        artifact_hash(url, archive_sha256)
    }

    fn rustup(&mut self, rustup: &Path, arguments: &[&str]) -> io::Result<()> {
        rustup_command(rustup, arguments, Duration::from_secs(180))
    }

    fn consumers_clear(&mut self, installation: &Path) -> io::Result<bool> {
        let mut probes = [json!({"installation_root": installation})];
        dependency_process::observe(&mut probes)?;
        Ok(probes[0]["active_consumers"]["state"] == "observed"
            && probes[0]["active_consumers"]["processes"]
                .as_array()
                .is_none_or(|processes| processes.is_empty()))
    }
}

/// Install the missing analyzer component of the adopted toolchain.
pub fn provision(request: &Request) -> io::Result<Value> {
    install(request, &mut RealOps)
}

fn install(request: &Request, ops: &mut dyn Ops) -> io::Result<Value> {
    let home = local_path(&request.user_home)?;
    ordinary_ancestors(&request.state)?;
    fs::create_dir_all(&request.state)?;
    build_identity::ordinary(&request.state)?;
    let state = local_path(&request.state)?;
    let Some(rustup) = find_rustup(&home)? else {
        return Ok(soft(
            "prerequisite-missing",
            "An existing rustup manager and installed selected toolchain are required; no toolchain will be installed or changed.",
        ));
    };
    let cohort = match cohort(&home)? {
        Err(result) => return Ok(result),
        Ok(cohort) => cohort,
    };
    let executable_hash = ops.artifact_hash(&cohort.url, &cohort.archive_sha256)?;
    ordinary_ancestors(&state)?;
    let _lock = InstallationLock::acquire(&state, &cohort.installation)?;
    if !ops.consumers_clear(&cohort.installation)? {
        return Ok(soft(
            "pending",
            "Existing toolchain consumers prevent component modification; no process stopped.",
        ));
    }
    let before = read_lines(&cohort.components)?;
    if before.iter().any(|line| line == &cohort.component)
        || fs::symlink_metadata(&cohort.executable).is_ok()
    {
        return Err(conflict(
            "Analyzer component state changed before provisioning",
        ));
    }
    let manifest_sha256 = build_identity::hash_file(&cohort.manifest)?;
    let transactions = state.join("transactions");
    ordinary_ancestors(&transactions)?;
    fs::create_dir_all(&transactions)?;
    build_identity::ordinary(&transactions)?;
    let owner = tempfile::Builder::new()
        .prefix("standalone-")
        .tempdir_in(&transactions)?
        .keep();
    let journal_path = owner.join("rust-component.json");
    let journal = json!({
        "schema_version": 1,
        "owner": "codex-harness-dependencies",
        "kind": "rustup-component-install",
        "phase": "prepared",
        "installation": cohort.installation,
        "rustup": rustup,
        "toolchain": cohort.toolchain,
        "component": cohort.component,
        "executable": cohort.executable,
        "executable_sha256": executable_hash,
        "components_before": before,
        "manifest_sha256": manifest_sha256,
        "transaction_id": owner.file_name().and_then(|name| name.to_str()).unwrap_or("standalone"),
    });
    let prepared = serde_json::to_vec_pretty(&journal)?;
    StagedFile::create(&journal_path, &prepared)?.commit()?;
    ops.rustup(
        &rustup,
        &[
            "component",
            "add",
            "rust-analyzer",
            "--toolchain",
            &cohort.toolchain,
        ],
    )?;
    let committed = (|| -> io::Result<()> {
        if build_identity::hash_file(&cohort.executable)? != executable_hash
            || build_identity::hash_file(&cohort.manifest)? != manifest_sha256
        {
            return Err(conflict(
                "Rust component differs from the existing cohort artifact; recovery journal retained",
            ));
        }
        let current = read_lines(&cohort.components)?;
        let expected: std::collections::BTreeSet<&str> = before
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(cohort.component.as_str()))
            .collect();
        let actual: std::collections::BTreeSet<&str> = current.iter().map(String::as_str).collect();
        if actual != expected {
            return Err(conflict(
                "Unexpected Rust component receipt change; recovery journal retained",
            ));
        }
        let mut updated = journal.clone();
        updated["phase"] = json!("committed");
        replace_json(
            &journal_path,
            &prepared,
            &serde_json::to_vec_pretty(&updated)?,
        )?;
        Ok(())
    })();
    match committed {
        Ok(()) => Ok(json!({
            "id": "rust",
            "state": "installed-unverified",
            "version": cohort.version,
            "transaction_journal": journal_path,
            "installation": cohort.installation,
            "source": cohort.url,
            "sha256": cohort.archive_sha256,
            "packages_acquired": true,
            "remaining": "Actual Rust project diagnostic, clearance and navigation; compiler/toolchain version was preserved.",
        })),
        Err(error) => Err(error),
    }
}

/// Finish or roll back one journaled rust component installation.
pub fn recover_journal(path: &Path, rollback_committed: bool) -> io::Result<Value> {
    let bytes = fs::read(path)?;
    let journal: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if journal["owner"] != "codex-harness-dependencies" {
        return Err(conflict("Unsupported or foreign dependency transaction"));
    }
    let pending = |reason: &str| Ok(json!({"state": "pending", "journal": path, "reason": reason}));
    if journal["kind"] != "rustup-component-install" {
        return pending("Unsupported dependency transaction kind; preserve it for repair.");
    }
    if !matches!(
        journal["phase"].as_str(),
        Some("prepared" | "committed" | "restoring" | "restored")
    ) {
        return pending("Dependency transaction phase is absent or unrecognized.");
    }
    if journal["phase"] == "restored" {
        return Ok(json!({"state": "restored", "journal": path}));
    }
    if journal["phase"] == "committed" && !rollback_committed {
        return Ok(json!({"state": "committed", "journal": path}));
    }
    let field = |name: &str| -> Option<PathBuf> {
        journal[name]
            .as_str()
            .map(PathBuf::from)
            .filter(|value| value.is_absolute())
    };
    let (Some(installation), Some(rustup), Some(executable)) =
        (field("installation"), field("rustup"), field("executable"))
    else {
        return pending("Dependency recovery path is missing or relative.");
    };
    for path in [&installation, &rustup, &executable] {
        if fs::symlink_metadata(path)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false)
        {
            return pending("Dependency recovery path is a link; preserve it for repair.");
        }
    }
    let manifest = installation.join("lib/rustlib/multirust-channel-manifest.toml");
    let components = installation.join("lib/rustlib/components");
    let Some(component) = journal["component"].as_str() else {
        return pending("Dependency recovery component is missing.");
    };
    let Some(toolchain) = journal["toolchain"].as_str() else {
        return pending("Dependency recovery toolchain is missing.");
    };
    let before: std::collections::BTreeSet<String> = journal["components_before"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let current = read_lines(&components).unwrap_or_default();
    let current_set: std::collections::BTreeSet<&str> =
        current.iter().map(String::as_str).collect();
    if !current_set.contains(component) {
        let mut updated = journal.clone();
        updated["phase"] = json!("restored");
        replace_json(path, &bytes, &serde_json::to_vec_pretty(&updated)?)?;
        return Ok(json!({"state": "restored", "journal": path}));
    }
    let expected: std::collections::BTreeSet<&str> = before
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(component))
        .collect();
    if build_identity::hash_file(&manifest).ok().as_deref() != journal["manifest_sha256"].as_str()
        || !executable.is_file()
        || build_identity::hash_file(&executable).ok().as_deref()
            != journal["executable_sha256"].as_str()
        || current_set != expected
    {
        return pending("Installed Rust cohort/component changed; preserve it.");
    }
    rustup_command(
        &rustup,
        &[
            "component",
            "remove",
            "rust-analyzer",
            "--toolchain",
            toolchain,
        ],
        Duration::from_secs(180),
    )?;
    let restored: std::collections::BTreeSet<String> =
        read_lines(&components)?.into_iter().collect();
    if restored != before {
        return Err(conflict(
            "rustup did not restore the exact prior component set",
        ));
    }
    let mut updated = journal.clone();
    updated["phase"] = json!("restored");
    replace_json(path, &bytes, &serde_json::to_vec_pretty(&updated)?)?;
    Ok(json!({"state": "restored", "journal": path}))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOOLCHAIN: &str = "stable-x86_64-pc-windows-msvc";
    const COMPONENT: &str = "rust-analyzer-preview-x86_64-pc-windows-msvc";

    struct FakeOps {
        executable: PathBuf,
        components: PathBuf,
        executable_bytes: Vec<u8>,
        component: String,
        added: bool,
    }

    impl FakeOps {
        fn new(fixture: &Fixture) -> Self {
            Self {
                executable: fixture.executable.clone(),
                components: fixture.components.clone(),
                executable_bytes: b"owned fixture analyzer".to_vec(),
                component: COMPONENT.to_owned(),
                added: false,
            }
        }
    }

    impl Ops for FakeOps {
        fn artifact_hash(&mut self, _url: &str, _archive_sha256: &str) -> io::Result<String> {
            Ok(format!("{:x}", Sha256::digest(&self.executable_bytes)))
        }

        fn rustup(&mut self, _rustup: &Path, arguments: &[&str]) -> io::Result<()> {
            if arguments[..3] != ["component", "add", "rust-analyzer"] {
                return Err(conflict("fixture rustup refused the request"));
            }
            fs::create_dir_all(self.executable.parent().ok_or_else(invalid)?)?;
            fs::write(&self.executable, &self.executable_bytes)?;
            let mut receipts = fs::read_to_string(&self.components)?;
            receipts.push_str(&self.component);
            receipts.push('\n');
            fs::write(&self.components, receipts)?;
            self.added = true;
            Ok(())
        }

        fn consumers_clear(&mut self, _installation: &Path) -> io::Result<bool> {
            Ok(true)
        }
    }

    struct Fixture {
        // Keeps the disposable fixture tree alive for the assertions.
        #[allow(dead_code)]
        root: tempfile::TempDir,
        request: Request,
        executable: PathBuf,
        components: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("harness-rust-component-Юникод-")
                .tempdir()
                .unwrap();
            let home = root.path().join("user");
            let rustlib = home
                .join(".rustup/toolchains")
                .join(TOOLCHAIN)
                .join("lib/rustlib");
            fs::create_dir_all(&rustlib).unwrap();
            fs::create_dir_all(home.join(".cargo/bin")).unwrap();
            fs::write(home.join(".cargo/bin/rustup.exe"), b"fixture manager").unwrap();
            fs::write(
                home.join(".rustup/settings.toml"),
                format!("default_toolchain = \"{TOOLCHAIN}\"\n"),
            )
            .unwrap();
            let manifest = rustlib.join("multirust-channel-manifest.toml");
            fs::write(
                &manifest,
                "[pkg.rust]\nversion = \"1.98.1\"\n\n[pkg.rust-analyzer-preview.target.x86_64-pc-windows-msvc]\navailable = true\nxz_url = \"https://static.rust-lang.org/dist/rust-analyzer.tar.xz\"\nxz_hash = \"9a07112f17a964d9c0b1a54653af95559d7de33cce1cb3dffd60dfc4c85ccfb0\"\n",
            )
            .unwrap();
            let components = rustlib.join("components");
            fs::write(&components, "rustc\nrust-std\n").unwrap();
            let executable = home
                .join(".rustup/toolchains")
                .join(TOOLCHAIN)
                .join("bin/rust-analyzer.exe");
            let state = root.path().join("state");
            Self {
                root,
                request: Request {
                    user_home: home,
                    state,
                },
                executable,
                components,
            }
        }

        fn install(&self, ops: &mut FakeOps) -> io::Result<Value> {
            install(&self.request, ops)
        }

        fn journal(&self) -> PathBuf {
            let mut found = Vec::new();
            let transactions = self.request.state.join("transactions");
            for owner in fs::read_dir(&transactions).unwrap() {
                let owner = owner.unwrap().path();
                if owner.is_dir() {
                    for journal in fs::read_dir(&owner).unwrap() {
                        found.push(journal.unwrap().path());
                    }
                }
            }
            assert_eq!(found.len(), 1, "one journal per installation");
            found.pop().unwrap()
        }
    }

    #[test]
    fn missing_component_installs_with_committed_journal() {
        let fixture = Fixture::new();
        let mut ops = FakeOps::new(&fixture);
        let result = fixture.install(&mut ops).unwrap();
        assert_eq!(result["state"], "installed-unverified");
        assert_eq!(result["version"], "1.98.1");
        assert_eq!(result["packages_acquired"], true);
        assert_eq!(
            result["source"],
            "https://static.rust-lang.org/dist/rust-analyzer.tar.xz"
        );
        assert!(ops.added);
        assert!(fixture.executable.is_file());
        assert_eq!(
            fs::read_to_string(&fixture.components).unwrap(),
            format!("rustc\nrust-std\n{COMPONENT}\n")
        );
        let journal: Value = serde_json::from_slice(&fs::read(fixture.journal()).unwrap()).unwrap();
        assert_eq!(journal["phase"], "committed");
        assert_eq!(journal["component"], COMPONENT);
        assert!(
            fixture
                .request
                .state
                .join("locks")
                .read_dir()
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn existing_analyzer_and_missing_prerequisites_are_preserved() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.executable.parent().unwrap()).unwrap();
        fs::write(&fixture.executable, b"existing analyzer").unwrap();
        let result = fixture.install(&mut FakeOps::new(&fixture)).unwrap();
        assert_eq!(result["state"], "pending");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .contains("identity repair")
        );
        assert_eq!(fs::read(&fixture.executable).unwrap(), b"existing analyzer");

        let bare = tempfile::tempdir().unwrap();
        let request = Request {
            user_home: bare.path().join("user"),
            state: bare.path().join("state"),
        };
        let result = install(&request, &mut FakeOps::new(&fixture)).unwrap();
        assert_eq!(result["state"], "prerequisite-missing");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .contains("no toolchain will be installed")
        );
    }

    #[test]
    fn invalid_toolchain_and_unavailable_targets_are_refused() {
        let fixture = Fixture::new();
        let settings = fixture.request.user_home.join(".rustup/settings.toml");
        fs::write(&settings, "default_toolchain = \"stable/../escape\"\n").unwrap();
        let error = fixture.install(&mut FakeOps::new(&fixture)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Invalid selected rustup toolchain")
        );

        let fixture = Fixture::new();
        let manifest = fixture
            .request
            .user_home
            .join(".rustup/toolchains")
            .join(TOOLCHAIN)
            .join("lib/rustlib/multirust-channel-manifest.toml");
        fs::write(
            &manifest,
            "[pkg.rust]\nversion = \"1.98.1\"\n\n[pkg.rust-analyzer-preview.target.x86_64-pc-windows-msvc]\navailable = false\n",
        )
        .unwrap();
        let result = fixture.install(&mut FakeOps::new(&fixture)).unwrap();
        assert_eq!(result["state"], "prerequisite-missing");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .contains("no available declared analyzer")
        );
    }

    #[test]
    fn recovery_honors_commitments_and_preserves_changed_cohorts() {
        let fixture = Fixture::new();
        fixture.install(&mut FakeOps::new(&fixture)).unwrap();
        let journal_path = fixture.journal();
        let committed = recover_journal(&journal_path, false).unwrap();
        assert_eq!(committed["state"], "committed");
        assert!(fixture.executable.is_file());

        fs::write(
            &fixture.components,
            format!("rustc\nrust-std\n{COMPONENT}\nextra\n"),
        )
        .unwrap();
        let pending = recover_journal(&journal_path, true).unwrap();
        assert_eq!(pending["state"], "pending");
        assert!(
            pending["reason"]
                .as_str()
                .unwrap()
                .contains("changed; preserve it")
        );

        let mut foreign: Value = serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
        foreign["owner"] = json!("foreign-owner");
        fs::write(&journal_path, serde_json::to_vec_pretty(&foreign).unwrap()).unwrap();
        assert!(recover_journal(&journal_path, true).is_err());
    }
}
