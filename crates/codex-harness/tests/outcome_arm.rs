//! Model-free actual CLI treatment and rollback, entirely on owned temp data.
#![cfg(windows)]
use harness_core::build_identity::hash_file;
use serde_json::{Value, json};
use std::{
    fs,
    os::windows::fs::{symlink_dir, symlink_file},
    path::{Path, PathBuf},
    process::Command,
};

const CANDIDATES: [&str; 2] = ["project-verification", "reproduce-regression"];
const BASE: &str = "# owned original\r\nmodel = 'gpt-6-astra'\r\ncheck_for_update_on_startup = false\r\n[features]\r\nhooks = false\r\nmulti_agent = false\r\n";

/// One fresh controlled case through the real preparation entry point. Its
/// path-independent contract digest is what the arm must bind.
fn prepare_case(host: &Path) -> (PathBuf, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-prepare", "--case", "entrypoint"])
        .current_dir(host)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "passed");
    (
        PathBuf::from(report["case_root"].as_str().unwrap()),
        report["setup"]["contract"].as_str().unwrap().to_owned(),
    )
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    home: PathBuf,
    user: PathBuf,
    case: PathBuf,
    contract: String,
    state: PathBuf,
    request: PathBuf,
}
impl Fixture {
    fn new(arm: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix("arm проверка-")
            .tempdir()
            .unwrap();
        let source = root.path().join("source");
        let user = root.path().join("user");
        let home = user.join(".codex");
        let (case, contract) = prepare_case(root.path());
        let request = root.path().join("request.json");
        let state = home.join("harness/installation.json");
        for dir in [
            source.join("global/agents"),
            user.join(".agents/skills"),
            home.join("skills"),
            home.join("harness"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        for file in [
            "harness.config.toml",
            "principles-of-work.md",
            "hooks.json",
            "rtk-hooks.json",
        ] {
            fs::write(source.join("global").join(file), "inert owned source data").unwrap();
        }
        write(
            &source.join("global/kit.json"),
            &json!({"schema":1,"profile_name":"harness","profile":"global/harness.config.toml","instructions":"global/principles-of-work.md","skills":".agents/skills","agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/rtk-hooks.json"}),
        );
        let mut links = Vec::new();
        let mut rows = Vec::new();
        for name in CANDIDATES {
            let skill_source = source.join(".agents/skills").join(name);
            fs::create_dir_all(&skill_source).unwrap();
            fs::write(skill_source.join("SKILL.md"), format!("---\nname: {name}\ndescription: owned outcome fixture\n---\n# Inert acceptance skill\n")).unwrap();
            let destination = user.join(".agents/skills").join(name);
            symlink_dir(&skill_source, &destination).unwrap();
            // Explicit CODEX_HOME compatibility location is discoverable even
            // when Windows Known Folders correctly ignore the isolated user.
            let fallback = home.join("skills").join(name);
            symlink_dir(&skill_source, &fallback).unwrap();
            links.push(json!({"kind":"skill","name":name,"source":skill_source,"destination":destination,"owned":true}));
            for path in [destination.join("SKILL.md"), fallback.join("SKILL.md")] {
                rows.push(json!({"name":name,"path":path,"description":"owned outcome fixture","enabled":true,"scope":"user"}));
            }
        }
        rows.push(json!({"name":"personal-owned","path":home.join("personal/SKILL.md"),"description":"preserved","enabled":false,"scope":"user"}));
        write(&case.join("fixture-skills.json"), &json!(rows));
        write(
            &state,
            &json!({"schemaVersion":1,"sourceRoot":source,"codexHome":home,"userHome":user,"dependencyUserHome":user,"codexCommand":env!("CARGO_BIN_EXE_harness-launch-fixture"),"profileName":"harness","pathScope":"Process","pathAdded":false,"versions":{},"links":links}),
        );
        fs::write(home.join("config.toml"), BASE).unwrap();
        write(
            &request,
            &json!({"case_root":case,"case_contract":contract,"codex_home":home,"user_home":user,"dependency_user_home":user,"source_root":source,"upstream":env!("CARGO_BIN_EXE_harness-launch-fixture"),"arm":arm,"timeout":5}),
        );
        Self {
            root,
            source,
            home,
            user,
            case,
            contract,
            state,
            request,
        }
    }
    fn run(&self, mode: &str, expected: i32) -> Value {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .args(["outcome-arm", "--request"])
            .arg(&self.request)
            .current_dir(self.root.path());
        if mode != "native" {
            command
                .env("HARNESS_LAUNCH_FIXTURE_MODE", "discovery")
                .env("HARNESS_DISCOVERY_FIXTURE", mode);
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("private discovery fixture sentinel")
        );
        let row: Value = serde_json::from_slice(&output.stdout).unwrap();
        let evidence = Path::new(row["evidence_root"].as_str().unwrap());
        assert_eq!(read(&evidence.join("arm.json")), row);
        assert_eq!(row["model_calls"], 0);
        assert!(!evidence.starts_with(self.root.path()));
        println!("arm evidence: {}", evidence.display());
        row
    }
}

#[test]
fn actual_cli_prepares_all_registrations_preserves_source_and_repeats_without_rewrite() {
    for arm in ["baseline", "candidate"] {
        let f = Fixture::new(arm);
        let metadata = fs::read(&f.state).unwrap();
        let source = fs::read(
            f.source
                .join(".agents/skills/project-verification/SKILL.md"),
        )
        .unwrap();
        let row = f.run("arm", 0);
        assert_eq!(row["discovery_verified"], true);
        assert_eq!(row["configuration_changed"], true);
        let skills = row["skills"].as_array().unwrap();
        assert_eq!(
            skills
                .iter()
                .filter(|s| CANDIDATES.contains(&s["name"].as_str().unwrap()))
                .count(),
            4
        );
        assert!(
            skills
                .iter()
                .filter(|s| CANDIDATES.contains(&s["name"].as_str().unwrap()))
                .all(|s| s["enabled"] == (arm == "candidate"))
        );
        assert_eq!(skills.last().unwrap()["enabled"], false);
        let changed = fs::read(f.home.join("config.toml")).unwrap();
        assert!(changed.starts_with(BASE.as_bytes()));
        let repeat = f.run("arm", 0);
        assert_eq!(repeat["configuration_changed"], false);
        assert_eq!(fs::read(f.home.join("config.toml")).unwrap(), changed);
        assert_eq!(fs::read(&f.state).unwrap(), metadata);
        assert_eq!(
            fs::read(
                f.source
                    .join(".agents/skills/project-verification/SKILL.md")
            )
            .unwrap(),
            source
        );
    }
}

#[test]
fn absent_configuration_is_created_and_failed_verification_restores_exact_absence_or_bytes() {
    for absent in [false, true] {
        for mode in [
            "arm-ignore",
            "arm-unrelated-change",
            "arm-after-error",
            "arm-config-change",
        ] {
            let f = Fixture::new("baseline");
            let config = f.home.join("config.toml");
            if absent {
                fs::remove_file(&config).unwrap();
            }
            let row = f.run(mode, 1);
            assert_eq!(row["discovery_verified"], false);
            assert_eq!(row["rollback"]["status"], "restored");
            if absent {
                assert!(!config.exists());
            } else {
                assert_eq!(fs::read(&config).unwrap(), BASE.as_bytes());
            }
        }
    }
    let f = Fixture::new("baseline");
    fs::remove_file(f.home.join("config.toml")).unwrap();
    f.run("arm", 0);
    assert!(f.home.join("config.toml").is_file());
}

#[test]
fn foreign_config_edit_blocks_rollback_and_retains_recovery_evidence() {
    let f = Fixture::new("baseline");
    let row = f.run("arm-foreign-edit", 1);
    assert_eq!(row["rollback"]["status"], "conflict");
    assert_eq!(
        fs::read_to_string(f.home.join("config.toml")).unwrap(),
        "# FOREIGN_ARM_WRITER\n"
    );
    let root = Path::new(row["evidence_root"].as_str().unwrap());
    assert!(root.join("rollback-failure.json").is_file());
    assert!(
        Path::new(row["publication_state"].as_str().unwrap())
            .join("journal.json")
            .is_file()
    );
}

#[test]
fn wrong_or_unowned_installation_link_and_linked_config_never_reach_publication() {
    for mutation in [
        "unowned",
        "wrong-source",
        "missing",
        "same-name-foreign-source",
        "linked-config",
    ] {
        let f = Fixture::new("baseline");
        match mutation {
            "unowned" => {
                let mut state = read(&f.state);
                state["links"][0]["owned"] = json!(false);
                write(&f.state, &state);
            }
            "wrong-source" => {
                let mut request = read(&f.request);
                request["source_root"] = json!(f.root.path());
                write(&f.request, &request);
            }
            "missing" => {
                fs::remove_dir(f.user.join(".agents/skills/project-verification")).unwrap()
            }
            "same-name-foreign-source" => {
                let mut rows = read(&f.case.join("fixture-skills.json"));
                for (index, row) in rows.as_array_mut().unwrap().iter_mut().enumerate() {
                    if row["name"] == "project-verification" {
                        let foreign = f.home.join(format!("foreign-{index}/SKILL.md"));
                        fs::create_dir_all(foreign.parent().unwrap()).unwrap();
                        fs::write(
                            &foreign,
                            "---\nname: project-verification\ndescription: another source\n---\n",
                        )
                        .unwrap();
                        row["path"] = json!(foreign);
                    }
                }
                write(&f.case.join("fixture-skills.json"), &rows);
            }
            _ => {
                let config = f.home.join("config.toml");
                fs::remove_file(&config).unwrap();
                let foreign = f.root.path().join("foreign.toml");
                fs::write(&foreign, BASE).unwrap();
                symlink_file(foreign, config).unwrap();
            }
        }
        let row = f.run("arm", 1);
        assert!(row.get("publication_state").is_none());
        if mutation == "same-name-foreign-source" {
            let failure =
                read(&Path::new(row["evidence_root"].as_str().unwrap()).join("failure.json"));
            assert_eq!(
                failure["message"],
                "native discovery lacks the recorded installer source skill"
            );
        }
        assert_eq!(
            fs::read(f.home.join("config.toml")).unwrap(),
            BASE.as_bytes()
        );
    }
}

#[test]
fn opposite_existing_override_is_preserved_and_refused() {
    let f = Fixture::new("baseline");
    f.run("arm", 0);
    let bytes = fs::read(f.home.join("config.toml")).unwrap();
    let mut request = read(&f.request);
    request["arm"] = json!("candidate");
    write(&f.request, &request);
    let row = f.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert_eq!(fs::read(f.home.join("config.toml")).unwrap(), bytes);
}

fn failure_message(row: &Value) -> String {
    read(&Path::new(row["evidence_root"].as_str().unwrap()).join("failure.json"))["message"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn both_arms_bind_the_same_frozen_contract_over_separate_case_copies() {
    let baseline = Fixture::new("baseline");
    let candidate = Fixture::new("candidate");
    assert_ne!(baseline.case, candidate.case);
    assert_eq!(baseline.contract, candidate.contract);
    let left = baseline.run("arm", 0);
    let right = candidate.run("arm", 0);
    assert_eq!(left["case"]["contract"], baseline.contract);
    assert_eq!(right["case"]["contract"], candidate.contract);
    assert_eq!(left["case"]["case_id"], "entrypoint");
    assert_eq!(left["case"]["source_state"], "controlled-v3-rust");
    assert_eq!(left["case"]["git_boundary"], "absent");
    assert_eq!(left["case"]["verified_before"], true);
    assert_eq!(left["case"]["verified_after"], true);
    assert_ne!(left["case"]["root"], right["case"]["root"]);
}

#[test]
fn changed_missing_or_mismatched_frozen_contracts_refuse_before_publication() {
    // An earlier arm's executor changed this copy's frozen input.
    let f = Fixture::new("baseline");
    fs::write(f.case.join("source.json"), "{\"version\":9}\n").unwrap();
    let row = f.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("frozen input changed since preparation"));
    // The contaminated copy is retained, not cleaned or silently re-frozen.
    assert_eq!(
        fs::read_to_string(f.case.join("source.json")).unwrap(),
        "{\"version\":9}\n"
    );
    assert_eq!(
        fs::read(f.home.join("config.toml")).unwrap(),
        BASE.as_bytes()
    );

    // A case whose receipt was removed cannot be measured at all.
    let f = Fixture::new("baseline");
    fs::remove_file(f.case.join("preparation.json")).unwrap();
    let row = f.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("no readable preparation receipt"));

    // An incomplete preparation is not a frozen workload.
    let f = Fixture::new("baseline");
    let receipt = f.case.join("preparation.json");
    let mut value = read(&receipt);
    value["status"] = json!("incomplete");
    write(&receipt, &value);
    let row = f.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("preparation did not pass"));

    // A receipt rewritten to a different contract does not match the
    // expected digest the caller received from the preparation entry point.
    let f = Fixture::new("baseline");
    let mut request = read(&f.request);
    request["case_contract"] = json!("0".repeat(64));
    write(&f.request, &request);
    let row = f.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("does not carry the frozen task contract"));
    assert_eq!(
        fs::read(f.home.join("config.toml")).unwrap(),
        BASE.as_bytes()
    );
}

