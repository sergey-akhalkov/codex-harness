//! Native source diagnostics. Public JSON contains only selected preferences and
//! source observations; private native responses live in an owned temporary root.
use crate::{native_read_rpc as rpc, outcome_discovery::Snapshot, source_diagnostics_view};
use harness_core::{build_identity, native_upstream, source_observation};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) struct Request {
    pub source: Option<PathBuf>,
    pub codex_home: PathBuf,
    pub user_home: PathBuf,
    pub dependency_user_home: PathBuf,
    pub project: PathBuf,
    pub upstream: Option<PathBuf>,
    pub profile: String,
    pub timeout: Duration,
}

fn invalid() -> io::Error {
    io::Error::other("native source observation is unavailable or incompatible")
}

fn local(path: &Path) -> io::Result<PathBuf> {
    let text = path.to_str().ok_or_else(invalid)?;
    normal(Path::new(text.strip_prefix("\\\\?\\").unwrap_or(text)))
}

fn normal(path: &Path) -> io::Result<PathBuf> {
    use std::path::{Component, Prefix};
    if !path.is_absolute()
        || path.to_str().is_none_or(|text| text.contains('\0'))
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
    {
        return Err(invalid());
    }
    std::path::absolute(path)
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness diagnose [--source CHECKOUT] [--codex-home DIRECTORY] [--user-home DIRECTORY] [--dependency-user-home DIRECTORY] [--project DIRECTORY] [--upstream EXECUTABLE_OR_PACKAGE] [--profile NAME] [--timeout-seconds SECONDS]\nRead-only native source observations, JSON status healthy|attention|incomplete. No model or project commands."
        );
        return Ok(0);
    }
    let allowed = [
        "--source",
        "--codex-home",
        "--user-home",
        "--dependency-user-home",
        "--project",
        "--upstream",
        "--profile",
        "--timeout-seconds",
    ];
    let mut values = BTreeMap::new();
    let mut iter = args.iter();
    while let Some(name) = iter.next() {
        if !name.to_str().is_some_and(|s| allowed.contains(&s)) {
            return Err(io::Error::other("unknown source diagnostics option"));
        }
        let value = iter
            .next()
            .ok_or_else(|| io::Error::other("missing source diagnostics option value"))?;
        if values.insert(name.clone(), value.clone()).is_some() {
            return Err(io::Error::other("duplicate source diagnostics option"));
        }
    }
    let path = |key: &str| values.get(&OsString::from(key)).map(PathBuf::from);
    let user_home = path("--user-home")
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
        .ok_or_else(invalid)?;
    let codex_home = path("--codex-home")
        .or_else(|| env::var_os("CODEX_HOME").map(PathBuf::from))
        .unwrap_or_else(|| user_home.join(".codex"));
    let profile = values
        .get(&OsString::from("--profile"))
        .map(|s| s.to_str().ok_or_else(invalid))
        .transpose()?
        .unwrap_or("harness")
        .to_owned();
    let timeout = values
        .get(&OsString::from("--timeout-seconds"))
        .map(|s| {
            s.to_str()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(invalid)
        })
        .transpose()?
        .unwrap_or(30);
    let mut request = Request {
        source: path("--source"),
        codex_home,
        dependency_user_home: path("--dependency-user-home").unwrap_or_else(|| user_home.clone()),
        user_home,
        project: path("--project").unwrap_or(env::current_dir()?),
        upstream: path("--upstream"),
        profile,
        timeout: Duration::from_secs(timeout),
    };
    for path in [
        &mut request.codex_home,
        &mut request.user_home,
        &mut request.dependency_user_home,
        &mut request.project,
    ] {
        *path = std::path::absolute(&*path)?;
    }
    for path in [&mut request.source, &mut request.upstream]
        .into_iter()
        .flatten()
    {
        *path = std::path::absolute(&*path)?;
    }
    println!("{}", serde_json::to_string_pretty(&diagnose(&request)?)?);
    Ok(0)
}

pub(crate) fn diagnose(request: &Request) -> io::Result<Value> {
    let started = Instant::now();
    for path in [
        &request.codex_home,
        &request.user_home,
        &request.dependency_user_home,
        &request.project,
    ] {
        normal(path)?;
    }
    for path in [&request.source, &request.upstream].into_iter().flatten() {
        normal(path)?;
    }
    if request.profile.is_empty()
        || request.profile.len() > 128
        || !request
            .profile
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || request.timeout.is_zero()
        || request.timeout > Duration::from_secs(60)
    {
        return Err(invalid());
    }
    let mut report = json!({"schemaVersion":1,"model_calls":0,"status":"incomplete","project":request.project,
        "codexHome":request.codex_home,"sourceRoot":request.source,"profile":request.profile,
        "observation":"native-base-and-profile-layers","native":{"status":"unavailable"},
        "settings":[],"layers":[],"skills":[],"links":[],"findings":[],
        "freshness":{"existingSessions":"unknown","existingMcpLspServers":"unknown",
            "action":"Restart the affected consumer to load source changes; use ordinary Check for protocol health."}});
    let mut findings = Vec::new();
    let mut upstream = request.upstream.clone();
    match source_observation::read(&request.codex_home, &request.user_home, &request.dependency_user_home, request.source.as_deref()) {
        Ok(observation) => {
            report["sourceRoot"] = json!(observation.source);
            report["links"] = json!(observation.links);
            findings.extend(observation.findings);
            upstream = upstream.or(observation.upstream);
        }
        Err(_) => findings.push(json!({"code":"installation-unavailable","source":request.codex_home.join("harness/installation.json"),
            "action":"Check the owner homes and any active installation operation; no repair was attempted."})),
    }
    let mut process_report = json!({});
    let observed = upstream
        .as_deref()
        .ok_or_else(invalid)
        .and_then(|entry| observe(request, entry, &mut process_report));
    match observed {
        Ok((native, mut view)) => {
            report["native"] = native;
            for key in ["settings", "layers", "skills"] {
                report[key] = view[key].take();
            }
            findings.extend(
                view["findings"]
                    .as_array()
                    .ok_or_else(invalid)?
                    .iter()
                    .cloned(),
            );
        }
        Err(_) => {
            let timed_out = process_report["process"]["outcome"]["reason"] == "Timeout"
                || process_report["diagnostic_timeout"] == true;
            findings.push(json!({"code":if timed_out {"native-timeout"} else {"native-unavailable-or-incompatible"},
                "source":upstream,"action":"Check the installed CLI, selected profile, project path and TOML locally; native error text is withheld."}));
        }
    }
    let incomplete = report["native"]["status"] != "observed"
        || findings.iter().any(|f| {
            matches!(
                f["code"].as_str(),
                Some(
                    "installation-unavailable"
                        | "inventory-unavailable"
                        | "skill-load-error"
                        | "profile-context-unresolved"
                        | "runtime-requirements-present"
                )
            )
        });
    report["status"] = json!(if incomplete {
        "incomplete"
    } else if findings.is_empty() {
        "healthy"
    } else {
        "attention"
    });
    report["findings"] = json!(findings);
    report["elapsedMilliseconds"] = json!(started.elapsed().as_millis());
    Ok(report)
}

