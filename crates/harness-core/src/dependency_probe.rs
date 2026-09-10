//! Opt-in version read with the existing owned management process boundary.
#![cfg(windows)]
use crate::{
    dependency_package as package, native_build,
    process::{CommandSpec, StopReason},
};
use std::{io, path::Path, time::Duration};

pub(crate) fn rust_analyzer(path: &Path, expected_hash: &str) -> io::Result<String> {
    if package::fingerprint(path)? != expected_hash {
        return Err(package::invalid());
    }
    let private = tempfile::Builder::new()
        .prefix("harness-dependency-version-")
        .tempdir()?;
    let stdout = private.path().join("stdout");
    let stderr = private.path().join("stderr");
    let mut command = CommandSpec::new(path);
    command.args.push("--version".into());
    command.current_dir = Some(private.path().to_owned());
    // A mistakenly selected rustup proxy must neither acquire a missing
    // toolchain nor use the caller's mutable Rust/Cargo homes.
    command
        .env
        .insert("RUSTUP_AUTO_INSTALL".into(), Some("0".into()));
    command.env.insert(
        "RUSTUP_HOME".into(),
        Some(private.path().join("rustup").into_os_string()),
    );
    command.env.insert(
        "CARGO_HOME".into(),
        Some(private.path().join("cargo").into_os_string()),
    );
    command.env.insert("RUSTUP_TOOLCHAIN".into(), None);
    let outcome =
        native_build::invoke_management(command, &stderr, Some(&stdout), Duration::from_secs(8))?;
    if outcome.reason != StopReason::Exited
        || outcome.exit_code != 0
        || package::fingerprint(path)? != expected_hash
    {
        return Err(package::invalid());
    }
    if std::fs::metadata(&stdout)?.len() > 16 * 1024
        || std::fs::metadata(&stderr)?.len() > 16 * 1024
    {
        return Err(package::invalid());
    }
    let output = package::read_text(&stdout)?.ok_or_else(package::invalid)?;
    let line = output.lines().next().ok_or_else(package::invalid)?.trim();
    if !line.starts_with("rust-analyzer ")
        || line.len() > 200
        || !line
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b" .()-_+".contains(&byte))
        || !line.bytes().any(|byte| byte.is_ascii_digit())
    {
        return Err(package::invalid());
    }
    let result = line.to_owned();
    private.close()?;
    Ok(result)
}
