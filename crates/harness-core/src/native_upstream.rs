//! Explicit management-time discovery. No shim is executed and ordinary launch
//! continues to consume the resulting pinned native registration.
#![cfg(windows)]

use crate::{
    build_identity,
    installation_state::{key as metadata_key, normal as absolute_normal},
    native_launcher::{Package, PackageManager, Upstream},
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

const LIMIT: usize = 64 * 1024;
const PLATFORM: &str = "@openai/codex-win32-x64";
const VENDOR_EXE: &str = "vendor/x86_64-pc-windows-msvc/bin/codex.exe";
// Normalized LF/BOM-free cmd-shim output inspected with installed CLI 0.153.4.
// Unknown/custom scripts require an explicit package entrypoint or native exe.
const PS1: &str = "9d6739bbe2308ba0527ee6dea3b2b50ed7e0229261e47b6ffe638112e9171f27";
const CMD: &str = "f9ac6982d587adf5e10b8b9a9eaf7e2c172f2fcad044d28e84289bb4960d2e46";

#[derive(Default)]
pub struct ManagerHints {
    pub user_agent: String,
    pub exec_path: String,
}

fn normal(path: &Path) -> io::Result<PathBuf> {
    let text = path
        .to_str()
        .ok_or_else(|| fail("upstream paths require Unicode"))?;
    absolute_normal(Path::new(text.strip_prefix("\\\\?\\").unwrap_or(text))).map_err(|_| {
        fail("upstream paths must be absolute local drive paths without parent traversal")
    })
}

fn key(path: &Path) -> io::Result<String> {
    metadata_key(&normal(path)?)
}

fn fail(message: &'static str) -> io::Error {
    io::Error::other(message)
}

fn read(path: &Path) -> io::Result<Vec<u8>> {
    if !fs::metadata(path)?.is_file() {
        return Err(fail("upstream metadata is not a file"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(fail("upstream metadata exceeds its bound"));
    }
    Ok(bytes)
}

fn json(path: &Path) -> io::Result<(Value, String)> {
    let bytes = read(path)?;
    let value =
        serde_json::from_slice(&bytes).map_err(|_| fail("invalid upstream package metadata"))?;
    Ok((value, build_identity::hash_bytes(&bytes)))
}

fn canonical(path: &Path) -> io::Result<PathBuf> {
    normal(path)?;
    let resolved = fs::canonicalize(path)?;
    normal(&resolved)
}

fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn ancestors(path: &Path) -> io::Result<Vec<&Path>> {
    let paths: Vec<_> = path.ancestors().take(65).collect();
    if paths.len() > 64 {
        return Err(fail("upstream lookup exceeds its ancestry bound"));
    }
    Ok(paths)
}

fn main_package(root: &Path) -> io::Result<Option<(Value, String)>> {
    let manifest = root.join("package.json");
    if !exists(&root.join("bin/codex.js"))? || !exists(&manifest)? {
        return Ok(None);
    }
    let (data, hash) = json(&manifest)?;
    let bin = data.get("bin").and_then(|v| {
        v.as_str()
            .or_else(|| v.get("codex").and_then(Value::as_str))
    });
    if data.get("name").and_then(Value::as_str) != Some("@openai/codex")
        || bin != Some("bin/codex.js")
    {
        return Ok(None);
    }
    if data
        .get("version")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .is_none()
    {
        return Err(fail("upstream package has no bounded version"));
    }
    if !fs::metadata(root.join("bin/codex.js"))?.is_file() {
        return Err(fail("upstream package entrypoint is missing"));
    }
    Ok(Some((data, hash)))
}

fn platform_executable(root: &Path, package: &Value) -> io::Result<PathBuf> {
    // Node's package lookup considers node_modules below each ancestor of bin,
    // skipping a node_modules/node_modules candidate. No directory recursion.
    for ancestor in ancestors(&root.join("bin"))? {
        if ancestor
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("node_modules"))
        {
            continue;
        }
        let platform = ancestor.join("node_modules").join(PLATFORM);
        let manifest = platform.join("package.json");
        if exists(&manifest)? {
            let (data, _) = json(&manifest)?;
            let version = package["version"].as_str().expect("checked main version");
            let current = data.get("name").and_then(Value::as_str) == Some("@openai/codex")
                && data.get("version").and_then(Value::as_str)
                    == Some(format!("{version}-win32-x64").as_str());
            let named = data.get("name").and_then(Value::as_str) == Some(PLATFORM)
                && data.get("version").and_then(Value::as_str) == Some(version);
            if !current && !named {
                return Err(fail("upstream platform package identity/version conflicts"));
            }
            // A resolved but broken optional package must not use vendor fallback.
            return canonical(&platform.join(VENDOR_EXE));
        }
    }
    canonical(&root.join(VENDOR_EXE))
}

fn same(path: &Path, root: &Path) -> bool {
    canonical(path).ok().and_then(|p| key(&p).ok()) == key(root).ok()
}

fn vite_owned(directory: &Path, root: &Path) -> io::Result<bool> {
    if !directory
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("packages"))
    {
        return Ok(false);
    }
    let marker = directory.join("@openai/codex.json");
    if !exists(&marker)? {
        return Ok(false);
    }
    let (data, _) = json(&marker)?;
    if data.get("name").and_then(Value::as_str) != Some("@openai/codex") {
        return Ok(false);
    }
    let id = data.get("installId").and_then(Value::as_str).unwrap_or("");
    if id.len() > 128 || id.contains(['/', '\\', ':', '\0']) || id == "." || id == ".." {
        return Err(fail("invalid upstream package-manager installation ID"));
    }
    let install = if id.starts_with('#') {
        directory.join(format!("@openai/codex{id}"))
    } else {
        directory.join("@openai/codex").join(id)
    };
    Ok([
        "lib/node_modules/@openai/codex",
        "node_modules/@openai/codex",
    ]
    .iter()
    .any(|p| same(&install.join(p), root)))
}

fn manager(root: &Path, entry: &Path, hints: &ManagerHints) -> io::Result<PackageManager> {
    for start in [root, entry.parent().unwrap_or(entry)] {
        for ancestor in ancestors(start)? {
            if vite_owned(ancestor, root)? {
                return Ok(PackageManager::VitePlus);
            }
            let modules = ancestor.join("node_modules");
            if exists(&modules.join(".modules.yaml"))? && same(&modules.join("@openai/codex"), root)
            {
                return Ok(PackageManager::Pnpm);
            }
        }
    }
    // Match the official entrypoint's fallback hints only after filesystem
    // ownership. Callers supply these separately; this module reads no ambient env.
    let bun_agent = hints.user_agent.match_indices("bun/").any(|(index, _)| {
        index == 0
            || !hints.user_agent.as_bytes()[index - 1].is_ascii_alphanumeric()
                && hints.user_agent.as_bytes()[index - 1] != b'_'
    });
    let spelling = root
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if bun_agent || hints.exec_path.contains("bun") || spelling.contains("/.bun/install/global/") {
        Ok(PackageManager::Bun)
    } else {
        Ok(PackageManager::Npm)
    }
}

fn package_for_executable(
    executable: &Path,
    entry: &Path,
    hints: &ManagerHints,
) -> io::Result<Option<Package>> {
    let mut roots = BTreeSet::new();
    let mut matches = Vec::new();
    for ancestor in ancestors(
        executable
            .parent()
            .ok_or_else(|| fail("upstream has no parent"))?,
    )? {
        let mut candidates = vec![ancestor.to_path_buf()];
        if ancestor
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("node_modules"))
        {
            candidates.push(ancestor.join("@openai/codex"));
        }
        for root in candidates {
            let Some((data, manifest_sha256)) = main_package(&root)? else {
                continue;
            };
            let root = canonical(&root)?;
            if !roots.insert(key(&root)?) {
                continue;
            }
            if key(&platform_executable(&root, &data)?)? == key(executable)? {
                matches.push(Package {
                    manager: manager(&root, entry, hints)?,
                    root,
                    manifest_sha256,
                });
            }
        }
    }
    if matches.len() > 1 {
        return Err(fail(
            "multiple upstream packages own this executable; select a package entrypoint explicitly",
        ));
    }
    Ok(matches.pop())
}

