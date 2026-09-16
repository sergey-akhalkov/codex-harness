//! Native subscription login CLI. Isolated fixtures never target the live proxy.
#![cfg(windows)]

use harness_core::subscription_login::{self, LoginRequest};
use std::{
    env,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

fn required(values: &[(OsString, OsString)], name: &str) -> io::Result<PathBuf> {
    values
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| PathBuf::from(value))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is required")))
}

fn optional(values: &[(OsString, OsString)], name: &str) -> Option<PathBuf> {
    values
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| PathBuf::from(value))
}

fn flag(args: &[OsString], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] || args.first().is_some_and(|arg| arg == "--help") {
        println!(
            "codex-harness subscription-login xai|zai --source CHECKOUT --codex-home DIRECTORY --user-home DIRECTORY [--package-root DIRECTORY] [--key-file FILE] [--no-open-browser]\nBrowser-only xAI OAuth or host-private Z.AI key login. xAI credentials stay in the harness store. Does not require an OpenCodex package or OpenCode auth.json. Isolated fixtures never target the live global proxy."
        );
        return Ok(0);
    }
    let provider = args
        .first()
        .and_then(|arg| arg.to_str())
        .filter(|value| *value == "xai" || *value == "zai")
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native subscription-login options",
            )
        })?;
    let mut values = Vec::new();
    let mut iter = args[1..].iter();
    while let Some(arg) = iter.next() {
        let key = arg.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native subscription-login options",
            )
        })?;
        if key == "--no-open-browser" {
            continue;
        }
        if !matches!(
            key,
            "--source"
                | "--codex-home"
                | "--user-home"
                | "--package-root"
                | "--key-file"
                | "--evidence"
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native subscription-login options",
            ));
        }
        let value = iter.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native subscription-login options",
            )
        })?;
        values.push((OsString::from(key), value.clone()));
    }
    let source = required(&values, "--source").or_else(|_| {
        env::current_dir()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "--source is required"))
    })?;
    let request = LoginRequest {
        provider: provider.to_owned(),
        source,
        codex_home: required(&values, "--codex-home").or_else(|_| {
            env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .or_else(|| env::var_os("USERPROFILE").map(|home| Path::new(&home).join(".codex")))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--codex-home is required")
                })
        })?,
        user_home: required(&values, "--user-home").or_else(|_| {
            env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--user-home is required")
                })
        })?,
        package_root: optional(&values, "--package-root"),
        key_file: optional(&values, "--key-file"),
        open_browser: !flag(args, "--no-open-browser"),
        evidence: optional(&values, "--evidence"),
    };
    let report = subscription_login::login(&request)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_is_accepted() {
        assert_eq!(run(&[OsString::from("--help")]).unwrap(), 0);
    }

    #[test]
    fn unknown_provider_is_rejected() {
        let error = run(&[OsString::from("openai")]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid native subscription-login")
        );
    }
}
