//! Fixed, model-free native app-server observation for outcome comparisons.
#[cfg(windows)]
#[path = "outcome_discovery_rpc.rs"]
mod rpc;

use crate::outcome_run::{bounded_read, config_arguments, isolated, repository, write_new};
use harness_core::{
    build_identity::{hash_file, ordinary},
    config_file::ConfigSnapshot,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

const INPUT_LIMIT: u64 = 4 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub(crate) case_root: PathBuf,
    pub(crate) codex_home: PathBuf,
    pub(crate) upstream: PathBuf,
    #[serde(default = "timeout")]
    pub(crate) timeout: u64,
    #[serde(default = "output_limit")]
    pub(crate) output_limit: u64,
    #[serde(default)]
    pub(crate) extra_config: BTreeMap<String, Value>,
}
fn timeout() -> u64 {
    30
}
fn output_limit() -> u64 {
    16 * 1024 * 1024
}

/// Follows a declared data link for reading, while remembering its target and
/// that target's ordinary-file identity. No path or file is changed here.
pub(crate) struct Snapshot {
    path: PathBuf,
    target: Option<PathBuf>,
    file: Option<ConfigSnapshot>,
}
impl Snapshot {
    fn receipt(&self) -> Value {
        json!({"path":self.path,"target":self.target,"present":self.file.is_some(),
            "sha256":self.file.as_ref().map(|file|harness_core::build_identity::hash_bytes(file.contents()))})
    }
    pub(crate) fn read(path: PathBuf) -> io::Result<Self> {
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                path,
                target: None,
                file: None,
            }),
            Err(error) => Err(error),
            Ok(_) => {
                let target = path.canonicalize()?;
                let file = ConfigSnapshot::read(&target)?;
                if file.contents().len() as u64 > INPUT_LIMIT {
                    return Err(invalid("configuration size limit"));
                }
                Ok(Self {
                    path,
                    target: Some(target),
                    file: Some(file),
                })
            }
        }
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        if let Some(target) = &self.target {
            if self.path.canonicalize()? != *target {
                return Err(invalid("configuration target changed"));
            }
            self.file
                .as_ref()
                .expect("present snapshot")
                .verify_unchanged()
        } else if fs::symlink_metadata(&self.path)
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        {
            Ok(())
        } else {
            Err(invalid("configuration appeared during discovery"))
        }
    }
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "codex-harness outcome-discover --request PATH\nModel-free native skills/list and config/read in an isolated temporary case/home. Detailed JSON is private evidence."
        );
        return Ok(0);
    }
    if args.len() != 2 || args[0] != "--request" {
        return Err(invalid("expected --request PATH"));
    }
    let bytes = bounded_read(Path::new(&args[1]), INPUT_LIMIT)
        .map_err(|_| invalid("unreadable request"))?;
    let request: Request =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid request JSON"))?;
    let report = discover(&request)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(if report["status"] == "passed" { 0 } else { 1 })
}

pub(crate) fn discover(request: &Request) -> io::Result<Value> {
    let private = tempfile::Builder::new()
        .prefix("codex-outcome-discovery-")
        .tempdir()?;
    let root = private.path().canonicalize()?;
    if root.starts_with(repository()) {
        return Err(invalid("evidence must be outside source"));
    }
    let _ = private.keep();
    let mut report = json!({"status":"incomplete","evidence_root":root,"model_calls":0,
        "scope":"native-base; profile has no discovery overrides"});
    write_new(&root.join("started.json"), &report)?;
    match observe(request, &root, &mut report) {
        Ok(()) => report["status"] = json!("passed"),
        Err(error) => {
            let phase = report
                .as_object_mut()
                .expect("owned report")
                .remove("phase")
                .unwrap_or(json!("validation"));
            write_new(
                &root.join("failure.json"),
                &json!({"phase":phase,"kind":format!("{:?}",error.kind()),
                "raw_os_error":error.raw_os_error(),"message":error.to_string()}),
            )?;
            report["status"] = json!("failed");
            report["error"] = json!("Native discovery was not verified; inspect private evidence.");
        }
    }
    write_new(&root.join("discovery.json"), &report)?;
    Ok(report)
}

