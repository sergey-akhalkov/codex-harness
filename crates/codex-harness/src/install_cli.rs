//! Native install/update/check/recover/disconnect with mutually exclusive
//! component selectors. Combined activation remains unfinished.
use harness_core::{
    code_tools_lifecycle, core_install,
    lifecycle::{self, Component},
    subscription_lifecycle, token_workflow_lifecycle,
};
use std::env;
use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf, time::Duration};

#[derive(Debug)]
struct Options {
    values: BTreeMap<OsString, OsString>,
    component: Component,
    preview: bool,
}

fn options(args: &[OsString], allowed: &[&str], allow_preview: bool) -> io::Result<Options> {
    let mut values = BTreeMap::new();
    let mut component = None;
    let mut preview = false;
    let mut iter = args.iter();
    while let Some(key) = iter.next() {
        match key.to_str() {
            Some("--preview") if allow_preview && !preview => preview = true,
            Some(name) if lifecycle::parse_flag(name).is_some() => {
                component = Some(lifecycle::exclusive(
                    component,
                    lifecycle::parse_flag(name).expect("selector"),
                )?);
            }
            Some(name) if allowed.contains(&name) => {
                let value = iter
                    .next()
                    .ok_or_else(|| io::Error::other("missing native installation option value"))?;
                if values.insert(key.clone(), value.clone()).is_some() {
                    return Err(io::Error::other("duplicate native installation option"));
                }
            }
            _ => {
                return Err(io::Error::other(
                    "unknown or duplicate native installation option",
                ));
            }
        }
    }
    Ok(Options {
        values,
        component: lifecycle::required(component)?,
        preview,
    })
}

fn required(values: &BTreeMap<OsString, OsString>, name: &str) -> io::Result<PathBuf> {
    values
        .get(&OsString::from(name))
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other(format!("{name} is required")))
}

fn optional(values: &BTreeMap<OsString, OsString>, name: &str) -> Option<PathBuf> {
    values.get(&OsString::from(name)).map(PathBuf::from)
}

#[allow(dead_code)]
fn unfinished(component: Component, action: &str) -> io::Error {
    io::Error::other(format!(
        "native {action} for {} has not completed acceptance",
        component.flag()
    ))
}

pub fn recover(args: &[OsString]) -> io::Result<i32> {
    let options = options(
        args,
        &[
            "--codex-home",
            "--user-home",
            "--dependency-user-home",
            "--source",
        ],
        true,
    )?;
    let user = required(&options.values, "--user-home")?;
    let dependency =
        optional(&options.values, "--dependency-user-home").unwrap_or_else(|| user.clone());
    let codex = required(&options.values, "--codex-home")?;
    let report = match options.component {
        Component::Core => {
            if options.preview {
                serde_json::to_value(core_install::preview_recovery(&codex, &user, &dependency)?)?
            } else {
                serde_json::to_value(core_install::recover(&codex, &user, &dependency)?)?
            }
        }
        Component::CodeTools => {
            serde_json::to_value(code_tools_lifecycle::run(&code_tools_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: codex,
                user_home: user,
                dependency_user_home: dependency,
                mode: code_tools_lifecycle::Mode::Recover,
                preview: options.preview,
                manager: None,
            })?)?
        }
        Component::TokenWorkflow => serde_json::to_value(token_workflow_lifecycle::recover(
            &token_workflow_lifecycle::Request {
                source: optional(&options.values, "--source").unwrap_or_default(),
                codex_home: codex,
                user_home: user,
                preview: options.preview,
            },
        )?)?,
        Component::Subscriptions => serde_json::to_value(subscription_lifecycle::recover(
            &subscription_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: codex,
                user_home: user,
                preview: options.preview,
            },
        )?)?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