#[test]
fn cases_that_can_reach_sibling_git_history_are_refused() {
    let linked = Fixture::new("baseline");
    fs::write(
        linked.case.join(".git"),
        format!(
            "gitdir: {}\n",
            linked
                .root
                .path()
                .join("sibling/.git/worktrees/case")
                .display()
        ),
    )
    .unwrap();
    let row = linked.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("links an external Git directory"));

    let shared = Fixture::new("baseline");
    fs::create_dir(shared.case.join(".git")).unwrap();
    fs::write(shared.case.join(".git/commondir"), "../shared\n").unwrap();
    let row = shared.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("shares a common Git directory"));

    let alternate = Fixture::new("baseline");
    fs::create_dir_all(alternate.case.join(".git/objects/info")).unwrap();
    fs::write(
        alternate.case.join(".git/objects/info/alternates"),
        "D:/somewhere/objects\n",
    )
    .unwrap();
    let row = alternate.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("shares an alternate Git object store"));

    let remote = Fixture::new("baseline");
    fs::create_dir(remote.case.join(".git")).unwrap();
    fs::write(
        remote.case.join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = ../sibling\n",
    )
    .unwrap();
    let row = remote.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("holds a configured Git remote"));

    // A case nested inside an enclosing checkout resolves that checkout's
    // references from its working directory and is refused as well.
    let nested = Fixture::new("baseline");
    let enclosing = nested.root.path().join("enclosing");
    fs::create_dir_all(enclosing.join(".git")).unwrap();
    let moved = enclosing.join("case");
    fs::rename(&nested.case, &moved).unwrap();
    let mut request = read(&nested.request);
    request["case_root"] = json!(moved);
    write(&nested.request, &request);
    let row = nested.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("lies inside an enclosing Git checkout"));

    // An independent repository boundary is accepted and recorded.
    let independent = Fixture::new("baseline");
    fs::create_dir(independent.case.join(".git")).unwrap();
    fs::write(
        independent.case.join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n",
    )
    .unwrap();
    let row = independent.run("arm", 0);
    assert_eq!(row["case"]["git_boundary"], "independent");
}

