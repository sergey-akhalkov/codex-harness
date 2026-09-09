//! Prepare an owned outcome comparison arm, then verify actual native discovery.
#[path = "outcome_arm_config.rs"]
mod config;

use crate::{
    outcome_discovery as discovery,
    outcome_run::{bounded_read, isolated, repository, write_new},
};
use harness_core::{
    config_create::ConfigCreation, config_file::ConfigSnapshot, registration::Registration,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Arm {
    Baseline,
    Candidate,
}
impl Arm {
    fn enabled(self) -> bool {
        matches!(self, Self::Candidate)
    }
    fn name(self) -> &'static str {
        if self.enabled() {
            "candidate"
        } else {
            "baseline"
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    case_root: PathBuf,
    codex_home: PathBuf,
    user_home: PathBuf,
    dependency_user_home: PathBuf,
    source_root: PathBuf,
    upstream: PathBuf,
    arm: Arm,
    #[serde(default = "timeout")]
    timeout: u64,
}
fn timeout() -> u64 {
    30
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::other(message)
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "codex-harness outcome-arm --request PATH\nModel-free baseline/candidate skill preparation in an owned temporary installation. JSON and recovery evidence are private."
        );
        return Ok(0);
    }
    if args.len() != 2 || args[0] != "--request" {
        return Err(invalid("expected --request PATH"));
    }
    let bytes = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)
        .map_err(|_| invalid("unreadable arm request"))?;
    let request: Request =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid arm request"))?;
    let private = tempfile::Builder::new()
        .prefix("codex-outcome-arm-")
        .tempdir()?;
    let root = private.path().canonicalize()?;
    if root.starts_with(repository()) {
        return Err(invalid("evidence must be outside source"));
    }
    let _ = private.keep();
    let mut report = json!({"status":"incomplete","arm":request.arm.name(),"evidence_root":root,"model_calls":0,"discovery_verified":false});
    write_new(&root.join("started.json"), &report)?;
    match configure(&request, &root, &mut report) {
        Ok(()) => report["status"] = json!("passed"),
        Err(error) => {
            let phase = report
                .as_object_mut()
                .expect("owned report")
                .remove("phase")
                .unwrap_or(json!("validation"));
            write_new(
                &root.join("failure.json"),
                &json!({"phase":phase,"kind":format!("{:?}",error.kind()),"raw_os_error":error.raw_os_error(),"message":error.to_string()}),
            )?;
            report["status"] = json!("failed");
            report["error"] = json!("Arm preparation was not verified; inspect private evidence.");
        }
    }
    write_new(&root.join("arm.json"), &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(if report["status"] == "passed" { 0 } else { 1 })
}

fn skills(report: &Value) -> io::Result<&Vec<Value>> {
    if report["status"] != "passed" {
        return Err(invalid("native arm discovery failed"));
    }
    report["skills"]
        .as_array()
        .ok_or_else(|| invalid("native discovery lacks skills"))
}