fn validate_profile(snapshot: &Snapshot, case: &Path) -> io::Result<()> {
    let Some(file) = &snapshot.file else {
        return Ok(());
    };
    let text = std::str::from_utf8(file.contents())
        .map_err(|_| invalid("profile is not UTF-8"))?
        .trim_start_matches('\u{feff}');
    let profile: toml::Table =
        toml::from_str(text).map_err(|_| invalid("profile is not valid TOML"))?;
    if ["skills", "project_root_markers", "credential_broker"]
        .iter()
        .any(|key| profile.contains_key(*key))
    {
        return Err(invalid(
            "file profile changes discovery; native base listing is insufficient",
        ));
    }
    if let Some(projects) = profile.get("projects") {
        let projects = projects
            .as_table()
            .ok_or_else(|| invalid("invalid profile projects"))?;
        for key in projects.keys() {
            let path = Path::new(key);
            if !path.is_absolute() {
                return Err(invalid(
                    "relative profile project cannot establish discovery scope",
                ));
            }
            match path.canonicalize() {
                Ok(path) if case.starts_with(&path) => {
                    return Err(invalid("file profile affects the selected project"));
                }
                Ok(_) => (),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn observe(request: &Request, root: &Path, report: &mut Value) -> io::Result<()> {
    report["phase"] = json!("validation");
    if !(1..=300).contains(&request.timeout)
        || !(1024..=64 * 1024 * 1024).contains(&request.output_limit)
        || !request.upstream.is_absolute()
        || !request
            .upstream
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
    {
        return Err(invalid("invalid native discovery limits or executable"));
    }
    let case = isolated(&request.case_root)?;
    let home = isolated(&request.codex_home)?;
    if case.starts_with(&home)
        || home.starts_with(&case)
        || root.starts_with(&case)
        || root.starts_with(&home)
    {
        return Err(invalid(
            "separate case, home and evidence directories required",
        ));
    }
    let args = config_arguments(&request.extra_config)?;
    ordinary(&request.upstream)?;
    let upstream_hash = hash_file(&request.upstream)?;
    let config = Snapshot::read(home.join("config.toml"))?;
    let profile = Snapshot::read(home.join("harness.config.toml"))?;
    validate_profile(&profile, &case)?;
    write_new(
        &root.join("inputs.json"),
        &json!({"upstream":request.upstream,
        "upstream_sha256":upstream_hash,"base_config":config.receipt(),"profile":profile.receipt()}),
    )?;
    report["upstream_sha256"] = json!(upstream_hash);
    report["phase"] = json!("protocol");
    let response = rpc::exchange(request, &case, &home, &args, root, report)?;
    report["phase"] = json!("response");
    let rows = response["listed"]["data"]
        .as_array()
        .ok_or_else(|| invalid("skills response has no data array"))?;
    if rows.len() != 1 {
        return Err(invalid(
            "skills response does not identify one requested project",
        ));
    }
    let row = &rows[0];
    let observed_cwd = row["cwd"]
        .as_str()
        .map(Path::new)
        .ok_or_else(|| invalid("skills response lacks project identity"))?;
    if !observed_cwd.is_absolute() || observed_cwd.canonicalize()? != case {
        return Err(invalid("skills response project mismatch"));
    }
    if !row["errors"].as_array().is_some_and(Vec::is_empty) {
        return Err(invalid("skills discovery is incomplete"));
    }
    let skills = row["skills"]
        .as_array()
        .ok_or_else(|| invalid("skills response lacks skills array"))?;
    for skill in skills {
        if !skill["name"].as_str().is_some_and(|s| !s.is_empty())
            || !skill["path"]
                .as_str()
                .is_some_and(|s| Path::new(s).is_absolute())
            || !skill["enabled"].is_boolean()
            || !skill["description"].is_string()
            || !skill["scope"]
                .as_str()
                .is_some_and(|s| ["user", "repo", "system", "admin"].contains(&s))
        {
            return Err(invalid("invalid native skill identity"));
        }
    }
    if !response["config"]["config"].is_object() || !response["config"]["origins"].is_object() {
        return Err(invalid("config/read has no configuration object"));
    }
    report["phase"] = json!("input-verification");
    config.verify()?;
    profile.verify()?;
    if hash_file(&request.upstream)? != upstream_hash {
        return Err(invalid("native executable changed during discovery"));
    }
    report["skills"] = json!(skills);
    report["native"] = response["native"].clone();
    report["config"] = response["config"].clone();
    report["base_configuration_unchanged"] = json!(true);
    report
        .as_object_mut()
        .expect("owned report")
        .remove("phase");
    Ok(())
}
#[cfg(not(windows))]
fn observe(_: &Request, _: &Path, _: &mut Value) -> io::Result<()> {
    Err(invalid("Windows native discovery required"))
}
fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
