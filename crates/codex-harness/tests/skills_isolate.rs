//! Model-free isolation through the installed CLI entry point.
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Fixture {
    marker: PathBuf,
    control: PathBuf,
    source: PathBuf,
    case: PathBuf,
    library: PathBuf,
    request: PathBuf,
    _root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("skills-isolate-")
            .tempdir()
            .unwrap();
        let source = root.path().join("source");
        let case = root.path().join("case");
        let control = root.path().join("control");
        let library = root.path().join("library");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&case).unwrap();
        fs::create_dir(&control).unwrap();
        fs::create_dir(&library).unwrap();
        let marker = source.join("SKILL.md");
        fs::write(&marker, "live source must not change\n").unwrap();
        fs::write(
            library.join("SKILL.md"),
            "---\nname: project-verification\ndescription: Isolation CLI fixture.\n---\nBody\n",
        )
        .unwrap();
        fs::write(control.join("oracle.json"), "{\"pass\":true}").unwrap();
        fs::write(control.join("baseline.json"), "{\"arm\":\"L\"}").unwrap();
        let request = root.path().join("request.json");
        fs::write(
            &request,
            serde_json::to_vec(&json!({
                "source_root": source,
                "case_root": case,
                "control_root": control,
                "library_root": library,
                "session_marker": marker
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            marker,
            control,
            source,
            case,
            library,
            request,
            _root: root,
        }
    }
}

fn run(request: &Path) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "isolate", "--request"])
        .arg(request)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    let code = output.status.code().unwrap_or(2);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (code, value)
}

#[test]
fn isolate_entry_point_denies_control_writes_without_a_model_call() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.marker).unwrap();
    let (code, value) = run(&fixture.request);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["status"], "passed");
    assert_eq!(value["model_calls"], 0);
    assert_eq!(value["isolation"]["control_files_write"], "denied");
    assert_eq!(value["isolation"]["isolation_verified"], true);
    assert_eq!(
        value["isolation"]["library"]["name"],
        "project-verification"
    );
    assert_eq!(fs::read(&fixture.marker).unwrap(), before);
    assert!(fixture.source.join("SKILL.md").exists());
    assert!(fixture.case.exists());
    assert!(fixture.library.join("SKILL.md").exists());
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(fixture.control.join("oracle.json"))
            .is_err()
    );
}

#[test]
fn isolate_does_not_run_without_a_request() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "isolate"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