pub fn check(args: &[OsString]) -> io::Result<i32> {
    let options = options(
        args,
        &[
            "--codex-home",
            "--user-home",
            "--dependency-user-home",
            "--timeout-seconds",
            "--source",
        ],
        true,
    )?;
    let user = required(&options.values, "--user-home")?;
    let dependency =
        optional(&options.values, "--dependency-user-home").unwrap_or_else(|| user.clone());
    let report = match options.component {
        Component::Core => {
            if options.preview {
                return Err(io::Error::other(
                    "native core check does not support --preview",
                ));
            }
            let timeout = options
                .values
                .get(&OsString::from("--timeout-seconds"))
                .map(|value| {
                    value
                        .to_str()
                        .and_then(|text| text.parse::<u64>().ok())
                        .ok_or_else(|| io::Error::other("invalid native check timeout"))
                })
                .transpose()?
                .unwrap_or(45);
            serde_json::to_value(harness_core::core_check::check(
                &required(&options.values, "--codex-home")?,
                &user,
                &dependency,
                Duration::from_secs(timeout),
            )?)?
        }
        Component::CodeTools => {
            serde_json::to_value(code_tools_lifecycle::run(&code_tools_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: required(&options.values, "--codex-home")?,
                user_home: user,
                dependency_user_home: dependency,
                mode: code_tools_lifecycle::Mode::Check,
                preview: options.preview,
                manager: None,
            })?)?
        }
        Component::TokenWorkflow => serde_json::to_value(token_workflow_lifecycle::check(
            &token_workflow_lifecycle::Request {
                source: optional(&options.values, "--source").unwrap_or_default(),
                codex_home: required(&options.values, "--codex-home")?,
                user_home: user,
                preview: options.preview,
            },
        )?)?,
        Component::Subscriptions => serde_json::to_value(subscription_lifecycle::check(
            &subscription_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: required(&options.values, "--codex-home")?,
                user_home: user,
                preview: options.preview,
            },
        )?)?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