fn candidate(entry: &Path, hints: &ManagerHints) -> io::Result<Upstream> {
    normal(entry)?;
    let resolved = canonical(entry)?;
    let package_root = if resolved.is_dir() {
        Some(resolved.clone())
    } else if resolved
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("codex.js"))
        && resolved
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n.eq_ignore_ascii_case("bin"))
    {
        resolved
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    } else if resolved
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ps1") || e.eq_ignore_ascii_case("cmd"))
    {
        let bytes = read(&resolved)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| fail("unsupported upstream shim encoding"))?
            .trim_start_matches('\u{feff}')
            .replace("\r\n", "\n");
        let hash = build_identity::hash_bytes(text.trim().as_bytes());
        if hash != PS1 && hash != CMD {
            return Err(fail(
                "unknown/custom upstream shim; select its Codex package entrypoint or native executable explicitly",
            ));
        }
        Some(
            resolved
                .parent()
                .ok_or_else(|| fail("upstream shim has no parent"))?
                .join("node_modules/@openai/codex"),
        )
    } else {
        None
    };
    let (executable, package) = if let Some(root) = package_root {
        let root = canonical(&root)?;
        let (data, manifest_sha256) = main_package(&root)?
            .ok_or_else(|| fail("selected entry is not a supported Codex package"))?;
        let executable = platform_executable(&root, &data)?;
        let package = Package {
            manager: manager(&root, entry, hints)?,
            root,
            manifest_sha256,
        };
        (executable, Some(package))
    } else {
        if !resolved
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            || !fs::metadata(&resolved)?.is_file()
        {
            return Err(fail(
                "upstream must select a native executable or recognized Codex package",
            ));
        }
        let package = package_for_executable(&resolved, entry, hints)?;
        (resolved, package)
    };
    let sha256 = build_identity::hash_file(&executable)?;
    Ok(Upstream {
        executable,
        sha256,
        package,
    })
}

