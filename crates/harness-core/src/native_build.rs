//! Explicit candidate preparation. This module never changes active registrations.
//! Compiler concurrency stays within the installation's fixed resource budget.
use crate::build_identity::{self, BINARIES, BuildRecord, SCHEMA};
use crate::process::{
    Cancellation, CommandSpec, Deadline, ExclusiveFileLock, Job, Limits, StopReason,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const OWNER: &[u8] = b"codex-harness-native-state-v1\n";
const TARGET: &str = "x86_64-pc-windows-msvc";

fn verify_compiled_inputs(
    target: &Path,
    source: &Path,
    inputs: &build_identity::SourceIdentity,
) -> io::Result<()> {
    let canonical_target = target.canonicalize()?;
    for binary in BINARIES {
        let depfile = target
            .join(TARGET)
            .join("release")
            .join(binary)
            .with_extension("d");
        ordinary_ancestors(&depfile)?;
        let dependencies = fs::read_to_string(depfile)?;
        let line = dependencies.lines().next().unwrap_or("");
        let (_, paths) = line.split_once(": ").ok_or_else(|| {
            io::Error::other("Unrecognized Cargo dep-info; candidate not accepted.")
        })?;
        let mut tokens = Vec::new();
        let mut token = String::new();
        let mut chars = paths.chars().peekable();
        while let Some(ch) = chars.next() {
            if (ch == '\\' && chars.peek().is_some_and(|c| c.is_whitespace() || *c == '#'))
                || (ch == '$' && chars.peek() == Some(&'$'))
            {
                token.push(chars.next().unwrap());
            } else if ch.is_whitespace() {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            } else {
                token.push(ch);
            }
        }
        if !token.is_empty() {
            tokens.push(token);
        }
        if tokens.is_empty() {
            return Err(io::Error::other("Missing Cargo compiled-input evidence."));
        }
        for token in tokens {
            let path = PathBuf::from(token);
            let path = if path.is_absolute() {
                path
            } else {
                source.join(path)
            };
            ordinary_ancestors(&path)?;
            let path = path.canonicalize()?;
            if let Ok(relative) = path.strip_prefix(source) {
                let key = relative.to_string_lossy().replace('\\', "/");
                if !inputs.files.contains_key(&key) {
                    return Err(io::Error::other(format!(
                        "Compiled input {key} is outside the native input inventory; place executable resources under the crate's src directory before explicit build."
                    )));
                }
            } else if !path.starts_with(&canonical_target) {
                return Err(io::Error::other(
                    "Compiled input is outside owned source/build roots; candidate not accepted.",
                ));
            }
        }
    }
    Ok(())
}

/// Serialize mutations without adopting an unknown or missing state directory.
pub(crate) fn lock_owned_state(state: &Path) -> io::Result<ExclusiveFileLock> {
    verify_owned_state(state)?;
    let path = state.join("build.lock");
    ordinary_ancestors(&path)?;
    ExclusiveFileLock::try_acquire(&path)?.ok_or_else(|| {
        io::Error::other("Another native operation owns this state; wait for it to finish.")
    })
}

pub(crate) fn verify_owned_state(state: &Path) -> io::Result<()> {
    ordinary_ancestors(state)?;
    let marker = state.join("owner");
    build_identity::ordinary(&marker)?;
    if fs::read(marker)? != OWNER {
        return Err(io::Error::other(
            "Native state has foreign ownership; preserving it.",
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct PreparedBuild {
    pub build: PathBuf,
    pub reused: bool,
    pub source_identity: String,
}

fn tool_version(tool: &OsStr, args: &[&str], source: &Path) -> io::Result<String> {
    let executable = resolve_tool(tool)?;
    let temp = tempfile::Builder::new()
        .prefix("harness-build-prerequisite-")
        .tempdir()?;
    let output_path = temp.path().join("stdout");
    let error_path = temp.path().join("stderr");
    let mut command = CommandSpec::new(&executable);
    command.args = args.iter().map(|value| (*value).into()).collect();
    command.current_dir = Some(source.to_owned());
    command.stdout = Some(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)?,
    );
    command.stderr = Some(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&error_path)?,
    );
    let job = Job::new(Limits {
        memory_bytes: Some(256 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })?;
    let child = job.spawn(&command)?;
    let outcome = job.wait(
        &child,
        Deadline::after(Duration::from_secs(15))?,
        &Cancellation::default(),
        Duration::from_secs(5),
    )?;
    drop(command);
    if outcome.reason != StopReason::Exited
        || outcome.exit_code != 0
        || fs::metadata(&output_path)?.len() > 16384
    {
        let retained = temp.keep();
        return Err(io::Error::other(format!(
            "Native build tool identity could not be established; bounded probe evidence: {}",
            retained.display()
        )));
    }
    fs::read_to_string(output_path).map(|s| s.trim().to_owned())
}

fn resolve_tool(tool: &OsStr) -> io::Result<PathBuf> {
    let requested = Path::new(tool);
    if requested.is_absolute() || requested.components().count() > 1 {
        // rustup dispatches by argv[0]: resolving rustc.exe/cargo.exe symlinks
        // to rustup.exe would silently select a different command.
        let resolved = std::path::absolute(requested)?;
        if resolved.is_file()
            && resolved
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            return Ok(resolved);
        }
    } else if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths).filter(|p| p.is_absolute()) {
            let mut candidate = directory.join(requested);
            if candidate.extension().is_none() {
                candidate.set_extension("exe");
            }
            if candidate.is_file()
                && candidate
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            {
                return Ok(candidate);
            }
        }
    }
    Err(io::Error::other(
        "Required native build executable is unavailable; install Rust/Cargo and the MSVC linker before explicit bootstrap.",
    ))
}

pub fn ordinary_ancestors(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(_) => build_identity::ordinary(ancestor)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Canonical spelling even when the leaf does not yet exist. On Windows,
/// comparing a verbatim canonical path against an ordinary absolute path would
/// miss ancestor/descendant overlap and permit build state inside the checkout.
fn state_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    ordinary_ancestors(&absolute)?;
    for ancestor in absolute.ancestors() {
        if ancestor.exists() {
            return Ok(ancestor
                .canonicalize()?
                .join(absolute.strip_prefix(ancestor).map_err(io::Error::other)?));
        }
    }
    Err(io::Error::other("Native state has no accessible ancestor."))
}

fn directory(path: &Path) -> io::Result<()> {
    ordinary_ancestors(path)?;
    fs::create_dir_all(path)?;
    build_identity::ordinary(path)?;
    if !path.is_dir() {
        return Err(io::Error::other(
            "Expected an ordinary native state directory.",
        ));
    }
    Ok(())
}

fn owner_root(state: &Path) -> io::Result<()> {
    ordinary_ancestors(state)?;
    let marker = state.join("owner");
    if state.exists() {
        build_identity::ordinary(state)?;
        if !state.is_dir() {
            return Err(io::Error::other(
                "Native state target is not a directory; preserving it.",
            ));
        }
        if marker.exists() {
            build_identity::ordinary(&marker)?;
            if fs::read(&marker)? != OWNER {
                return Err(io::Error::other(
                    "Native state has foreign ownership; preserving it.",
                ));
            }
            return Ok(());
        }
        if fs::read_dir(state)?.next().is_some() {
            return Err(io::Error::other(
                "Native state has no ownership record and is nonempty; preserving it.",
            ));
        }
    } else {
        directory(state)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)?;
    file.write_all(OWNER)?;
    file.sync_all()
}

fn find_reusable(
    state: &Path,
    source: &Path,
    rustc: &str,
    cargo: &str,
) -> io::Result<Option<PreparedBuild>> {
    let builds = state.join("builds");
    if !builds.exists() {
        return Ok(None);
    }
    directory(&builds)?;
    for entry in fs::read_dir(builds)? {
        let path = entry?.path();
        let Ok(record) = build_identity::read_record(&path) else {
            continue;
        };
        if record.source_root != source
            || record.rustc != rustc
            || record.cargo != cargo
            || record.target != TARGET
        {
            continue;
        }
        if build_identity::check(&path, Some(source)).runtime_allowed {
            return Ok(Some(PreparedBuild {
                build: path,
                reused: true,
                source_identity: record.source.sha256,
            }));
        }
    }
    Ok(None)
}

/// Build into installation-owned state, then publish an immutable candidate.
/// No Cargo command is reachable from `build_identity::check`.
pub fn prepare(source: &Path, state: &Path, cargo: &OsStr) -> io::Result<PreparedBuild> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err(io::Error::other(
            "Native bootstrap currently supports Windows x64 with MSVC.",
        ));
    }
    let source = source.canonicalize()?;
    let state = state_path(state)?;
    // Runtime data must not be mixed into the checkout or encompass its source.
    if state.starts_with(&source) || source.starts_with(&state) {
        return Err(io::Error::other(
            "Native build state must be outside the source checkout.",
        ));
    }
    let before = build_identity::source_identity(&source)?;
    let rustc = tool_version(OsStr::new("rustc"), &["-vV"], &source)?;
    if !rustc.lines().any(|line| line == format!("host: {TARGET}")) {
        return Err(io::Error::other(
            "Native bootstrap requires the x86_64-pc-windows-msvc Rust toolchain.",
        ));
    }
    let cargo_version = tool_version(cargo, &["--version"], &source)?;
    // Custom compiler wrappers and flags must get an explicit identity contract
    // before they can participate in an installation build.
    for (key, value) in std::env::vars_os() {
        let key = key.to_string_lossy().to_ascii_uppercase();
        let override_input = key.starts_with("RUSTC")
            || key == "RUSTFLAGS"
            || key.starts_with("CARGO_BUILD_")
            || key.starts_with("CARGO_TARGET_")
            || key.starts_with("CARGO_PROFILE_")
            || key.starts_with("CARGO_ALIAS_")
            || matches!(
                key.as_str(),
                "CARGO_ENCODED_RUSTFLAGS" | "CARGO_INCREMENTAL"
            );
        if override_input && !value.is_empty() {
            return Err(io::Error::other(format!(
                "Unsupported native bootstrap override {key}; use the declared toolchain without ambient overrides."
            )));
        }
    }
    owner_root(&state)?;
    let lock = lock_owned_state(&state)?;
    if let Some(reused) = find_reusable(&state, &source, &rustc, &cargo_version)? {
        return Ok(reused);
    }
    let sequence = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let id = format!("{}-{sequence}-{}", &before.sha256[..16], std::process::id());
    let staging_root = state.join("staging");
    directory(&staging_root)?;
    let staging = staging_root.join(&id);
    fs::create_dir(&staging)?;
    let log_path = staging.join("cargo.log");
    let log = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&log_path)?;
    // Never let Cargo's mtime cache certify a different content identity, and
    // never write into pre-existing cache descendants (which may be links).
    // Unchanged builds reuse their verified immutable artifacts above.
    // Keep compiler paths short: nested state/build IDs can exceed MSVC's
    // practical object-file path limit even when Rust accepts verbatim paths.
    let scratch = tempfile::Builder::new().prefix("hcb-").tempdir()?;
    let target = scratch.path().to_owned();
    ordinary_ancestors(&target)?;
    let mut command = CommandSpec::new(resolve_tool(cargo)?);
    command.args = [
        "build",
        "--release",
        "--locked",
        // Cargo otherwise scales rustc concurrency to host CPU count, which
        // does not reflect the fixed memory budget of the compiler job.
        "--jobs",
        "1",
        "--target",
        TARGET,
        "-p",
        "codex-harness",
        "-p",
        "harness-rtk",
        "--bins",
        "--manifest-path",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    command.args.extend([
        source.join("Cargo.toml").into_os_string(),
        "--target-dir".into(),
        target.as_os_str().to_owned(),
    ]);
    command.current_dir = Some(source.clone());
    command.stdout = Some(log.try_clone()?);
    command.stderr = Some(log);
    let job = Job::new(Limits {
        memory_bytes: Some(2048 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })?;
    let child = job
        .spawn(&command)
        .map_err(|e| io::Error::other(format!("Starting bounded Cargo failed: {e}")))?;
    let status = job.wait(
        &child,
        Deadline::after(Duration::from_secs(600))?,
        &Cancellation::default(),
        Duration::from_secs(5),
    )?;
    drop(command);
    if status.reason != StopReason::Exited || status.exit_code != 0 {
        return Err(io::Error::other(format!(
            "Candidate build failed (exit {}, {:?}); active installation preserved. See {}",
            status.exit_code,
            status.reason,
            log_path.display()
        )));
    }
    if build_identity::source_identity(&source)? != before {
        return Err(io::Error::other(
            "Native sources changed during the build; candidate not accepted. Retry explicit build with stable inputs.",
        ));
    }
    verify_compiled_inputs(&target, &source, &before)?;
    let mut binaries = BTreeMap::new();
    for name in BINARIES {
        let built = target.join(TARGET).join("release").join(name);
        ordinary_ancestors(&built)?;
        let candidate = staging.join(name);
        fs::copy(built, &candidate)
            .map_err(|e| io::Error::other(format!("Staging native binary {name} failed: {e}")))?;
        binaries.insert((*name).to_owned(), build_identity::hash_file(&candidate)?);
    }
    let record = BuildRecord {
        schema: SCHEMA,
        source_root: source.clone(),
        source: before,
        rustc,
        cargo: cargo_version,
        target: TARGET.into(),
        profile: "release".into(),
        binaries,
    };
    let mut receipt = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging.join("build.json"))?;
    receipt.write_all(&serde_json::to_vec_pretty(&record)?)?;
    receipt.sync_all()?;
    drop(receipt);
    if !build_identity::check(&staging, Some(&source)).runtime_allowed {
        return Err(io::Error::other(
            "Native candidate verification failed; active installation preserved.",
        ));
    }
    let builds = state.join("builds");
    directory(&builds)?;
    let published = builds.join(id);
    fs::rename(&staging, &published).map_err(|e| {
        io::Error::other(format!(
            "Publishing native candidate failed; retained {}: {e}",
            staging.display()
        ))
    })?;
    drop(lock);
    Ok(PreparedBuild {
        build: published,
        reused: false,
        source_identity: record.source.sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_state_is_preserved_and_owned_lock_serializes() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("foreign"), "keep").unwrap();
        assert!(owner_root(temp.path()).is_err());
        assert_eq!(
            fs::read_to_string(temp.path().join("foreign")).unwrap(),
            "keep"
        );
        let owned = temp.path().join("owned");
        owner_root(&owned).unwrap();
        owner_root(&owned).unwrap();
        let path = owned.join("build.lock");
        let a = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let b = OpenOptions::new().write(true).open(&path).unwrap();
        a.try_lock().unwrap();
        assert!(b.try_lock().is_err());
        drop(a);
        b.try_lock().unwrap();
    }
}
