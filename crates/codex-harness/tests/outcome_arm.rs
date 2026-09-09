//! Model-free actual CLI treatment and rollback, entirely on owned temp data.
#![cfg(windows)]
use serde_json::{Value, json};
use std::{
    fs,
    os::windows::fs::{symlink_dir, symlink_file},
    path::{Path, PathBuf},
    process::Command,
};

const CANDIDATES: [&str; 2] = ["project-verification", "reproduce-regression"];
const BASE: &str = "# owned original\r\nmodel = 'gpt-6-astra'\r\ncheck_for_update_on_startup = false\r\n[features]\r\nhooks = false\r\nmulti_agent = false\r\n";
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
        let case = root.path().join("case");
        let request = root.path().join("request.json");
        let state = home.join("harness/installation.json");
        for dir in [
            source.join("global/agents"),
            user.join(".agents/skills"),
            home.join("skills"),
            home.join("harness"),
            case.clone(),
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
            &json!({"case_root":case,"codex_home":home,"user_home":user,"dependency_user_home":user,"source_root":source,"upstream":env!("CARGO_BIN_EXE_harness-launch-fixture"),"arm":arm,"timeout":5}),
        );
        Self {
            root,
            source,
            home,
            user,
            case,
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
