//! Codex `[model_providers.*.auth]` helper. Prints one access token.
#![cfg(windows)]

use harness_core::xai_token_helper::{emit_access_token, store_path};
use std::{env, ffi::OsString, io, path::PathBuf};

fn required(values: &[(OsString, OsString)], name: &str) -> io::Result<PathBuf> {
    values
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| PathBuf::from(value))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is required")))
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] || args.first().is_some_and(|arg| arg == "--help") {
        println!(
            "codex-harness xai-token --codex-home DIRECTORY\nPrint a short-lived xAI access token to stdout for Codex provider auth. Credentials stay in the harness store. Does not read OpenCode auth.json."
        );
        return Ok(0);
    }
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let key = arg.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native xai-token options",
            )
        })?;
        if key != "--codex-home" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native xai-token options",
            ));
        }
        let value = iter.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native xai-token options",
            )
        })?;
        values.push((OsString::from(key), value.clone()));
    }
    let home = required(&values, "--codex-home").or_else(|_| {
        env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".codex")))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--codex-home is required"))
    })?;
    if !home.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--codex-home must be absolute",
        ));
    }
    let _ = store_path(&home);
    emit_access_token(&home)?;
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
    fn unknown_option_is_rejected() {
        let error = run(&[OsString::from("--proxy")]).unwrap_err();
        assert!(error.to_string().contains("invalid native xai-token"));
    }
}