fn configure(request: &Request, root: &Path, report: &mut Value) -> io::Result<()> {
    report["phase"] = json!("validation");
    let case = isolated(&request.case_root)?;
    let home = isolated(&request.codex_home)?;
    let user = isolated(&request.user_home)?;
    if [&home, &user]
        .iter()
        .any(|p| case.starts_with(p) || p.starts_with(&case) || root.starts_with(p))
    {
        return Err(invalid(
            "case and evidence must be outside installation homes",
        ));
    }
    let path = home.join("config.toml");
    let original = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
        Ok(_) => Some(ConfigSnapshot::read(&path)?),
    };
    let original_bytes = original.as_ref().map_or(&[][..], ConfigSnapshot::contents);
    let base_guard = discovery::Snapshot::read(path.clone())?;
    let profile_guard = discovery::Snapshot::read(home.join("harness.config.toml"))?;
    // The read-only guard owns the installer's existing mutex and checks both
    // metadata and actual source links. It does not mutate installation state.
    let ownership = harness_core::installation_links::OwnedSkillLinks::read(
        &request.source_root,
        &request.codex_home,
        &request.user_home,
        &request.dependency_user_home,
        &config::CANDIDATES,
        harness_core::installation_lock::InstallationLock::acquire(&request.user_home)?,
    )?;
    report["ownership"] = serde_json::to_value(ownership.receipt())?;
    let discovery_request = discovery::Request {
        case_root: case,
        codex_home: home,
        upstream: request.upstream.clone(),
        timeout: request.timeout,
        output_limit: 16 * 1024 * 1024,
        extra_config: BTreeMap::new(),
    };
    report["phase"] = json!("before-discovery");
    let before = discovery::discover(&discovery_request)?;
    report["before_evidence"] = before["evidence_root"].clone();
    write_new(&root.join("before.json"), &before)?;
    let before_skills = skills(&before)?;
    let candidates = config::candidate_skills(before_skills)?;
    let mut skill_documents = Vec::new();
    for skill in &candidates {
        let path = Path::new(
            skill["path"]
                .as_str()
                .ok_or_else(|| invalid("invalid skill path"))?,
        )
        .canonicalize()?;
        skill_documents.push(ConfigSnapshot::read(&path)?);
    }
    for link in ownership.receipt().skills.iter() {
        let expected = link.source.join("SKILL.md").canonicalize()?;
        if !candidates.iter().any(|skill| {
            skill["name"] == link.name
                && skill["path"]
                    .as_str()
                    .is_some_and(|p| Path::new(p).canonicalize().is_ok_and(|p| p == expected))
        }) {
            return Err(invalid(
                "native discovery lacks the recorded installer source skill",
            ));
        }
    }
    let bytes = config::prepare(original_bytes, before_skills, request.arm.enabled())?;
    base_guard.verify()?;
    profile_guard.verify()?;
    ownership.verify_unchanged()?;
    let registration = Registration::open(&root.join("publication"))?;
    report["publication_state"] = json!(registration.state());
    let mut attempted = false;
    let result = (|| {
        report["phase"] = json!("publication");
        if bytes != original_bytes || original.is_none() {
            attempted = true;
            match &original {
                Some(original) => {
                    registration.apply_with_files(&[], &[original.plan_replace(&bytes)?], &[])?;
                }
                None => {
                    registration.apply_with_files(
                        &[],
                        &[],
                        &[ConfigCreation::new(&path, &bytes)?],
                    )?;
                }
            }
        }
        let published = ConfigSnapshot::read(&path)?;
        if published.contents() != bytes {
            return Err(invalid("published arm bytes differ"));
        }
        report["phase"] = json!("after-discovery");
        let after = discovery::discover(&discovery_request)?;
        report["after_evidence"] = after["evidence_root"].clone();
        write_new(&root.join("after.json"), &after)?;
        config::verify_after(before_skills, skills(&after)?, request.arm.enabled())?;
        config::verify_configuration(&before["config"]["config"], &after["config"]["config"])?;
        if before["upstream_sha256"] != after["upstream_sha256"] {
            return Err(invalid("upstream changed between arm observations"));
        }
        published.verify_unchanged()?;
        profile_guard.verify()?;
        ownership.verify_unchanged()?;
        for document in &skill_documents {
            document.verify_unchanged()?;
        }
        report["skills"] = after["skills"].clone();
        report["config"] = after["config"].clone();
        report["native"] = after["native"].clone();
        report["scope"] = after["scope"].clone();
        report["configuration_changed"] = json!(attempted);
        report["discovery_verified"] = json!(true);
        Ok(())
    })();
    if result.is_err() && attempted {
        match registration.disconnect() {
            Ok(undo) => {
                report["rollback"] =
                    json!({"status":"restored","restored":undo.restored,"removed":undo.removed})
            }
            Err(error) => {
                write_new(
                    &root.join("rollback-failure.json"),
                    &json!({"kind":format!("{:?}",error.kind()),"raw_os_error":error.raw_os_error(),"message":error.to_string()}),
                )?;
                report["rollback"] = json!({"status":"conflict","error":"Recovery remains in private publication state; foreign data was preserved."});
            }
        }
    }
    result?;
    report
        .as_object_mut()
        .expect("owned report")
        .remove("phase");
    Ok(())
}