fn observe(
    request: &Request,
    entry: &Path,
    process_report: &mut Value,
) -> io::Result<(Value, Value)> {
    let excluded = [
        env::current_exe()?,
        request.codex_home.join("harness/bin/codex.exe"),
        request.codex_home.join("harness/bin/codex.ps1"),
    ];
    let upstream = native_upstream::resolve(
        Some(entry),
        None,
        &excluded,
        &native_upstream::ManagerHints::default(),
    )?;
    let home = request.codex_home.canonicalize()?;
    let project = normal(&request.project)?;
    if !project.is_dir() {
        return Err(invalid());
    }
    let profile_path = request
        .codex_home
        .join(format!("{}.config.toml", request.profile));
    let base_snapshot = Snapshot::read(request.codex_home.join("config.toml"))?;
    let profile_snapshot = Snapshot::read(profile_path.clone())?;
    if !profile_path.is_file() {
        return Err(invalid());
    }
    let temporary = tempfile::Builder::new()
        .prefix("harness-native-source-read-")
        .tempdir()?;
    let root = temporary.path().canonicalize()?;
    let base_evidence = root.join("base");
    let profile_evidence = root.join("profile");
    let profile_home = root.join("profile-home");
    for path in [&base_evidence, &profile_evidence, &profile_home] {
        fs::create_dir(path)?;
    }
    let profile_link = profile_home.join("config.toml");
    std::os::windows::fs::symlink_file(&profile_path, &profile_link)?;
    let deadline = Instant::now()
        .checked_add(request.timeout)
        .ok_or_else(invalid)?;
    let observation = (|| {
        let mut read = |case: &Path, home: &Path, evidence: &Path, protocol| {
            let remaining = match deadline.checked_duration_since(Instant::now()) {
                Some(remaining) if !remaining.is_zero() => remaining,
                _ => {
                    process_report["diagnostic_timeout"] = json!(true);
                    return Err(invalid());
                }
            };
            rpc::exchange(
                rpc::Request {
                    upstream: &upstream.executable,
                    case,
                    working_directory: &root,
                    home,
                    extra: &[],
                    root: evidence,
                    timeout: remaining,
                    output_limit: 16 * 1024 * 1024,
                    protocol,
                },
                process_report,
            )
        };
        let base = read(&project, &home, &base_evidence, rpc::Protocol::Sources)?;
        let version = regex::Regex::new(r"/([0-9]+\.[0-9]+\.[0-9]+)").map_err(|_| invalid())?;
        if version
            .captures(base["native"]["userAgent"].as_str().ok_or_else(invalid)?)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            != Some("0.153.4")
        {
            return Err(invalid());
        }
        let profile_home = profile_home.canonicalize()?;
        let profile = read(
            &profile_home,
            &profile_home,
            &profile_evidence,
            rpc::Protocol::Profile,
        )?;
        let layers = profile["config"]["layers"].as_array().ok_or_else(invalid)?;
        let users: Vec<_> = layers
            .iter()
            .filter(|l| l["name"]["type"] == "user")
            .collect();
        if users.len() != 1 {
            return Err(invalid());
        }
        let view = source_diagnostics_view::summarize(
            &request.project,
            &request.profile,
            &profile_path,
            &base["config"],
            &users[0]["config"],
            &base["requirements"],
            &base["listed"],
        )?;
        base_snapshot.verify()?;
        profile_snapshot.verify()?;
        if build_identity::hash_file(&upstream.executable)? != upstream.sha256 {
            return Err(invalid());
        }
        Ok((
            json!({"status":"observed","executable":local(&upstream.executable)?,
            "protocol":"config/read + configRequirements/read + skills/list","version":"0.153.4",
            "profileSelection":"reconstructed; app-server does not accept file profiles","skillsScope":"native base consumer"}),
            view,
        ))
    })();
    if observation.is_err()
        && Instant::now() >= deadline
        && process_report["process"]["outcome"]["reason"] != "MemoryLimit"
        && process_report["process"]["output_limit_reached"] != true
    {
        process_report["diagnostic_timeout"] = json!(true);
    }
    // Each RPC returned only after the owned job was stopped. Unlink the source
    // before deleting the diagnostic-owned caches and private raw observations.
    fs::remove_file(profile_link)?;
    temporary.close()?;
    observation
}
