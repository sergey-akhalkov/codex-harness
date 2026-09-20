//! Native install/update/check/recover/disconnect with mutually exclusive
//! component selectors. Combined activation remains unfinished.
use harness_core::{
    board_lifecycle, code_tools_lifecycle, core_install, installation_reset,
    lifecycle::{self, Component},
    native_build, subscription_lifecycle, token_workflow_lifecycle,
};
use std::env;
use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf, time::Duration};
use std::{path::Path, process::Command};

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
                manager: None,
            },
        )?)?,
        Component::Board => {
            serde_json::to_value(board_lifecycle::recover(&board_lifecycle::Request {
                source: optional(&options.values, "--source").unwrap_or_default(),
                codex_home: codex,
                user_home: user,
                preview: options.preview,
            })?)?
        }
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
                manager: None,
            },
        )?)?,
        Component::Board => {
            serde_json::to_value(board_lifecycle::check(&board_lifecycle::Request {
                source: optional(&options.values, "--source").unwrap_or_default(),
                codex_home: required(&options.values, "--codex-home")?,
                user_home: user,
                preview: options.preview,
            })?)?
        }
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
                manager: None,
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
                manager: None,
            },
        )?)?,
        Component::Board => {
            serde_json::to_value(board_lifecycle::disconnect(&board_lifecycle::Request {
                source: optional(&options.values, "--source").unwrap_or_default(),
                codex_home: required(&options.values, "--codex-home")?,
                user_home: user,
                preview: options.preview,
            })?)?
        }
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
            let source = required(&options.values, "--source")?;
            absolute_source(&source)?;
            // A named build is explicit; otherwise deliver the freshest
            // verified build of the selected source in this manager's owned
            // state, so new sessions pick up a fresh manager without replacing
            // one that is still running.
            let build = match optional(&options.values, "--build") {
                Some(build) => build,
                None => native_build::delivery_build(&source)?,
            };
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
                    source,
                    build,
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
            let codex_home = required(&options.values, "--codex-home")?;
            // The MCP registrations name the stable manager link, not this
            // build: a new Codex CLI session then resolves whatever manager the
            // last Install/Update delivered, and running an older manager can
            // no longer pin itself for later sessions.
            let manager = codex_home.join("harness/bin/codex-harness.exe");
            serde_json::to_value(code_tools_lifecycle::run(&code_tools_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home,
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
                manager: Some(manager),
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
                manager: Some(env::current_exe()?),
            },
        )?)?,
        Component::Board => {
            serde_json::to_value(board_lifecycle::install(&board_lifecycle::Request {
                source: required(&options.values, "--source")?,
                codex_home: required(&options.values, "--codex-home")?,
                user_home: required(&options.values, "--user-home")?,
                preview: options.preview,
            })?)?
        }
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

fn absolute_source(source: &Path) -> io::Result<()> {
    if source.is_absolute() {
        Ok(())
    } else {
        Err(io::Error::other(
            "deploy/install --source must be an absolute checkout path",
        ))
    }
}

