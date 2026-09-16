//! Isolated Grok Responses probe CLI. Never prints access tokens.
#![cfg(windows)]

use harness_core::xai_responses_probe::{ProbeRequest, ProbeTransport, probe};
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

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] || args.first().is_some_and(|arg| arg == "--help") {
        println!(
            "codex-harness xai-responses-probe --user-home DIRECTORY --evidence DIRECTORY\nBounded live Grok Responses probe using harness xAI OAuth. Does not copy OpenCode tokens or print credentials. Isolated fixtures never target the live global proxy."
        );
        return Ok(0);
    }
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let key = arg.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native xai-responses-probe options",
            )
        })?;
        if !matches!(key, "--user-home" | "--evidence") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native xai-responses-probe options",
            ));
        }
        let value = iter.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native xai-responses-probe options",
            )
        })?;
        values.push((OsString::from(key), value.clone()));
    }
    let user_home = required(&values, "--user-home").or_else(|_| {
        env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--user-home is required"))
    })?;
    let evidence = required(&values, "--evidence").or_else(|_| {
        optional(&values, "--evidence")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--evidence is required"))
    })?;
    if !user_home.is_absolute() || !evidence.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "xai-responses-probe paths must be absolute",
        ));
    }
    let _ = Path::new(&user_home);
    let report = probe(&ProbeRequest {
        user_home,
        evidence,
        transport: ProbeTransport::Live,
    })?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(if report.ok { 0 } else { 2 })
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
        assert!(
            error
                .to_string()
                .contains("invalid native xai-responses-probe")
        );
    }
}
