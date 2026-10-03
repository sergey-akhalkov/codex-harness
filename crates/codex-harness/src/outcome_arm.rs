//! Prepare an owned outcome comparison arm, then verify actual native discovery.
#[path = "outcome_arm_config.rs"]
mod config;

use crate::{
    outcome_discovery as discovery,
    outcome_prepare::contract_digest,
    outcome_run::{bounded_read, isolated, repository, write_new},
};
use harness_core::{
    build_identity::{hash_file, ordinary},
    config_create::ConfigCreation,
    config_file::ConfigSnapshot,
    registration::Registration,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Component, Path, PathBuf},
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
    /// Expected path-independent digest of the frozen task contract, as
    /// reported by `outcome-prepare`.
    case_contract: String,
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

const CASE_RECEIPT_LIMIT: u64 = 4 * 1024 * 1024;

fn digest_text(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn relative_input(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\0')
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

struct CaseContract {
    contract: String,
    case_id: String,
    source_state: String,
    immutable: BTreeMap<String, String>,
}

/// Read the prepared case's own receipt and recompute the frozen contract from
/// it. The digest binds the case kind, the declared source state, the exact
/// prompt and every immutable input hash, so an altered receipt cannot keep
/// the expected identity. A missing, incomplete or changed contract refuses
/// the arm instead of measuring an unknown workload input.
fn read_case_contract(case: &Path, expected: &str) -> io::Result<CaseContract> {
    let bytes = bounded_read(&case.join("preparation.json"), CASE_RECEIPT_LIMIT)
        .map_err(|_| invalid("the measured case has no readable preparation receipt"))?;
    let receipt: Value = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("the measured case receipt is not valid JSON"))?;
    if receipt["status"] != "passed" {
        return Err(invalid("the measured case preparation did not pass"));
    }
    let setup = receipt
        .get("setup")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("the measured case receipt has no frozen contract"))?;
    let field = |name: &str| {
        setup
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| invalid("the measured case receipt is incomplete"))
    };
    let case_id = field("case_id")?;
    let source_state = field("source_state")?;
    let prompt = field("prompt")?;
    let recorded = field("contract")?;
    let immutable = setup
        .get("immutable")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("the measured case receipt has no immutable inputs"))?;
    let mut frozen = BTreeMap::new();
    for (name, hash) in immutable {
        if !relative_input(name) {
            return Err(invalid(
                "the measured case receipt holds an invalid input name",
            ));
        }
        let hash = hash
            .as_str()
            .filter(|hash| digest_text(hash))
            .ok_or_else(|| invalid("the measured case receipt holds an invalid input hash"))?;
        frozen.insert(name.clone(), hash.to_owned());
    }
    if frozen.is_empty() {
        return Err(invalid(
            "the measured case receipt declares no immutable input",
        ));
    }
    let contract = contract_digest(&case_id, &prompt, &frozen);
    if !digest_text(&recorded) || recorded != contract || contract != expected {
        return Err(invalid(
            "the measured case does not carry the frozen task contract",
        ));
    }
    Ok(CaseContract {
        contract,
        case_id,
        source_state,
        immutable: frozen,
    })
}

/// Re-hash every declared immutable input. A source change made by another
/// arm's executor, a helper or this arm's own discovery changes one of them
/// and refuses the arm instead of silently measuring an altered contract.
fn verify_case_inputs(case: &Path, contract: &CaseContract) -> io::Result<()> {
    for (name, declared) in &contract.immutable {
        let path = case.join(name);
        ordinary(&path).map_err(|_| invalid("a frozen case input is missing or linked"))?;
        if hash_file(&path).map_err(|_| invalid("a frozen case input is unreadable"))? != *declared
        {
            return Err(invalid(
                "the measured case's frozen input changed since preparation",
            ));
        }
    }
    Ok(())
}

/// The measured case may not expose another checkout's Git history. A linked
/// worktree pointer, a shared common Git directory, an alternate object store
/// or a configured remote keep sibling solutions reachable from the case, and
/// a case nested inside an enclosing checkout resolves that checkout's
/// references from its working directory. An independent repository or no
/// repository at all is the verified boundary; anything else refuses before
/// the arm consumes the input.
fn verify_git_boundary(case: &Path) -> io::Result<&'static str> {
    let git = case.join(".git");
    match fs::symlink_metadata(&git) {
        Ok(meta) if meta.file_type().is_symlink() || meta.is_file() => {
            Err(invalid("the measured case links an external Git directory"))
        }
        Ok(_) => {
            ordinary(&git)
                .map_err(|_| invalid("the measured case links an external Git directory"))?;
            if git.join("commondir").exists() {
                return Err(invalid("the measured case shares a common Git directory"));
            }
            if git.join("objects/info/alternates").exists() {
                return Err(invalid(
                    "the measured case shares an alternate Git object store",
                ));
            }
            let text = bounded_read(&git.join("config"), 1024 * 1024).unwrap_or_default();
            if String::from_utf8_lossy(&text)
                .lines()
                .any(|line| line.trim_start().starts_with("[remote"))
            {
                return Err(invalid("the measured case holds a configured Git remote"));
            }
            Ok("independent")
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let temp = std::env::temp_dir().canonicalize()?;
            let mut current = case.parent();
            while let Some(dir) = current {
                if fs::symlink_metadata(dir.join(".git")).is_ok() {
                    return Err(invalid(
                        "the measured case lies inside an enclosing Git checkout",
                    ));
                }
                if dir == temp {
                    break;
                }
                current = dir.parent();
            }
            Ok("absent")
        }
        Err(error) => Err(error),
    }
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "codex-harness outcome-arm --request PATH\nModel-free baseline/candidate preparation of an owned temporary installation over one frozen outcome case. JSON and recovery evidence are private."
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
    if !digest_text(&request.case_contract) {
        return Err(invalid("invalid frozen case contract digest"));
    }
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
    if home.join("AGENTS.override.md").exists() {
        return Err(invalid(
            "the arm home carries an AGENTS override; a fresh executor home must not replace the arm's instructions",
        ));
    }
    report["phase"] = json!("case");
    let boundary = verify_git_boundary(&case)?;
    let contract = read_case_contract(&case, &request.case_contract)?;
    verify_case_inputs(&case, &contract)?;
    report["case"] = json!({"root":case,"contract":contract.contract,
        "case_id":contract.case_id,"source_state":contract.source_state,
        "immutable_files":contract.immutable.len(),"git_boundary":boundary,
        "verified_before":true});
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
        case_root: case.clone(),
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
        report["phase"] = json!("case-after");
        let after_contract = read_case_contract(&case, &request.case_contract)?;
        verify_case_inputs(&case, &after_contract)?;
        report["case"]["verified_after"] = json!(true);
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
