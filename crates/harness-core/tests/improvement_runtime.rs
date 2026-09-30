//! Native owned temporary arm installation cases.
//!
//! These run the real installation owner (`core_install::connect`, preview and
//! publication) and the real disconnection owner inside owned temporary
//! homes, with two compiled fixture stand-ins for the upstream client and the
//! installed launcher (the established `rustc` fixture pattern). The fixtures
//! emulate `--version`/`--help`, `features list|enable|disable` and the
//! model-free `debug prompt-input` contract, so the checks prove the
//! installation, isolation and consumption wiring - not real Codex CLI
//! behavior. A real qualified local-model acceptance remains lead-owned; the
//! fixture home directories and the ambient account are never touched.
#![cfg(windows)]

use harness_core::{
    build_identity,
    improvement_experiment::{Arm, prepare_home, prepare_variant, select_variant},
    improvement_runtime::{
        ArmRequest, ClientInputs, PrivateInput, discard_arm, install_arm, retire_arm,
        verify_consumption,
    },
    outcome_qualification::{LocalRunner, MaterialIdentity},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::Duration,
};

/// Serializes the cases: the installation owner publishes process-local PATH
/// entries and holds shared installation locks.
static INSTALL: Mutex<()> = Mutex::new(());

struct Fixtures {
    _root: tempfile::TempDir,
    upstream: PathBuf,
    launcher: PathBuf,
}

static FIXTURES: OnceLock<Fixtures> = OnceLock::new();

fn fixtures() -> &'static Fixtures {
    FIXTURES.get_or_init(|| {
        let root = tempfile::Builder::new()
            .prefix("improvement-runtime-fixtures-")
            .tempdir()
            .unwrap();
        let upstream = compile_fixture(root.path(), "upstream", UPSTREAM_SOURCE);
        let launcher = compile_fixture(root.path(), "launcher", LAUNCHER_SOURCE);
        Fixtures {
            _root: root,
            upstream,
            launcher,
        }
    })
}

fn compile_fixture(root: &Path, name: &str, source: &str) -> PathBuf {
    let source_path = root.join(format!("{name}.rs"));
    fs::write(&source_path, source).unwrap();
    let located = Command::new("where.exe").arg("rustc.exe").output().unwrap();
    assert!(located.status.success(), "rustc is required for fixtures");
    let located = String::from_utf8(located.stdout).unwrap();
    let rustc = located.lines().next().unwrap();
    let executable = root.join(format!("{name}.exe"));
    let output = Command::new(rustc)
        .arg(&source_path)
        .arg("--edition=2024")
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture compile failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

/// Model-free stand-in for the original Codex CLI: version/help contract,
/// feature disable/list contract over `$CODEX_HOME/config.toml`.
const UPSTREAM_SOURCE: &str = r##"
use std::{env, fs, path::PathBuf, process::exit};

fn home() -> PathBuf {
    PathBuf::from(env::var_os("CODEX_HOME").expect("CODEX_HOME"))
}

fn set_hooks(text: &mut String, enabled: bool) {
    let mut replaced = String::new();
    let mut found = false;
    for line in text.lines() {
        if line.trim_start().starts_with("hooks") && line.contains('=') {
            replaced.push_str(&format!("hooks = {enabled}\n"));
            found = true;
        } else {
            replaced.push_str(line);
            replaced.push('\n');
        }
    }
    if found {
        *text = replaced;
    } else {
        text.push_str(&format!("\n[features]\nhooks = {enabled}\n"));
    }
}

fn hooks_state(text: &str) -> bool {
    text.lines()
        .rev()
        .find(|line| line.trim_start().starts_with("hooks") && line.contains('='))
        .and_then(|line| line.split('=').nth(1))
        .map(|value| value.trim() == "true")
        .unwrap_or(false)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("codex-cli 0.160.0"),
        Some("--help") => println!(
            "Codex CLI\n\nUsage: codex [OPTIONS]\n      --profile <PROFILE>  Configuration profile from config.toml\n      Select <name>.config.toml with --profile.\n"
        ),
        Some("features") => {
            let config = home().join("config.toml");
            let mut text = fs::read_to_string(&config).unwrap_or_default();
            match (args.get(1).map(String::as_str), args.get(2).map(String::as_str)) {
                (Some("disable"), Some("hooks")) => {
                    set_hooks(&mut text, false);
                    fs::write(&config, text).unwrap();
                }
                (Some("enable"), Some("hooks")) => {
                    set_hooks(&mut text, true);
                    fs::write(&config, text).unwrap();
                }
                (Some("list"), _) => {
                    println!("hooks stable {}", hooks_state(&text));
                    println!("code_mode experimental false");
                }
                _ => exit(2),
            }
        }
        _ => exit(2),
    }
}
"##;