#[test]
fn one_arms_work_cannot_change_the_other_arms_frozen_input() {
    let left = Fixture::new("baseline");
    let right = Fixture::new("candidate");
    // The baseline arm's executor legitimately rewrites its own generated
    // artifact and leaves its solution attempt in its own copy.
    fs::write(left.case.join("built.json"), "{\"version\":2}\n").unwrap();
    fs::write(left.case.join("solution-attempt.txt"), "arm work\n").unwrap();
    let left_row = left.run("arm", 0);
    assert_eq!(left_row["case"]["verified_after"], true);
    // Its own frozen inputs are unchanged by the arm's own preparation.
    let receipt = read(&left.case.join("preparation.json"));
    for (name, hash) in receipt["setup"]["immutable"].as_object().unwrap() {
        assert_eq!(
            hash_file(&left.case.join(name)).unwrap(),
            hash.as_str().unwrap()
        );
    }
    // The other arm still measures the untouched frozen contract, sees no
    // solution artifact and binds the same contract identity.
    let right_row = right.run("arm", 0);
    assert_eq!(right_row["case"]["contract"], left_row["case"]["contract"]);
    assert!(!right.case.join("solution-attempt.txt").exists());
    assert_eq!(
        fs::read_to_string(right.case.join("built.json")).unwrap(),
        "{\"version\":1}\n"
    );
}