/// Resolve only at explicit install/update time. Exclusions include the current
/// build and installed harness aliases. Empty/relative PATH entries are ignored;
/// the caller's working directory never becomes an implicit upstream source.
pub fn resolve(
    explicit: Option<&Path>,
    search_path: Option<&OsStr>,
    excluded: &[PathBuf],
    hints: &ManagerHints,
) -> io::Result<Upstream> {
    if hints.user_agent.len() > LIMIT || hints.exec_path.len() > LIMIT {
        return Err(fail("upstream package-manager hints exceed their bound"));
    }
    let mut paths = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    let mut hashed_paths = BTreeSet::new();
    for path in excluded {
        paths.insert(key(path)?);
        if exists(path)? {
            let target = match canonical(path) {
                Ok(target) => target,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let target_key = key(&target)?;
            paths.insert(target_key.clone());
            if hashed_paths.insert(target_key) && fs::metadata(&target)?.is_file() {
                hashes.insert(build_identity::hash_file(&target)?);
            }
        }
    }
    let allowed = |upstream: &Upstream| -> io::Result<bool> {
        Ok(!paths.contains(&key(&upstream.executable)?) && !hashes.contains(&upstream.sha256))
    };
    if let Some(entry) = explicit {
        if paths.contains(&key(entry)?) {
            return Err(fail("upstream selects a harness entrypoint"));
        }
        let upstream = candidate(entry, hints)?;
        if !allowed(&upstream)? {
            return Err(fail("upstream selects a harness executable"));
        }
        return Ok(upstream);
    }
    let search_path = search_path.ok_or_else(|| fail("no upstream selected and PATH is absent"))?;
    let directories: Vec<_> = std::env::split_paths(search_path).collect();
    if directories.len() > 256 {
        return Err(fail("upstream PATH lookup exceeds its bound"));
    }
    for directory in directories {
        if directory.as_os_str().is_empty() || !directory.is_absolute() {
            continue;
        }
        let mut found: Option<Upstream> = None;
        for name in ["codex.ps1", "codex.exe", "codex.cmd"] {
            let entry = directory.join(name);
            if paths.contains(&key(&entry)?) || !exists(&entry)? {
                continue;
            }
            let upstream = candidate(&entry, hints)?;
            if !allowed(&upstream)? {
                continue;
            }
            if found.as_ref().is_some_and(|old| {
                old.executable != upstream.executable || old.sha256 != upstream.sha256
            }) {
                return Err(fail(
                    "ambiguous Codex commands in one PATH directory; select the intended upstream explicitly",
                ));
            }
            found = Some(upstream);
        }
        if let Some(upstream) = found {
            return Ok(upstream);
        }
    }
    Err(fail(
        "original Codex executable was not found; select --upstream explicitly",
    ))
}

#[cfg(test)]
#[path = "native_upstream_tests.rs"]
mod tests;