/// Lifecycle path checks reject verbatim (`\\?\`) spelling; candidates from
/// `native_build::prepare` carry it, so normalize before connecting.
fn plain_path(path: PathBuf) -> PathBuf {
    match path.to_str() {
        Some(text) if text.starts_with(r"\\?\") => PathBuf::from(&text[4..]),
        _ => path,
    }
}

/// The state this manager itself runs from, so a deploy builds into the
/// installation's existing build history instead of a guessed location.
fn current_state() -> io::Result<PathBuf> {
    let executable = env::current_exe()?.canonicalize()?;
    let plain = executable
        .to_str()
        .and_then(|path| path.strip_prefix(r"\\?\"))
        .unwrap_or_default()
        .to_owned();
    let executable = if plain.is_empty() {
        executable
    } else {
        PathBuf::from(plain)
    };
    let build = executable
        .parent()
        .ok_or_else(|| io::Error::other("deploy manager has no build directory"))?;
    let builds = build
        .parent()
        .ok_or_else(|| io::Error::other("deploy requires --state or --build"))?;
    if builds.file_name().and_then(|name| name.to_str()) != Some("builds") {
        return Err(io::Error::other("deploy requires --state or --build"));
    }
    builds
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("deploy requires --state or --build"))
}

fn verification(codex_home: &Path) -> serde_json::Value {
    let installed = codex_home.join("harness/bin/codex-harness.exe");
    let version = Command::new(&installed).arg("--version").output();
    let probe = Command::new(&installed)
        .args(["executor", "--help"])
        .output();
    let version = match version {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        _ => String::new(),
    };
    let probe_ok = probe.is_ok_and(|output| {
        output.status.success()
            && String::from_utf8_lossy(&output.stdout).starts_with("codex-harness executor spawn")
    });
    serde_json::json!({
        "installed": installed,
        "version": version,
        "executor_probe_ok": probe_ok,
    })
}

/// Arguments for one `deploy --all` component step. The step is executed by
/// the delivered manager, so the mapping must stay aligned with the component
/// commands in [`run`]: `code-tools` uses `update` (never re-acquires
/// packages) and is the only step that consumes a dependency user home.
fn component_step_args(
    name: &str,
    source: &Path,
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> (&'static str, Vec<OsString>) {
    let mut arguments = vec![
        OsString::from(format!("--{name}-only")),
        OsString::from("--source"),
        source.as_os_str().to_owned(),
        OsString::from("--codex-home"),
        codex_home.as_os_str().to_owned(),
        OsString::from("--user-home"),
        user_home.as_os_str().to_owned(),
    ];
    let verb = if name == "code-tools" {
        arguments.push(OsString::from("--dependency-user-home"));
        arguments.push(dependency_user_home.as_os_str().to_owned());
        "update"
    } else {
        "install"
    };
    (verb, arguments)
}

/// One-action delivery: build a candidate, install or update the core
/// connection, optionally repair blocked ownership first, optionally chain
/// the scoped components, and verify the installed launcher.
pub fn deploy(args: &[OsString]) -> io::Result<i32> {
    let mut values: BTreeMap<OsString, OsString> = BTreeMap::new();
    let mut preview = false;
    let mut reset = false;
    let mut all = false;
    let mut iter = args.iter();
    while let Some(key) = iter.next() {
        match key.to_str() {
            Some("--preview") if !preview => preview = true,
            Some("--reset") if !reset => reset = true,
            Some("--all") if !all => all = true,
            Some(name)
                if [
                    "--source",
                    "--build",
                    "--state",
                    "--codex-home",
                    "--user-home",
                    "--dependency-user-home",
                    "--upstream",
                    "--timeout-seconds",
                    "--path-scope",
                    "--cargo",
                ]
                .contains(&name) =>
            {
                let value = iter
                    .next()
                    .ok_or_else(|| io::Error::other("missing deploy option value"))?;
                if values.insert(key.clone(), value.clone()).is_some() {
                    return Err(io::Error::other("duplicate deploy option"));
                }
            }
            _ => return Err(io::Error::other("unknown or duplicate deploy option")),
        }
    }
    let optional = |name: &str| values.get(&OsString::from(name)).map(PathBuf::from);
    let required =
        |name: &str| optional(name).ok_or_else(|| io::Error::other(format!("{name} is required")));
    let source = required("--source")?;
    absolute_source(&source)?;
    let user_home = match optional("--user-home") {
        Some(home) => home,
        None => PathBuf::from(
            env::var_os("USERPROFILE")
                .ok_or_else(|| io::Error::other("--user-home is required"))?,
        ),
    };
    let codex_home = match optional("--codex-home") {
        Some(home) => home,
        None => user_home.join(".codex"),
    };
    let dependency_user_home =
        optional("--dependency-user-home").unwrap_or_else(|| user_home.clone());
    let timeout = values
        .get(&OsString::from("--timeout-seconds"))
        .map(|value| {
            value
                .to_str()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| io::Error::other("invalid deploy timeout"))
        })
        .transpose()?
        .unwrap_or(45);
    let build = match optional("--build") {
        Some(build) => build,
        None => {
            let state = match optional("--state") {
                Some(state) => state,
                None => current_state()?,
            };
            let cargo = values
                .get(&OsString::from("--cargo"))
                .cloned()
                .unwrap_or_else(|| "cargo".into());
            plain_path(native_build::prepare(&source, &state, &cargo)?.build)
        }
    };
    let build = plain_path(build);
    let build_identity = harness_core::build_identity::read_record(&build)
        .map(|record| record.source.sha256.chars().take(16).collect::<String>())
        .unwrap_or_default();
    let reset_report = if reset && !preview {
        Some(serde_json::to_value(installation_reset::run(
            &codex_home,
            &user_home,
        )?)?)
    } else {
        None
    };
    let core = serde_json::to_value(core_install::connect(
        &core_install::Request {
            source: source.clone(),
            build: build.clone(),
            codex_home: codex_home.clone(),
            dependency_user_home: dependency_user_home.clone(),
            user_home: user_home.clone(),
            upstream: optional("--upstream"),
            timeout: Duration::from_secs(timeout),
            path_scope: values
                .get(&OsString::from("--path-scope"))
                .map(
                    |value| match value.to_str().map(str::to_ascii_lowercase).as_deref() {
                        Some("user") => Ok(harness_core::installation_state::PathScope::User),
                        Some("process") => Ok(harness_core::installation_state::PathScope::Process),
                        _ => Err(io::Error::other("PATH scope must be User or Process")),
                    },
                )
                .transpose()?,
        },
        preview,
    )?)?;
    let mut components = Vec::new();
    let mut status = if preview { "preview" } else { "deployed" };
    if all && !preview {
        let manager = codex_home.join("harness/bin/codex-harness.exe");
        // Component steps run through the manager this deploy just delivered,
        // not through this possibly older process: their behavior then always
        // matches the delivered source, and a component fix in that source
        // takes effect in the same delivery run.
        for name in ["code-tools", "token-workflow", "board", "subscriptions"] {
            let (verb, arguments) = component_step_args(
                name,
                &source,
                &codex_home,
                &user_home,
                &dependency_user_home,
            );
            let mut step = Command::new(&manager);
            step.arg(verb).args(&arguments);
            match step.output() {
                Ok(output) => {
                    if !output.stderr.is_empty() {
                        eprint!("{}", String::from_utf8_lossy(&output.stderr));
                    }
                    let outcome = if output.status.success() {
                        serde_json::from_slice::<serde_json::Value>(&output.stdout)
                            .map(|report| {
                                serde_json::json!({"name": name, "ok": true, "report": report})
                            })
                            .map_err(|error| {
                                format!("delivered manager report is not JSON: {error}")
                            })
                    } else {
                        let message = String::from_utf8_lossy(&output.stderr);
                        let message = message.trim();
                        Err(if message.is_empty() {
                            format!("delivered manager exited with {}", output.status)
                        } else {
                            message.to_owned()
                        })
                    };
                    match outcome {
                        Ok(component) => components.push(component),
                        Err(error) => {
                            components.push(serde_json::json!({
                                "name": name,
                                "ok": false,
                                "error": error
                            }));
                            status = "partial";
                            break;
                        }
                    }
                }
                Err(error) => {
                    components.push(serde_json::json!({
                        "name": name,
                        "ok": false,
                        "error": format!("starting delivered manager failed: {error}")
                    }));
                    status = "partial";
                    break;
                }
            }
        }
    }
    let verify = if preview {
        None
    } else {
        Some(verification(&codex_home))
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": status,
            "source": source,
            "build": build,
            "build_identity": build_identity,
            "reset": reset_report,
            "core": core,
            "components": components,
            "verification": verify,
        }))?
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn deploy_rejects_a_relative_source_before_any_work() {
        let error = deploy(&os(&["--source", "kit"])).unwrap_err();
        assert!(error.to_string().contains("absolute checkout path"));
    }

    #[test]
    fn deploy_component_steps_select_component_commands() {
        let source = Path::new("D:/kit");
        let codex_home = Path::new("C:/users/u/.codex");
        let user_home = Path::new("C:/users/u");
        let dependency_user_home = Path::new("D:/dependency-home");
        for (name, verb, selector) in [
            ("code-tools", "update", "--code-tools-only"),
            ("token-workflow", "install", "--token-workflow-only"),
            ("board", "install", "--board-only"),
            ("subscriptions", "install", "--subscriptions-only"),
        ] {
            let (step_verb, arguments) =
                component_step_args(name, source, codex_home, user_home, dependency_user_home);
            assert_eq!(step_verb, verb);
            let arguments: Vec<String> = arguments
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect();
            let position = arguments
                .iter()
                .position(|value| value == selector)
                .unwrap_or_else(|| panic!("{name} selector missing: {arguments:?}"));
            assert_eq!(arguments[position + 1], "--source");
            assert!(arguments.contains(&"--codex-home".to_owned()));
            assert!(arguments.contains(&"--user-home".to_owned()));
            let dependency = arguments
                .iter()
                .position(|value| value == "--dependency-user-home");
            assert_eq!(
                dependency.is_some(),
                name == "code-tools",
                "unexpected dependency-home handling for {name}: {arguments:?}"
            );
        }
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
