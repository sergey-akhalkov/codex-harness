//! Explicit native core component entry point. Other components retain their
//! existing lifecycle until their native implementations pass acceptance.
use harness_core::core_install::{self, Request};
use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf, time::Duration};

fn options(
    args: &[OsString],
    allowed: &[&str],
    allow_preview: bool,
) -> io::Result<(BTreeMap<OsString, OsString>, bool)> {
    let mut values = BTreeMap::new();
    let mut core = false;
    let mut preview = false;
    let mut iter = args.iter();
    while let Some(key) = iter.next() {
        match key.to_str() {
            Some("--core-only") if !core => core = true,
            Some("--preview") if allow_preview && !preview => preview = true,
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
    if !core {
        return Err(io::Error::other(
            "native lifecycle currently requires explicit --core-only; other components have not completed native acceptance",
        ));
    }
    Ok((values, preview))
}

fn required(values: &BTreeMap<OsString, OsString>, name: &str) -> io::Result<PathBuf> {
    values
        .get(&OsString::from(name))
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other(format!("{name} is required")))
}

pub fn recover(args: &[OsString]) -> io::Result<i32> {
    let (values, preview) = options(
        args,
        &["--codex-home", "--user-home", "--dependency-user-home"],
        true,
    )?;
    let user = required(&values, "--user-home")?;
    let dependency = values
        .get(&OsString::from("--dependency-user-home"))
        .map(PathBuf::from)
        .unwrap_or_else(|| user.clone());
    let codex = required(&values, "--codex-home")?;
    if preview {
        let report = core_install::preview_recovery(&codex, &user, &dependency)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        let report = core_install::recover(&codex, &user, &dependency)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    Ok(0)
}

pub fn check(args: &[OsString]) -> io::Result<i32> {
    let (values, _) = options(
        args,
        &[
            "--codex-home",
            "--user-home",
            "--dependency-user-home",
            "--timeout-seconds",
        ],
        false,
    )?;
    let user = required(&values, "--user-home")?;
    let dependency = values
        .get(&OsString::from("--dependency-user-home"))
        .map(PathBuf::from)
        .unwrap_or_else(|| user.clone());
    let timeout = values
        .get(&OsString::from("--timeout-seconds"))
        .map(|value| {
            value
                .to_str()
                .and_then(|text| text.parse::<u64>().ok())
                .ok_or_else(|| io::Error::other("invalid native check timeout"))
        })
        .transpose()?
        .unwrap_or(45);
    let report = harness_core::core_check::check(
        &required(&values, "--codex-home")?,
        &user,
        &dependency,
        Duration::from_secs(timeout),
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

pub fn disconnect(args: &[OsString]) -> io::Result<i32> {
    let (values, preview) = options(
        args,
        &["--codex-home", "--user-home", "--dependency-user-home"],
        true,
    )?;
    let user = required(&values, "--user-home")?;
    let dependency = values
        .get(&OsString::from("--dependency-user-home"))
        .map(PathBuf::from)
        .unwrap_or_else(|| user.clone());
    let report = harness_core::core_disconnect::disconnect(
        &required(&values, "--codex-home")?,
        &user,
        &dependency,
        preview,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    let (values, preview) = options(
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
    let user_home = required(&values, "--user-home")?;
    let timeout = values
        .get(&OsString::from("--timeout-seconds"))
        .map(|v| {
            v.to_str()
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| io::Error::other("invalid native installation timeout"))
        })
        .transpose()?
        .unwrap_or(45);
    let request = Request {
        source: required(&values, "--source")?,
        build: required(&values, "--build")?,
        codex_home: required(&values, "--codex-home")?,
        dependency_user_home: values
            .get(&OsString::from("--dependency-user-home"))
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home.clone()),
        user_home,
        upstream: values.get(&OsString::from("--upstream")).map(PathBuf::from),
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
    };
    let report = core_install::connect(&request, preview)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}