/// Model-free stand-in for the installed launcher: answers the owner's
/// `debug prompt-input` probe from the arm home's own instruction link and the
/// shared permission contract.
const LAUNCHER_SOURCE: &str = r##"
use std::{env, fs, path::PathBuf, process::exit};

fn escape(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("debug")
        || args.get(1).map(String::as_str) != Some("prompt-input")
    {
        exit(2);
    }
    let home = PathBuf::from(env::var_os("CODEX_HOME").expect("CODEX_HOME"));
    let instructions = fs::read_to_string(home.join("AGENTS.md")).expect("installed instructions");
    let permissions = "<permissions instructions>\nFilesystem sandboxing defines which files can be read or written. `sandbox_mode` is `danger-full-access`: No filesystem sandboxing - all commands are permitted.\nApproval policy is currently never.\n</permissions instructions>";
    println!(
        "[{{\"role\":\"developer\",\"content\":[{{\"type\":\"input_text\",\"text\":{}}},{{\"type\":\"input_text\",\"text\":{}}}]}}]",
        escape(&instructions),
        escape(permissions)
    );
}
"##;

struct EnvironmentGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvironmentGuard {
    fn capture(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        Self { name, previous }
    }

    fn set(&self, value: &Path) {
        unsafe { std::env::set_var(self.name, value) };
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

/// Synthetic kit source: the same layout the installation owner reads
/// (`global/kit.json`, profile, instructions, hooks, agents, skills) plus the
/// minimal compiled-input tree the build identity records.
fn kit_source(root: &Path, name: &str, marker: &str) -> PathBuf {
    let source = root.join(name);
    for directory in [
        "crates/one/src",
        "global/agents",
        ".agents/skills/arm-skill",
    ] {
        fs::create_dir_all(source.join(directory)).unwrap();
    }
    fs::write(source.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    fs::write(source.join("Cargo.lock"), "").unwrap();
    fs::write(source.join("crates/one/src/lib.rs"), "pub fn one() {}\n").unwrap();
    fs::write(
        source.join("global/kit.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "profile_name": "harness",
            "profile": "global/harness.config.toml",
            "instructions": "global/principles-of-work.md",
            "skills": ".agents/skills",
            "agents": "global/agents",
            "hooks": "global/hooks.json",
            "token_hooks": "global/rtk-hooks.json",
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        source.join("global/harness.config.toml"),
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\n",
    )
    .unwrap();
    fs::write(
        source.join("global/principles-of-work.md"),
        format!("# Principles\n{marker} arm instructions\n"),
    )
    .unwrap();
    fs::write(source.join("global/hooks.json"), "{}\n").unwrap();
    fs::write(source.join("global/rtk-hooks.json"), "{}\n").unwrap();
    fs::write(
        source.join(".agents/skills/arm-skill/SKILL.md"),
        format!("---\nname: arm-skill\ndescription: Fixture skill.\n---\n\n{marker} skill body\n"),
    )
    .unwrap();
    source
}

fn owned_state(root: &Path) -> PathBuf {
    let state = root.join("state");
    fs::create_dir_all(state.join("builds")).unwrap();
    fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
    state
}

/// One published immutable build inside the owned state: the launcher fixture
/// plus arm-specific recorded artifacts.
fn fixture_build(
    state: &Path,
    name: &str,
    source: &Path,
    launcher: &Path,
    marker: &str,
) -> PathBuf {
    let build = state.join("builds").join(name);
    fs::create_dir_all(&build).unwrap();
    let mut binaries = BTreeMap::new();
    for binary in build_identity::BINARIES {
        let bytes = if *binary == "codex.exe" {
            fs::read(launcher).unwrap()
        } else {
            format!("{binary} fixture {marker}\n").into_bytes()
        };
        fs::write(build.join(binary), &bytes).unwrap();
        binaries.insert((*binary).to_string(), build_identity::hash_bytes(&bytes));
    }
    let record = build_identity::BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.to_path_buf(),
        source: build_identity::source_identity(source).unwrap(),
        rustc: "fixture".into(),
        cargo: "fixture".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries,
    };
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    build
}

struct ArmFixture {
    source: PathBuf,
    build: PathBuf,
    request: ArmRequest,
}

#[allow(clippy::too_many_arguments)]
fn arm_fixture(
    root: &Path,
    state: &Path,
    name: &str,
    arm: Arm,
    label: &str,
    marker: &str,
    launcher: &Path,
    upstream: &Path,
) -> ArmFixture {
    let source = kit_source(root, &format!("kit-{name}"), marker);
    let build = fixture_build(state, name, &source, launcher, marker);
    let variant = prepare_variant(state, arm, label, &build).unwrap();
    let home = prepare_home(&root.join(format!("homes/{name}"))).unwrap();
    let user_home = prepare_home(&root.join(format!("homes/{name}-user"))).unwrap();
    let dependency_user_home = prepare_home(&root.join(format!("homes/{name}-dep"))).unwrap();
    ArmFixture {
        source,
        build,
        request: ArmRequest {
            variant,
            home,
            user_home,
            dependency_user_home,
            upstream: upstream.to_path_buf(),
            timeout: Duration::from_secs(60),
            client: None,
            private_inputs: Vec::new(),
            protected: Vec::new(),
        },
    }
}

fn local_client(catalogue: &Path) -> ClientInputs {
    ClientInputs {
        runner: LocalRunner {
            endpoint: "http://127.0.0.1:45999/v1".into(),
            model: "fixture-glyph-1".into(),
            identity: MaterialIdentity {
                weights: Some("sha256:fixture-weights".into()),
                quantization: Some("fixture-q4".into()),
                ..MaterialIdentity::default()
            },
        },
        reasoning_effort: Some("low".into()),
        catalogue: Some(catalogue.to_path_buf()),
    }
}

/// End-to-end preparation: both arms install through the real owner, consume
/// their own runtime/instructions/skills/configuration, refuse drift, and are
/// restored without touching unrelated or ambient state.
#[test]
fn arms_install_consume_their_own_runtime_and_retire_restores() {
    let _serial = INSTALL.lock().unwrap();
    let temp = tempfile::Builder::new()
        .prefix("improvement-runtime-arms-")
        .tempdir()
        .unwrap();
    let root = temp.path();
    let fixtures = fixtures();
    let _cpu = EnvironmentGuard::capture("CODEX_HARNESS_CPU_ACCOUNT");
    fs::create_dir_all(root.join("cpu-account")).unwrap();
    _cpu.set(&root.join("cpu-account"));
    let _path = EnvironmentGuard::capture("PATH");
    let original_path = std::env::var_os("PATH");
    let state = owned_state(root);

    let mut baseline = arm_fixture(
        root,
        &state,
        "baseline",
        Arm::Baseline,
        "H",
        "baseline",
        &fixtures.launcher,
        &fixtures.upstream,
    );
    let mut candidate = arm_fixture(
        root,
        &state,
        "candidate",
        Arm::Candidate,
        "H+A",
        "candidate",
        &fixtures.launcher,
        &fixtures.upstream,
    );

    // The baseline arm deliberately shares one user/dependency home (the real
    // installation shape); the candidate arm keeps them separate.
    baseline.request.dependency_user_home = baseline.request.user_home.clone();

    // Explicit shared client inputs and one explicit private runner file; the
    // ambient account is never read for these.
    let catalogue = root.join("model-catalogue.json");
    fs::write(&catalogue, br#"{"models":[{"name":"fixture-glyph-1"}]}"#).unwrap();
    let secret = "private-runner-input-sentinel";
    let private_source = root.join("runner.private");
    fs::write(&private_source, secret).unwrap();
    for arm in [&mut baseline, &mut candidate] {
        arm.request.client = Some(local_client(&catalogue));
        arm.request.private_inputs = vec![PrivateInput {
            source: private_source.clone(),
            destination: "runner.private".into(),
        }];
    }
    baseline.request.protected = vec![
        candidate.build.clone(),
        candidate.request.home.clone(),
        candidate.request.user_home.clone(),
        candidate.request.dependency_user_home.clone(),
        candidate.source.clone(),
    ];
    candidate.request.protected = vec![
        baseline.build.clone(),
        baseline.request.home.clone(),
        baseline.request.user_home.clone(),
        baseline.source.clone(),
    ];

    // Unrelated state a fresh installation must preserve.
    let unrelated = baseline.request.home.join("auth.json");
    fs::write(&unrelated, b"unrelated-credentials-stand-in").unwrap();
    let foreign = baseline
        .request
        .user_home
        .join(".agents/skills/foreign-fixture/SKILL.md");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::write(
        &foreign,
        "---\nname: foreign-fixture\ndescription: Preserved.\n---\n",
    )
    .unwrap();

    let baseline_runtime = install_arm(&baseline.request).unwrap();
    let candidate_runtime = install_arm(&candidate.request).unwrap();
    assert_eq!(baseline_runtime.installation.status, "connected");
    assert!(baseline_runtime.installation.links > 0);
    assert_eq!(baseline_runtime.model_calls, 0);
    assert_eq!(
        baseline_runtime.installation.runtime_executable_sha256,
        build_identity::hash_file(&fixtures.upstream).unwrap()
    );
    assert_eq!(
        baseline_runtime.upstream_sha256,
        build_identity::hash_file(&fixtures.upstream).unwrap()
    );

    // Separately verified consumption: each launcher resolves to its own
    // frozen arm build, and no cross-arm aliasing exists.
    let baseline_consumed = verify_consumption(&baseline_runtime).unwrap();
    let candidate_consumed = verify_consumption(&candidate_runtime).unwrap();
    assert_eq!(baseline_consumed.arm, Arm::Baseline);
    assert_eq!(candidate_consumed.arm, Arm::Candidate);
    assert!(baseline_consumed.model_ready);
    assert_eq!(baseline_consumed.model_calls, 0);
    assert_eq!(
        fs::canonicalize(&baseline_runtime.launcher.source).unwrap(),
        fs::canonicalize(baseline.build.join("codex.exe")).unwrap()
    );
    assert_eq!(
        fs::read_link(baseline_runtime.home.join("harness/bin/codex.exe"))
            .unwrap()
            .canonicalize()
            .unwrap(),
        fs::canonicalize(baseline.build.join("codex.exe")).unwrap()
    );
    assert_eq!(
        fs::read_link(candidate_runtime.home.join("harness/bin/codex.exe"))
            .unwrap()
            .canonicalize()
            .unwrap(),
        fs::canonicalize(candidate.build.join("codex.exe")).unwrap()
    );
    assert_ne!(
        baseline_runtime.launcher.sha256,
        String::new(),
        "the launcher identity is retained"
    );

    // Arm-specific instruction and skill consumption.
    assert_eq!(
        fs::canonicalize(&baseline_runtime.instructions.source).unwrap(),
        fs::canonicalize(baseline.source.join("global/principles-of-work.md")).unwrap()
    );
    assert_eq!(
        fs::canonicalize(&candidate_runtime.instructions.source).unwrap(),
        fs::canonicalize(candidate.source.join("global/principles-of-work.md")).unwrap()
    );
    let baseline_text = fs::read_to_string(baseline_runtime.home.join("AGENTS.md")).unwrap();
    let candidate_text = fs::read_to_string(candidate_runtime.home.join("AGENTS.md")).unwrap();
    assert!(baseline_text.contains("baseline arm instructions"));
    assert!(candidate_text.contains("candidate arm instructions"));
    assert_eq!(baseline_runtime.skills.len(), 1);
    assert_eq!(candidate_runtime.skills.len(), 1);
    assert_eq!(baseline_runtime.skills[0].name, "arm-skill");
    assert_ne!(
        fs::canonicalize(&baseline_runtime.skills[0].source).unwrap(),
        fs::canonicalize(&candidate_runtime.skills[0].source).unwrap()
    );
    assert!(
        baseline_runtime.skills[0]
            .source
            .canonicalize()
            .unwrap()
            .starts_with(fs::canonicalize(&baseline.source).unwrap())
    );

    // Tool links and the effective configuration; the shared client
    // configuration is byte-identical across arms (non-treatment setting).
    let baseline_config = baseline_runtime.configuration.as_ref().unwrap();
    let candidate_config = candidate_runtime.configuration.as_ref().unwrap();
    assert_eq!(
        fs::read(&baseline_config.path).unwrap(),
        fs::read(&candidate_config.path).unwrap()
    );
    let config_text = fs::read_to_string(&baseline_config.path).unwrap();
    for expected in [
        "model = \"fixture-glyph-1\"",
        "model_provider = \"local\"",
        "model_reasoning_effort = \"low\"",
        "base_url = \"http://127.0.0.1:45999/v1\"",
        "hooks = false",
    ] {
        assert!(config_text.contains(expected), "{config_text}");
    }
    assert!(config_text.contains("model_catalog_json"));
    assert!(
        baseline_runtime
            .commands
            .iter()
            .any(|command| command.name == "codex-harness.exe")
    );

    // Unrelated state survived the installation.
    assert_eq!(
        fs::read(&unrelated).unwrap(),
        b"unrelated-credentials-stand-in"
    );
    assert_eq!(
        fs::read_to_string(&foreign).unwrap(),
        "---\nname: foreign-fixture\ndescription: Preserved.\n---\n"
    );
    // The private input was copied with its digest; the value itself never
    // appears in the retained public identity.
    assert_eq!(
        fs::read_to_string(baseline_runtime.home.join("runner.private")).unwrap(),
        secret
    );
    let serialized = serde_json::to_string(&baseline_runtime).unwrap();
    assert!(!serialized.contains(secret));
    assert!(serialized.contains("runner.private"));

    // Selection reuses the unchanged prepared variants and refuses an active
    // attempt without editing source or build artifacts.
    let first = select_variant(&state, &baseline.request.variant, false).unwrap();
    assert!(first.changed);
    let repeated = select_variant(&state, &baseline.request.variant, false).unwrap();
    assert!(!repeated.changed);
    assert!(
        select_variant(&state, &candidate.request.variant, true)
            .unwrap_err()
            .to_string()
            .contains("active")
    );
    verify_consumption(&baseline_runtime).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    // Drift refuses an apparently ready arm, and the other arm stays consumed.
    let instructions = candidate.source.join("global/principles-of-work.md");
    let original_instructions = fs::read(&instructions).unwrap();
    fs::write(
        &instructions,
        [original_instructions.as_slice(), b"\ndrift"].concat(),
    )
    .unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(error.contains("AGENTS"), "{error}");
    verify_consumption(&baseline_runtime).unwrap();
    fs::write(&instructions, &original_instructions).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    let skill = candidate.source.join(".agents/skills/arm-skill/SKILL.md");
    let original_skill = fs::read(&skill).unwrap();
    fs::write(&skill, [original_skill.as_slice(), b"\ndrift"].concat()).unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(error.contains("arm-skill"), "{error}");
    fs::write(&skill, &original_skill).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    let command = candidate.build.join("token-audit.exe");
    let original_command = fs::read(&command).unwrap();
    fs::write(&command, b"altered").unwrap();
    assert!(verify_consumption(&candidate_runtime).is_err());
    fs::write(&command, &original_command).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    let module = candidate.source.join("crates/one/src/lib.rs");
    let original_module = fs::read(&module).unwrap();
    fs::write(&module, "pub fn one() { /* drift */ }\n").unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unchanged build"), "{error}");
    fs::write(&module, &original_module).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    let catalogue_bytes = fs::read(&catalogue).unwrap();
    fs::write(&catalogue, b"{\"models\":[]}").unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(error.contains("catalogue"), "{error}");
    fs::write(&catalogue, &catalogue_bytes).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    fs::write(candidate_runtime.home.join("runner.private"), "drift").unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(error.contains("runner.private"), "{error}");
    fs::write(candidate_runtime.home.join("runner.private"), secret).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    let candidate_config_bytes = fs::read(&candidate_config.path).unwrap();
    fs::write(
        &candidate_config.path,
        [candidate_config_bytes.as_slice(), b"\n# drift\n"].concat(),
    )
    .unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(error.contains("configuration"), "{error}");
    fs::write(&candidate_config.path, &candidate_config_bytes).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    // Retirement restores the prepared homes: owned links and the
    // process-local PATH entry are removed; unrelated files stay untouched.
    let retirement = retire_arm(&baseline_runtime).unwrap();
    assert_eq!(retirement.status, "disconnected");
    assert!(retirement.removed_links > 0);
    assert!(retirement.model_calls == 0);
    assert!(!baseline_runtime.home.join("AGENTS.md").exists());
    assert!(
        !baseline_runtime
            .home
            .join("harness/installation.json")
            .exists()
    );
    assert!(
        !baseline_runtime
            .user_home
            .join(".agents/skills/arm-skill")
            .exists()
    );
    assert_eq!(
        fs::read(&unrelated).unwrap(),
        b"unrelated-credentials-stand-in"
    );
    assert!(foreign.is_file());
    assert!(
        fs::read_to_string(&baseline_config.path)
            .unwrap()
            .contains("model = \"fixture-glyph-1\"")
    );
    verify_consumption(&candidate_runtime).unwrap();

    // The restored homes can carry a fresh preparation again with the same
    // identity: reversible cleanup, never a merge of prior state.
    let reinstalled = install_arm(&baseline.request).unwrap();
    assert_eq!(
        reinstalled.launcher.sha256,
        baseline_runtime.launcher.sha256
    );
    verify_consumption(&reinstalled).unwrap();
    retire_arm(&reinstalled).unwrap();

    // A missing launcher link refuses the arm and still allows the lifecycle
    // owner to restore the home (missing owned links are not foreign data).
    fs::remove_file(candidate_runtime.home.join("harness/bin/codex.exe")).unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(error.contains("codex.exe"), "{error}");

    let candidate_retirement = retire_arm(&candidate_runtime).unwrap();
    assert_eq!(candidate_retirement.status, "disconnected");
    assert!(candidate_retirement.already_missing >= 1);
    assert!(!candidate_runtime.home.join("AGENTS.md").exists());

    // Process-local PATH publication was fully withdrawn.
    assert_eq!(std::env::var_os("PATH"), original_path);
}

/// Refusals: overlap, missing or mismatched inputs, ambient homes and partial
/// installation state never produce an apparently ready arm.
#[test]
fn refusals_guard_overlap_missing_inputs_and_partial_state() {
    let _serial = INSTALL.lock().unwrap();
    let temp = tempfile::Builder::new()
        .prefix("improvement-runtime-refusals-")
        .tempdir()
        .unwrap();
    let root = temp.path();
    let fixtures = fixtures();
    let _cpu = EnvironmentGuard::capture("CODEX_HARNESS_CPU_ACCOUNT");
    fs::create_dir_all(root.join("cpu-account")).unwrap();
    _cpu.set(&root.join("cpu-account"));
    let state = owned_state(root);
    let fixture = arm_fixture(
        root,
        &state,
        "refusal",
        Arm::Baseline,
        "H",
        "baseline",
        &fixtures.launcher,
        &fixtures.upstream,
    );
    let refused =
        |request: &ArmRequest| -> String { install_arm(request).unwrap_err().to_string() };
    let not_installed = |home: &Path| !home.join("harness/installation.json").exists();

    // Existing or missing homes.
    let mut request = fixture.request.clone();
    request.home = root.join("missing-home");
    assert!(refused(&request).contains("fresh owned directory"));

    let occupied = prepare_home(&root.join("occupied")).unwrap();
    fs::create_dir_all(occupied.join("harness")).unwrap();
    fs::write(occupied.join("harness/installation.json"), "{}").unwrap();
    let mut request = fixture.request.clone();
    request.home = occupied.clone();
    assert!(refused(&request).contains("already carries an installation"));
    assert!(occupied.join("harness/installation.json").is_file());

    let shadowed = prepare_home(&root.join("shadowed")).unwrap();
    fs::write(shadowed.join("AGENTS.override.md"), "shadow").unwrap();
    let mut request = fixture.request.clone();
    request.home = shadowed.clone();
    assert!(refused(&request).contains("override"));
    assert!(not_installed(&shadowed));

    // Overlapping owned allocations never install.
    let nested = fixture.request.home.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let mut request = fixture.request.clone();
    request.user_home = nested;
    assert!(refused(&request).contains("disjoint"));
    assert!(not_installed(&fixture.request.home));

    let nested_dependency = prepare_home(&root.join("nested-dep")).unwrap();
    fs::create_dir_all(nested_dependency.join("inner")).unwrap();
    let mut request = fixture.request.clone();
    request.user_home = nested_dependency.clone();
    request.dependency_user_home = nested_dependency.join("inner");
    let error = refused(&request);
    assert!(error.contains("nest"), "{error}");

    let mut request = fixture.request.clone();
    request.home = fixture.build.clone();
    let error = refused(&request);
    assert!(error.contains("overlaps the prepared runtime"), "{error}");

    let mut request = fixture.request.clone();
    request.protected = vec![fixture.request.user_home.join("workload")];
    let error = refused(&request);
    assert!(error.contains("protected"), "{error}");

    // Ambient homes are refused.
    let ambient = root.join("ambient-codex");
    fs::create_dir_all(&ambient).unwrap();
    let codex_home = EnvironmentGuard::capture("CODEX_HOME");
    codex_home.set(&ambient);
    let mut request = fixture.request.clone();
    request.home = ambient.clone();
    let error = refused(&request);
    assert!(error.contains("ambient"), "{error}");
    drop(codex_home);

    // Missing or mismatched client and runner inputs.
    let mut request = fixture.request.clone();
    request.client = Some(local_client(&root.join("missing-catalogue.json")));
    let error = refused(&request);
    assert!(error.contains("catalogue"), "{error}");

    let mut request = fixture.request.clone();
    request.upstream = root.join("missing-upstream.exe");
    assert!(refused(&request).contains("upstream client"));

    let mut request = fixture.request.clone();
    request.upstream = fixture.build.join("codex.exe");
    let error = refused(&request);
    assert!(error.contains("overlaps the prepared runtime"), "{error}");

    let decoy = root.join("decoy-client.exe");
    fs::copy(&fixtures.launcher, &decoy).unwrap();
    let mut request = fixture.request.clone();
    request.upstream = decoy;
    let error = refused(&request);
    assert!(error.contains("prepared harness binaries"), "{error}");

    let mut request = fixture.request.clone();
    request.timeout = Duration::ZERO;
    assert!(refused(&request).contains("timeout"));

    // Private inputs: unresolved source, escaping or reserved destinations.
    let mut request = fixture.request.clone();
    request.private_inputs = vec![PrivateInput {
        source: root.join("missing-private"),
        destination: "runner.private".into(),
    }];
    assert!(refused(&request).contains("runner.private"));

    let present = root.join("present-private");
    fs::write(&present, "value").unwrap();
    for destination in ["..\\escape", "harness/config.json", "config.toml"] {
        let mut request = fixture.request.clone();
        request.private_inputs = vec![PrivateInput {
            source: present.clone(),
            destination: destination.into(),
        }];
        let error = refused(&request);
        assert!(error.contains("private input"), "{destination}: {error}");
    }
    let mut request = fixture.request.clone();
    request.private_inputs = vec![
        PrivateInput {
            source: present.clone(),
            destination: "runner.private".into(),
        },
        PrivateInput {
            source: present.clone(),
            destination: "RUNNER.private".into(),
        },
    ];
    assert!(refused(&request).contains("duplicate"));

    // A leftover journal from an interrupted preparation refuses completion,
    // is preserved for inspection, and is restorable through the lifecycle
    // owner before a clean preparation succeeds.
    let journal = fixture
        .request
        .home
        .join("harness/native-registration/journal.json");
    fs::create_dir_all(journal.parent().unwrap()).unwrap();
    fs::write(&journal, "{\"schema\":11}").unwrap();
    let error = refused(&fixture.request);
    assert!(error.contains("preflight"), "{error}");
    assert!(not_installed(&fixture.request.home));
    assert!(journal.is_file(), "partial diagnostic state is preserved");

    // The lifecycle owner refuses to discard while pending recovery state
    // exists; the state is preserved until it is explicitly resolved.
    assert!(
        discard_arm(
            &fixture.request.home,
            &fixture.request.user_home,
            &fixture.request.dependency_user_home,
        )
        .is_err()
    );
    assert!(journal.is_file());
    fs::remove_file(&journal).unwrap();

    let discarded = discard_arm(
        &fixture.request.home,
        &fixture.request.user_home,
        &fixture.request.dependency_user_home,
    )
    .unwrap();
    assert_eq!(discarded.status, "not-connected");

    let runtime = install_arm(&fixture.request).unwrap();
    verify_consumption(&runtime).unwrap();
    assert!(fixture.request.home.join("AGENTS.md").exists());
    retire_arm(&runtime).unwrap();
    assert!(!fixture.request.home.join("AGENTS.md").exists());
}
