//! Declared external-language analysis samples. These files may contain Python
//! or other syntax for a foreign analyzer, but they are test data and must never
//! be launched as harness helpers.
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

/// Relative checkout paths that a foreign analyzer may inspect without treating
/// the file as a first-party executable helper.
pub const DECLARED: &[&str] = &[
    "tests/fixtures/lsp/delayed-mcp.py",
    "tests/fixtures/lsp/mcp-edit.py",
];

pub fn declared_paths(root: &Path) -> Vec<PathBuf> {
    DECLARED.iter().map(|name| root.join(name)).collect()
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/")
}

fn relative_key(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|value| normalize(&value.to_string_lossy()))
}

fn declared_set() -> BTreeSet<String> {
    DECLARED.iter().map(|name| normalize(name)).collect()
}

/// True when `path` is one of the declared analysis samples under `root`.
pub fn is_declared_sample(root: &Path, path: &Path) -> bool {
    match relative_key(root, path) {
        Some(key) => declared_set().contains(&key),
        None => false,
    }
}

fn sample_suffix(path: &Path) -> bool {
    let normalized = normalize(&path.to_string_lossy()).to_ascii_lowercase();
    DECLARED
        .iter()
        .any(|name| normalized.ends_with(&name.to_ascii_lowercase()))
}

/// True when `path` is a declared analysis sample, even without a checkout root.
pub fn is_sample_program(path: &Path) -> bool {
    sample_suffix(path)
}

/// Refuse to launch a declared analysis sample as a harness helper. Native
/// helpers remain `.exe` files; samples stay data even when they have a shebang.
pub fn refuse_helper_launch(root: &Path, program: &Path) -> io::Result<()> {
    if is_declared_sample(root, program) || is_sample_program(program) {
        return Err(io::Error::other(
            "inert analysis sample cannot execute as a harness helper",
        ));
    }
    Ok(())
}

/// Refuse a helper program that is itself a declared analysis sample.
pub fn refuse_sample_program(program: &Path) -> io::Result<()> {
    if is_sample_program(program) {
        return Err(io::Error::other(
            "inert analysis sample cannot execute as a harness helper",
        ));
    }
    Ok(())
}

/// Absolute native helper executables keep their `.exe` identity. Declared
/// samples and other non-executables cannot satisfy that contract.
pub fn require_native_helper(program: &Path) -> io::Result<()> {
    if program
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        && program.is_absolute()
    {
        return Ok(());
    }
    Err(io::Error::other(
        "Native helper must be an absolute EXE; inert analysis samples cannot execute as harness helpers",
    ))
}

pub fn looks_like_python_sample(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
}
