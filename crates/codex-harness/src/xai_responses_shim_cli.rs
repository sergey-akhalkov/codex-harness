//! Local compatibility shim lifecycle host for `codex --profile xai`.
#![cfg(windows)]

use harness_core::xai_responses_shim::{self, Options};
use std::{ffi::OsString, io};

fn positive(value: &str) -> io::Result<u16> {
    value
        .parse::<u16>()
        .map_err(|_| io::Error::other("invalid native xai-responses-shim options"))
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.first().is_some_and(|arg| arg == "--help") {
        println!(
            "codex-harness xai-responses-shim [--port N]\nServe the kit-owned Codex <-> api.x.ai compatibility fix on 127.0.0.1: drop the `content: null` field api.x.ai rejects in echoed reasoning items and translate the `custom`, `namespace` and `external_web_access` shapes Codex sends. No credentials are stored; exits when no codex.exe process remains."
        );
        return Ok(0);
    }
    let mut options = Options::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let key = arg
            .to_str()
            .ok_or_else(|| io::Error::other("invalid native xai-responses-shim options"))?;
        let mut value = || {
            iter.next()
                .and_then(|value| value.to_str())
                .ok_or_else(|| io::Error::other("invalid native xai-responses-shim options"))
        };
        match key {
            "--port" => options.port = positive(value()?)?,
            "--upstream" => {
                let upstream = value()?;
                if upstream != "https://api.x.ai" && upstream != "https://api.x.ai/" {
                    return Err(io::Error::other(
                        "invalid native xai-responses-shim options",
                    ));
                }
                options.upstream = "https://api.x.ai".to_string();
            }
            _ => {
                return Err(io::Error::other(
                    "invalid native xai-responses-shim options",
                ));
            }
        }
    }
    xai_responses_shim::run(&options)?;
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
    fn foreign_upstream_is_rejected() {
        let error = run(&[
            OsString::from("--upstream"),
            OsString::from("https://example.invalid/"),
        ])
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid native xai-responses-shim")
        );
    }

    #[test]
    fn unknown_option_is_rejected() {
        let error = run(&[OsString::from("--proxy")]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid native xai-responses-shim")
        );
    }
}