#[test]
fn a_home_instruction_override_is_refused_before_publication() {
    let f = Fixture::new("baseline");
    fs::write(f.home.join("AGENTS.override.md"), "# sibling context\n").unwrap();
    let row = f.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("AGENTS override"));
    assert_eq!(
        fs::read(f.home.join("config.toml")).unwrap(),
        BASE.as_bytes()
    );
}

#[test]
fn the_measured_case_must_stay_separate_from_the_installation_homes() {
    let f = Fixture::new("baseline");
    let nested = f.user.join("measured-case");
    fs::rename(&f.case, &nested).unwrap();
    let mut request = read(&f.request);
    request["case_root"] = json!(nested);
    write(&f.request, &request);
    let row = f.run("arm", 1);
    assert!(row.get("publication_state").is_none());
    assert!(failure_message(&row).contains("outside installation homes"));
    assert_eq!(
        fs::read(f.home.join("config.toml")).unwrap(),
        BASE.as_bytes()
    );
}

#[test]
#[ignore = "explicit installed native CLI, no models or authentication copy"]
fn installed_native_confirms_baseline_and_candidate_in_owned_homes() {
    let native =
        std::env::var_os("HARNESS_NATIVE_CODEX").expect("explicit original native executable");
    for arm in ["baseline", "candidate"] {
        let f = Fixture::new(arm);
        let mut request = read(&f.request);
        request["upstream"] = json!(PathBuf::from(&native));
        request["timeout"] = json!(90);
        write(&f.request, &request);
        let before = fs::read(&f.state).unwrap();
        let row = f.run("native", 0);
        assert_eq!(row["discovery_verified"], true);
        assert_eq!(fs::read(&f.state).unwrap(), before);
    }
}