pub fn configure_restart(args: &[OsString]) -> io::Result<i32> {
    let options = options(args, &["--codex-home", "--user-home", "--source"], true)?;
    let report = match options.component {
        Component::Subscriptions => serde_json::to_value(
            subscription_lifecycle::configure_restart(&subscription_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: required(&options.values, "--codex-home")?,
                user_home: required(&options.values, "--user-home")?,
                preview: options.preview,
            })?,
        )?,
        other => {
            return Err(io::Error::other(format!(
                "configure-restart requires --subscriptions-only, not {}",
                other.flag()
            )));
        }
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

pub fn disconnect(args: &[OsString]) -> io::Result<i32> {
    let options = options(
        args,
        &[
            "--codex-home",
            "--user-home",
            "--dependency-user-home",
            "--source",
        ],
        true,
    )?;
    let user = required(&options.values, "--user-home")?;
    let dependency =
        optional(&options.values, "--dependency-user-home").unwrap_or_else(|| user.clone());
    let report = match options.component {
        Component::Core => serde_json::to_value(harness_core::core_disconnect::disconnect(
            &required(&options.values, "--codex-home")?,
            &user,
            &dependency,
            options.preview,
        )?)?,
        Component::CodeTools => {
            serde_json::to_value(code_tools_lifecycle::run(&code_tools_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: required(&options.values, "--codex-home")?,
                user_home: user,
                dependency_user_home: dependency,
                mode: code_tools_lifecycle::Mode::Disconnect,
                preview: options.preview,
                manager: None,
            })?)?
        }
        Component::TokenWorkflow => serde_json::to_value(token_workflow_lifecycle::disconnect(
            &token_workflow_lifecycle::Request {
                source: optional(&options.values, "--source").unwrap_or_default(),
                codex_home: required(&options.values, "--codex-home")?,
                user_home: user,
                preview: options.preview,
            },
        )?)?,
        Component::Subscriptions => serde_json::to_value(subscription_lifecycle::disconnect(
            &subscription_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: required(&options.values, "--codex-home")?,
                user_home: user,
                preview: options.preview,
            },
        )?)?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

pub fn run(command: &str, args: &[OsString]) -> io::Result<i32> {
    let options = options(
        args,
        &[
            "--source",
            "--build",
            "--codex-home",
            "--user-home",
            "--dependency-user-home",
            "--upstream",
            "--timeout-seconds",
            "--path-scope",
        ],
        true,
    )?;
    let report = match options.component {
        Component::Core => {
            let user_home = required(&options.values, "--user-home")?;
            let timeout = options
                .values
                .get(&OsString::from("--timeout-seconds"))
                .map(|v| {
                    v.to_str()
                        .and_then(|v| v.parse::<u64>().ok())
                        .ok_or_else(|| io::Error::other("invalid native installation timeout"))
                })
                .transpose()?
                .unwrap_or(45);
            serde_json::to_value(core_install::connect(
                &core_install::Request {
                    source: required(&options.values, "--source")?,
                    build: required(&options.values, "--build")?,
                    codex_home: required(&options.values, "--codex-home")?,
                    dependency_user_home: optional(&options.values, "--dependency-user-home")
                        .unwrap_or_else(|| user_home.clone()),
                    user_home,
                    upstream: optional(&options.values, "--upstream"),
                    timeout: Duration::from_secs(timeout),
                    path_scope: options
                        .values
                        .get(&OsString::from("--path-scope"))
                        .map(
                            |value| match value.to_str().map(str::to_ascii_lowercase).as_deref() {
                                Some("user") => {
                                    Ok(harness_core::installation_state::PathScope::User)
                                }
                                Some("process") => {
                                    Ok(harness_core::installation_state::PathScope::Process)
                                }
                                _ => Err(io::Error::other("PATH scope must be User or Process")),
                            },
                        )
                        .transpose()?,
                },
                options.preview,
            )?)?
        }
        Component::CodeTools => {
            serde_json::to_value(code_tools_lifecycle::run(&code_tools_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: required(&options.values, "--codex-home")?,
                user_home: required(&options.values, "--user-home")?,
                dependency_user_home: optional(&options.values, "--dependency-user-home")
                    .or_else(|| optional(&options.values, "--user-home"))
                    .ok_or_else(|| io::Error::other("--user-home is required"))?,
                mode: if command == "update" {
                    code_tools_lifecycle::Mode::Update
                } else {
                    code_tools_lifecycle::Mode::Install
                },
                preview: options.preview,
                manager: Some(env::current_exe()?),
            })?)?
        }
        Component::TokenWorkflow => serde_json::to_value(token_workflow_lifecycle::install(
            &token_workflow_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: required(&options.values, "--codex-home")?,
                user_home: required(&options.values, "--user-home")?,
                preview: options.preview,
            },
        )?)?,
        Component::Subscriptions => serde_json::to_value(subscription_lifecycle::install(
            &subscription_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: required(&options.values, "--codex-home")?,
                user_home: required(&options.values, "--user-home")?,
                preview: options.preview,
            },
        )?)?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn mutually_exclusive_selectors_are_rejected() {
        let error = options(
            &os(&["--core-only", "--code-tools-only", "--codex-home", "x"]),
            &["--codex-home"],
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn missing_selector_is_rejected() {
        let error = options(&os(&["--codex-home", "x"]), &["--codex-home"], true).unwrap_err();
        assert!(error.to_string().contains("explicit component selector"));
    }

    #[test]
    fn code_tools_only_update_is_no_longer_rejected_as_unsupported() {
        let error = run(
            "update",
            &os(&[
                "--code-tools-only",
                "--source",
                ".",
                "--codex-home",
                "x",
                "--user-home",
                "y",
            ]),
        )
        .unwrap_err();
        assert!(
            !error.to_string().contains("never updates dependencies"),
            "{error}"
        );
    }

    #[test]
    fn subscriptions_install_requires_source() {
        let error = run(
            "install",
            &os(&[
                "--subscriptions-only",
                "--codex-home",
                "x",
                "--user-home",
                "y",
            ]),
        )
        .unwrap_err();
        assert!(error.to_string().contains("--source is required"));
    }

    #[test]
    fn subscriptions_recover_requires_source() {
        let error = recover(&os(&[
            "--subscriptions-only",
            "--codex-home",
            "x",
            "--user-home",
            "y",
        ]))
        .unwrap_err();
        assert!(error.to_string().contains("--source is required"));
    }

    #[test]
    fn configure_restart_requires_subscriptions_only() {
        let error = configure_restart(&os(&[
            "--core-only",
            "--source",
            ".",
            "--codex-home",
            "x",
            "--user-home",
            "y",
        ]))
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("configure-restart requires --subscriptions-only")
        );
    }

    #[test]
    fn configure_restart_requires_source() {
        let error = configure_restart(&os(&[
            "--subscriptions-only",
            "--codex-home",
            "x",
            "--user-home",
            "y",
        ]))
        .unwrap_err();
        assert!(error.to_string().contains("--source is required"));
    }
}
