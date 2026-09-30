//! The deterministic CI workflow is an enforced contract, not documentation:
//! this test parses the checked-in workflow and fails when a required check
//! disappears, a floating action revision replaces an immutable one, or a
//! credential or model-backed route appears in the default CI signal.
use std::{fs, path::Path};
use yaml_rust2::{Yaml, YamlLoader};

const WORKFLOW: &str = ".github/workflows/windows-native-checks.yml";
const INSTALLED_WORKFLOW: &str = ".github/workflows/windows-installed-integration.yml";

fn workflow() -> Yaml {
    tracked_workflow(WORKFLOW)
}

fn tracked_workflow(relative: &str) -> Yaml {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = fs::read_to_string(root.join(relative)).expect("workflow is tracked");
    let documents = YamlLoader::load_from_str(&text).expect("workflow is valid YAML");
    assert_eq!(documents.len(), 1, "one workflow document");
    documents.into_iter().next().unwrap()
}

fn string(value: &Yaml) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn steps(job: &Yaml) -> Vec<&Yaml> {
    job["steps"].as_vec().into_iter().flatten().collect()
}

fn all_run_scripts(document: &Yaml) -> Vec<String> {
    let mut scripts = Vec::new();
    for job in document["jobs"].as_hash().into_iter().flatten() {
        for step in steps(job.1) {
            if let Some(script) = step["run"].as_str() {
                scripts.push(script.to_owned());
            }
        }
    }
    scripts
}

#[test]
fn installed_integration_is_a_separate_explicitly_selected_route() {
    let document = tracked_workflow(INSTALLED_WORKFLOW);
    // Only manual dispatch: a green default CI run never stands in for the
    // installed acceptance route.
    assert!(
        !document["on"]["workflow_dispatch"].is_badvalue(),
        "manual dispatch trigger"
    );
    assert!(document["on"]["push"].is_badvalue(), "no push trigger");
    assert!(
        document["on"]["pull_request"].is_badvalue(),
        "no pull-request trigger"
    );
    let permissions = document["permissions"].as_hash().expect("permissions");
    assert_eq!(
        string(
            permissions
                .get(&Yaml::String("contents".into()))
                .unwrap_or(&Yaml::Null)
        ),
        "read"
    );
    let scripts = all_run_scripts(&document).join("\n---\n");
    for required in [
        "cargo build --workspace --locked --jobs 1",
        "npm install @openai/codex@0.157.1",
        "HARNESS_CONTROL_CODEX_EXE",
        "--test native_launcher",
        "--test manager_delivery",
        "--test orchestration_isolated_install -- --test-threads=1 --ignored",
        "--test xai_transport",
        "--test mcp_stdio",
        "--test tui",
    ] {
        assert!(
            scripts.contains(required),
            "installed route step missing: {required}"
        );
    }
    assert!(
        !scripts.contains("--run-model-probes"),
        "model-backed evaluation stays outside both workflows"
    );
    // The deterministic signal must not claim this scope either.
    let deterministic_scripts = all_run_scripts(&workflow()).join("\n---\n");
    for installed_only in [
        "cargo build --workspace --locked --jobs 1",
        "--test orchestration_isolated_install",
        "--test manager_delivery",
    ] {
        assert!(
            !deterministic_scripts.contains(installed_only),
            "installed-only route leaked into default CI: {installed_only}"
        );
    }
}

#[test]
fn permissions_are_read_only_and_triggers_are_public() {
    let document = workflow();
    let permissions = document["permissions"]
        .as_hash()
        .expect("permissions block");
    assert_eq!(permissions.len(), 1, "one permission: {permissions:?}");
    assert_eq!(
        string(
            permissions
                .get(&Yaml::String("contents".into()))
                .unwrap_or(&Yaml::Null)
        ),
        "read",
        "CI reads the repository and nothing else"
    );
    let push = document["on"]["push"]["branches"]
        .as_vec()
        .expect("push trigger on main");
    assert_eq!(string(&push[0]), "main");
    assert!(document["on"]["pull_request"].is_badvalue() == false);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw = fs::read_to_string(root.join(WORKFLOW)).unwrap();
    assert!(
        !raw.contains("secrets."),
        "the default CI signal uses no credentials"
    );
}

#[test]
fn actions_are_pinned_to_immutable_revisions() {
    let document = workflow();
    let mut used = Vec::new();
    for job in document["jobs"].as_hash().into_iter().flatten() {
        for step in steps(job.1) {
            let reference = string(&step["uses"]);
            if reference.is_empty() {
                continue;
            }
            let (action, revision) = reference.split_once('@').expect("action@revision");
            assert_eq!(action, "actions/checkout", "only the official checkout");
            assert!(
                revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()),
                "immutable full commit revision required: {reference}"
            );
            used.push(reference);
        }
    }
    assert_eq!(used.len(), 1, "exactly one pinned action: {used:?}");
}

#[test]
fn every_required_deterministic_check_is_present() {
    let document = workflow();
    let jobs = document["jobs"].as_hash().expect("one job");
    assert_eq!(jobs.len(), 1, "the deterministic signal is one job");
    let job = jobs.values().next().unwrap();
    assert!(
        string(&job["runs-on"]).contains("windows"),
        "Windows/MSVC acceptance"
    );
    let timeout = job["timeout-minutes"].as_i64().expect("bounded job");
    assert!((30..=300).contains(&timeout), "bounded timeout: {timeout}");
    let scripts = all_run_scripts(&document);
    let joined = scripts.join("\n---\n");
    for required in [
        "cargo fmt --all -- --check",
        "cargo clippy --workspace --all-targets --locked --jobs 1 -- -D warnings",
        "cargo test --workspace --locked --jobs 1 -- --test-threads=1",
        "cargo run -p codex-harness --locked --bin codex-harness -- ownership-check --source .",
        "cargo run -p codex-harness --locked --bin harness-source-check -- --root .",
        "rustup toolchain install 1.98.1",
    ] {
        assert!(
            joined.contains(required),
            "required check missing from the workflow: {required}"
        );
    }
    assert!(
        !joined.contains("--run-model-probes"),
        "model-backed evaluation is explicitly selected outside default CI"
    );
    assert!(
        !joined.contains("subscription-login"),
        "no credential or subscription route in default CI"
    );
}
