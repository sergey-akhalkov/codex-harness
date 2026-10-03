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
    build_identity, build_selection,
    improvement_experiment::{Arm, prepare_home, prepare_variant, select_variant},
    improvement_runtime::{
        ArmRequest, ClientInputs, PrivateInput, discard_arm, install_arm, retire_arm, select_arm,
        verify_consumption, verify_consumption_with_trust,
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

fn set_hooks(text: &mut String, name: &str, enabled: bool) {
    let mut replaced = String::new();
    let mut in_features = false;
    let mut seen_features = false;
    let mut done = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            if in_features && !done {
                replaced.push_str(&format!("{name} = {enabled}\n"));
                done = true;
            }
            in_features = trimmed == "[features]";
            seen_features |= in_features;
        }
        if in_features && !done && trimmed.starts_with(name) && line.contains('=') {
            replaced.push_str(&format!("{name} = {enabled}\n"));
            done = true;
            continue;
        }
        replaced.push_str(line);
        replaced.push('\n');
    }
    if in_features && !done {
        replaced.push_str(&format!("{name} = {enabled}\n"));
        done = true;
    }
    *text = if done {
        replaced
    } else if seen_features {
        format!("{replaced}{name} = {enabled}\n")
    } else {
        format!("{replaced}\n[features]\n{name} = {enabled}\n")
    };
}

fn hooks_state(text: &str, name: &str) -> bool {
    text.lines()
        .rev()
        .find(|line| line.trim_start().starts_with(name) && line.contains('='))
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
                (Some("disable"), Some(name @ ("hooks" | "code_mode"))) => {
                    set_hooks(&mut text, name, false);
                    fs::write(&config, text).unwrap();
                }
                (Some("enable"), Some(name @ ("hooks" | "code_mode"))) => {
                    set_hooks(&mut text, name, true);
                    fs::write(&config, text).unwrap();
                }
                (Some("list"), _) => {
                    println!("hooks stable {}", hooks_state(&text, "hooks"));
                    println!("code_mode experimental {}", hooks_state(&text, "code_mode"));
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
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nweb_search = \"disabled\"\n\n[features]\ncode_mode = true\napps = false\n",
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

fn local_client(catalogue: &Path, overlay: Option<&Path>) -> ClientInputs {
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
        overlay: overlay.map(Path::to_path_buf),
        executor_profile: None,
    }
}

/// Sorted relative-path to content-digest map of every ordinary file under a
/// directory, used to prove selection leaves sources and prepared builds
/// byte-identical.
fn tree_fingerprint(root: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(relative, build_identity::hash_file(&path).unwrap());
            }
        }
    }
    files
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
    // The qualified local client settings the route fields cannot express,
    // including false-valued features. The guard below is real: this false
    // declaration must shield the arm kit profile's `code_mode = true`.
    let overlay = root.join("qualified-client.config.toml");
    fs::write(
        &overlay,
        "model_context_window = 65536\nweb_search = \"disabled\"\napproval_policy = \"never\"\n\n[features]\ncode_mode = false\ncode_mode_only = false\n",
    )
    .unwrap();
    let secret = "private-runner-input-sentinel";
    let private_source = root.join("runner.private");
    fs::write(&private_source, secret).unwrap();
    for arm in [&mut baseline, &mut candidate] {
        arm.request.client = Some(local_client(&catalogue, Some(&overlay)));
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
        "model_context_window = 65536",
        "web_search = \"disabled\"",
        "code_mode = false",
        "code_mode_only = false",
        "hooks = false",
    ] {
        assert!(config_text.contains(expected), "{config_text}");
    }
    assert!(config_text.contains("model_catalog_json"));
    let baseline_kit_profile = baseline_config
        .kit_profile
        .as_ref()
        .expect("the declared client records its kit profile");
    assert!(
        baseline_config.overlay.is_some(),
        "the explicit client overlay is retained"
    );
    assert!(
        baseline_config
            .settings
            .iter()
            .any(|setting| setting.key == "features.code_mode"
                && setting.value == toml::Value::Boolean(false)),
        "the false feature value is a declared setting"
    );
    // Native precedence: the launcher computes the arm kit profile's launch
    // overrides; the false-valued feature shields the kit's `code_mode = true`
    // while the kit's other defaults still reach the launch.
    let launch_args = harness_core::portable_config::overrides(
        &baseline_kit_profile.path,
        &baseline_runtime.home,
        &baseline_runtime.home,
    )
    .unwrap()
    .iter()
    .map(|value| value.to_string_lossy().into_owned())
    .collect::<Vec<_>>();
    assert!(
        launch_args.iter().any(|arg| arg == "features.apps=false"),
        "{launch_args:?}"
    );
    assert!(
        !launch_args
            .iter()
            .any(|arg| arg.starts_with("features.code_mode=")),
        "the declared false value must shield the kit profile's code_mode: {launch_args:?}"
    );
    assert!(
        launch_args
            .iter()
            .any(|arg| arg == "web_search=\"disabled\"")
    );
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

    // A changed declared setting refuses even when the file is otherwise the
    // prepared one: the effective client identity is the declared value, not
    // merely the file's presence.
    let falsified = candidate_config_bytes
        .windows(b"code_mode = false".len())
        .position(|window| window == b"code_mode = false")
        .expect("the prepared configuration declares the false feature");
    let mut changed = candidate_config_bytes.clone();
    changed[falsified..falsified + b"code_mode = false".len()]
        .copy_from_slice(b"code_mode = true ");
    fs::write(&candidate_config.path, &changed).unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("configuration") || error.contains("code_mode"),
        "{error}"
    );
    fs::write(&candidate_config.path, &candidate_config_bytes).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    // The explicit overlay is an external, drift-checked input.
    let overlay_bytes = fs::read(&overlay).unwrap();
    fs::write(
        &overlay,
        "model_context_window = 1\nweb_search = \"disabled\"\napproval_policy = \"never\"\n\n[features]\ncode_mode = false\ncode_mode_only = false\n",
    )
    .unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(error.contains("overlay"), "{error}");
    fs::write(&overlay, &overlay_bytes).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    // The arm kit profile feeds the launcher's precedence: changing it refuses
    // that arm while the other arm stays consumed.
    let kit_profile = candidate.source.join("global/harness.config.toml");
    let kit_profile_bytes = fs::read(&kit_profile).unwrap();
    fs::write(
        &kit_profile,
        [kit_profile_bytes.as_slice(), b"web_search = \"enabled\"\n"].concat(),
    )
    .unwrap();
    let error = verify_consumption(&candidate_runtime)
        .unwrap_err()
        .to_string();
    assert!(error.contains("kit profile"), "{error}");
    verify_consumption(&baseline_runtime).unwrap();
    fs::write(&kit_profile, &kit_profile_bytes).unwrap();
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

/// The measured dispatch's own trust write is the only configuration
/// exception: exactly the workspaces an arm's own recorded dispatches
/// allocated may carry a trusted-project entry, and only at trust_level
/// "trusted". An unrecorded workspace, a changed trust level, an extra setting
/// and ordinary model drift all keep refusing the arm.
#[test]
fn consumption_authorizes_only_recorded_dispatch_workspaces() {
    let _serial = INSTALL.lock().unwrap();
    let temp = tempfile::Builder::new()
        .prefix("improvement-runtime-trust-")
        .tempdir()
        .unwrap();
    let root = temp.path();
    let fixtures = fixtures();
    let _cpu = EnvironmentGuard::capture("CODEX_HARNESS_CPU_ACCOUNT");
    fs::create_dir_all(root.join("cpu-account")).unwrap();
    _cpu.set(&root.join("cpu-account"));
    let _path = EnvironmentGuard::capture("PATH");
    let state = owned_state(root);
    let mut arm = arm_fixture(
        root,
        &state,
        "trust",
        Arm::Baseline,
        "H",
        "trust",
        &fixtures.launcher,
        &fixtures.upstream,
    );
    let catalogue = root.join("model-catalogue.json");
    fs::write(&catalogue, br#"{"models":[{"name":"fixture-glyph-1"}]}"#).unwrap();
    arm.request.client = Some(local_client(&catalogue, None));
    arm.request.client.as_mut().unwrap().executor_profile = Some("workload-executor".into());
    let runtime = install_arm(&arm.request).unwrap();
    let config = runtime.configuration.as_ref().unwrap().path.clone();
    let prepared = fs::read(&config).unwrap();
    let text = String::from_utf8(prepared.clone()).unwrap();
    assert!(
        text.contains("[profiles.workload-executor]"),
        "the declared executor profile mirrors the qualified route: {text}"
    );

    let workspace = root.join("checkout-wt1");
    let owned = workspace.to_string_lossy().into_owned();
    let with_trust = |workspace: &str| {
        let mut text = String::from_utf8(prepared.clone()).unwrap();
        text.push_str(&format!(
            "\n[projects.'{}']\ntrust_level = \"trusted\"\n",
            workspace.to_ascii_lowercase()
        ));
        text.into_bytes()
    };

    // The dispatch's own addition is not authority by itself: with no recorded
    // dispatch workspace the strict check still refuses it.
    let recorded = with_trust(&owned);
    fs::write(&config, &recorded).unwrap();
    assert!(verify_consumption(&runtime).is_err());

    // The recorded workspace authorizes exactly its own trusted-project entry,
    // under ordinary Windows path spelling.
    verify_consumption_with_trust(&runtime, std::slice::from_ref(&workspace)).unwrap();
    let spelled = PathBuf::from(format!(
        r"\\?\{}\",
        owned.replace('\\', "/").to_ascii_uppercase()
    ));
    verify_consumption_with_trust(&runtime, &[spelled]).unwrap();

    // Everything else keeps refusing: an unrelated trusted workspace, a
    // changed trust level, an extra setting and ordinary model drift.
    let unrelated = with_trust(r"c:\unrelated-workspace");
    let changed_level = String::from_utf8(recorded.clone())
        .unwrap()
        .replace("trust_level = \"trusted\"", "trust_level = \"untrusted\"")
        .into_bytes();
    let mut extra = recorded.clone();
    extra.extend_from_slice(b"\n[fixture-extra]\nvalue = 1\n");
    let mut model_drift = recorded.clone();
    let needle = b"model = \"fixture-glyph-1\"";
    let at = model_drift
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("the prepared configuration declares the route model");
    model_drift[at..at + b"fixture-glyph-1".len()].copy_from_slice(b"fixture-glyph-2");
    for (name, mutated) in [
        ("an unrelated trusted workspace", unrelated),
        ("a changed trust level", changed_level),
        ("an extra configuration setting", extra),
        ("ordinary model drift", model_drift),
    ] {
        fs::write(&config, &mutated).unwrap();
        let error = verify_consumption_with_trust(&runtime, std::slice::from_ref(&workspace))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("configuration changed since preparation"),
            "{name}: {error}"
        );
    }

    // The recorded addition alone is accepted again after the counterexamples.
    fs::write(&config, &recorded).unwrap();
    verify_consumption_with_trust(&runtime, std::slice::from_ref(&workspace)).unwrap();
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
    let present_catalogue = root.join("present-catalogue.json");
    fs::write(&present_catalogue, br#"{"models":[]}"#).unwrap();
    let mut request = fixture.request.clone();
    request.client = Some(local_client(&root.join("missing-catalogue.json"), None));
    let error = refused(&request);
    assert!(error.contains("catalogue"), "{error}");

    // Explicit client overlay refusals: unresolved, contradictory or route
    // replacing declarations never become an apparently ready arm.
    let mut request = fixture.request.clone();
    request.client = Some(local_client(
        &present_catalogue,
        Some(&root.join("missing-overlay.toml")),
    ));
    let error = refused(&request);
    assert!(error.contains("overlay"), "{error}");
    assert!(not_installed(&fixture.request.home));

    let overlay = |name: &str, body: &str| -> PathBuf {
        let path = root.join(name);
        fs::write(&path, body).unwrap();
        path
    };
    let cases: [(&str, &str, &str); 7] = [
        (
            "overlay-model.config.toml",
            "model = \"other-model\"\n",
            "must not replace the accepted model",
        ),
        (
            "overlay-provider.config.toml",
            "model_provider = \"xai\"\n",
            "must not replace the accepted model_provider",
        ),
        (
            "overlay-effort.config.toml",
            "model_reasoning_effort = \"high\"\n",
            "must not replace the accepted model_reasoning_effort",
        ),
        (
            "overlay-endpoint.config.toml",
            "[model_providers.local]\nbase_url = \"http://127.0.0.1:1/v1\"\n",
            "must not replace the accepted local endpoint",
        ),
        (
            "overlay-route.config.toml",
            "[model_providers.xai]\nname = \"xAI\"\nbase_url = \"http://127.0.0.1:2/v1\"\nwire_api = \"responses\"\n",
            "alternate provider routes",
        ),
        (
            "overlay-hooks.config.toml",
            "[features]\nhooks = true\n",
            "hooks",
        ),
        (
            "overlay-trust.config.toml",
            "[projects.'C:\\synthetic']\ntrust_level = \"trusted\"\n",
            "projects",
        ),
    ];
    for (name, body, expected) in cases {
        let mut request = fixture.request.clone();
        request.client = Some(local_client(&present_catalogue, Some(&overlay(name, body))));
        let error = refused(&request);
        assert!(error.contains(expected), "{name}: {error}");
        assert!(not_installed(&fixture.request.home), "{name}");
    }
    let mut request = fixture.request.clone();
    request.client = Some(local_client(
        &present_catalogue,
        Some(&overlay("overlay-invalid.config.toml", "model = [\n")),
    ));
    let error = refused(&request);
    assert!(error.contains("overlay"), "{error}");

    // A local endpoint carrying credentials is refused before any arm is
    // prepared: route credentials belong in explicit private inputs, never in
    // the declared endpoint record or the retained client configuration.
    let mut request = fixture.request.clone();
    let mut credentialed = local_client(&present_catalogue, None);
    credentialed.runner.endpoint = "http://user:secret@127.0.0.1:45999/v1".to_owned();
    request.client = Some(credentialed);
    let error = refused(&request);
    assert!(error.contains("credential-free"), "{error}");
    assert!(not_installed(&fixture.request.home));

    // A declared setting the arm's own kit profile would override at launch is
    // refused instead of being recorded as consumed.
    let mut request = fixture.request.clone();
    request.client = Some(local_client(
        &present_catalogue,
        Some(&overlay(
            "overlay-approval.config.toml",
            "approval_policy = \"on-request\"\n",
        )),
    ));
    let error = refused(&request);
    assert!(error.contains("would override"), "{error}");
    assert!(not_installed(&fixture.request.home));

    // A reused home whose configuration no longer carries the declared client
    // settings refuses instead of being merged.
    let reused_home = prepare_home(&root.join("reused-home")).unwrap();
    fs::write(
        reused_home.join("config.toml"),
        "model = \"other-model\"\nmodel_provider = \"local\"\n",
    )
    .unwrap();
    let mut request = fixture.request.clone();
    request.home = reused_home.clone();
    request.client = Some(local_client(&present_catalogue, None));
    let error = refused(&request);
    assert!(
        error.contains("does not match the declared client inputs"),
        "{error}"
    );
    assert!(not_installed(&reused_home));

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

/// Advertised token workflow is prepared through its lifecycle owner. Declared
/// client settings still hold after that owner's feature edits, and a changed
/// owned command refuses consumption. An advertised component with no vendor
/// artifact refuses readiness instead of using another copy.
#[test]
fn advertised_token_workflow_is_prepared_and_drift_refuses_consumption() {
    let _serial = INSTALL.lock().unwrap();
    let temp = tempfile::Builder::new()
        .prefix("improvement-runtime-rtk-")
        .tempdir()
        .unwrap();
    let root = temp.path();
    let fixtures = fixtures();
    let _cpu = EnvironmentGuard::capture("CODEX_HARNESS_CPU_ACCOUNT");
    fs::create_dir_all(root.join("cpu-account")).unwrap();
    _cpu.set(&root.join("cpu-account"));
    let _path = EnvironmentGuard::capture("PATH");
    let state = owned_state(root);
    let source = kit_source(root, "kit-token", "token");
    for relative in [
        "crates/harness-rtk/Cargo.toml",
        "crates/harness-rtk/src/main.rs",
    ] {
        let path = source.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, relative.as_bytes()).unwrap();
    }
    let build = fixture_build(&state, "token", &source, &fixtures.launcher, "token");
    let home = prepare_home(&root.join("homes/token")).unwrap();
    let user_home = prepare_home(&root.join("homes/token-user")).unwrap();
    let dependency_user_home = prepare_home(&root.join("homes/token-dep")).unwrap();
    let vendor = home.join("harness/rtk/packages/0.48.0/rtk.exe");
    fs::create_dir_all(vendor.parent().unwrap()).unwrap();
    fs::write(&vendor, b"staged-rtk-for-arm").unwrap();
    fs::write(
        source.join("global/rtk.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": "0.48.0",
            "executableSha256": build_identity::hash_file(&vendor).unwrap(),
        }))
        .unwrap(),
    )
    .unwrap();
    let identity =
        harness_core::token_workflow_lifecycle::component_source_identity(&source).unwrap();
    let adapter = home.join(format!("harness/rtk/build/{identity}/harness-rtk.exe"));
    fs::create_dir_all(adapter.parent().unwrap()).unwrap();
    fs::write(&adapter, b"staged-adapter-for-arm").unwrap();
    fs::write(
        adapter.parent().unwrap().join("build.json"),
        serde_json::to_vec(&serde_json::json!({
            "sourceIdentity": identity,
            "binarySha256": build_identity::hash_file(&adapter).unwrap(),
        }))
        .unwrap(),
    )
    .unwrap();
    let catalogue = root.join("model-catalogue.json");
    fs::write(&catalogue, br#"{"models":[{"name":"fixture-glyph-1"}]}"#).unwrap();
    let overlay = root.join("qualified-client.config.toml");
    fs::write(
        &overlay,
        "model_context_window = 65536\nweb_search = \"disabled\"\napproval_policy = \"never\"\n\n[features]\ncode_mode = false\n",
    )
    .unwrap();
    let request = ArmRequest {
        variant: prepare_variant(&state, Arm::Baseline, "H", &build).unwrap(),
        home: home.clone(),
        user_home: user_home.clone(),
        dependency_user_home,
        upstream: fixtures.upstream.clone(),
        timeout: Duration::from_secs(60),
        client: Some(local_client(&catalogue, Some(&overlay))),
        private_inputs: Vec::new(),
        protected: Vec::new(),
    };
    let runtime = install_arm(&request).unwrap();
    verify_consumption(&runtime).unwrap();
    let component = runtime
        .components
        .iter()
        .find(|component| component.name == "token-workflow")
        .expect("token workflow identity is retained");
    assert_eq!(component.status, "Token workflow connected");
    assert_eq!(component.vendor_version.as_deref(), Some("0.48.0"));
    assert_eq!(component.model_calls, 0);
    assert_eq!(
        component.source_identity.as_deref(),
        Some(identity.as_str())
    );
    for name in ["rtk.exe", "harness-rtk.exe"] {
        let link = component
            .links
            .iter()
            .find(|link| link.name == name)
            .unwrap_or_else(|| panic!("{name} was not recorded"));
        let destination = link.destination.to_string_lossy().to_ascii_lowercase();
        let destination = destination
            .strip_prefix(r"\\?\")
            .unwrap_or(&destination)
            .to_owned();
        let expected = home
            .join("harness/bin")
            .to_string_lossy()
            .to_ascii_lowercase();
        let expected = expected
            .strip_prefix(r"\\?\")
            .unwrap_or(&expected)
            .to_owned();
        assert!(
            destination.starts_with(&expected),
            "{name} destination {destination} is not under {expected}"
        );
        assert_eq!(
            build_identity::hash_file(&link.source).unwrap(),
            link.sha256
        );
    }
    let config = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(
        config.contains("fixture-glyph-1"),
        "local model selection did not survive component setup: {config}"
    );
    assert!(
        config.contains("code_mode = false"),
        "declared client feature did not hold after token-workflow setup: {config}"
    );
    let adapter_link = component
        .links
        .iter()
        .find(|link| link.name == "harness-rtk.exe")
        .unwrap();
    let original = fs::read(&adapter_link.source).unwrap();
    fs::write(&adapter_link.source, b"changed-adapter").unwrap();
    let drifted = verify_consumption(&runtime).unwrap_err().to_string();
    assert!(
        drifted.contains("changed since preparation"),
        "changed token-workflow command was accepted: {drifted}"
    );
    fs::write(&adapter_link.source, original).unwrap();
    verify_consumption(&runtime).unwrap();
    retire_arm(&runtime).unwrap();
    assert!(!home.join("harness/bin/harness-rtk.exe").exists());
    assert!(!home.join("harness/bin/rtk.exe").exists());

    let missing = kit_source(root, "kit-missing-token", "missing");
    for relative in [
        "crates/harness-rtk/Cargo.toml",
        "crates/harness-rtk/src/main.rs",
    ] {
        let path = missing.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, relative.as_bytes()).unwrap();
    }
    fs::write(
        missing.join("global/rtk.json"),
        br#"{"version":"0.48.0","executableSha256":"abc"}"#,
    )
    .unwrap();
    let missing_build = fixture_build(
        &state,
        "missing-token",
        &missing,
        &fixtures.launcher,
        "missing",
    );
    let missing_home = prepare_home(&root.join("homes/missing")).unwrap();
    let missing_request = ArmRequest {
        variant: prepare_variant(&state, Arm::Candidate, "H+A", &missing_build).unwrap(),
        home: missing_home.clone(),
        user_home: prepare_home(&root.join("homes/missing-user")).unwrap(),
        dependency_user_home: prepare_home(&root.join("homes/missing-dep")).unwrap(),
        upstream: fixtures.upstream.clone(),
        timeout: Duration::from_secs(60),
        client: None,
        private_inputs: Vec::new(),
        protected: vec![home, source],
    };
    let refused = install_arm(&missing_request).unwrap_err().to_string();
    assert!(
        refused.contains("token-workflow") && refused.contains("readiness is refused"),
        "unprepared token workflow was ready: {refused}"
    );
    assert!(
        !missing_home.join("harness/bin/harness-rtk.exe").exists(),
        "a missing component still published a command"
    );
}

/// Change task 2.5: repeated off/on selection of the prepared
/// baseline/candidate arms goes through the existing runtime-selection owner,
/// reports the identity actually consumed, changes no source or build
/// artifact, makes no model call, refuses mid-attempt changes, and keeps the
/// shared and per-arm state controlled.
#[test]
fn prepared_arm_selection_cycles_without_source_rebuild_or_model_call() {
    let _serial = INSTALL.lock().unwrap();
    let temp = tempfile::Builder::new()
        .prefix("improvement-runtime-selection-")
        .tempdir()
        .unwrap();
    let root = temp.path();
    let fixtures = fixtures();
    let _cpu = EnvironmentGuard::capture("CODEX_HARNESS_CPU_ACCOUNT");
    fs::create_dir_all(root.join("cpu-account")).unwrap();
    _cpu.set(&root.join("cpu-account"));
    let _path = EnvironmentGuard::capture("PATH");
    let state = owned_state(root);

    let baseline = arm_fixture(
        root,
        &state,
        "baseline",
        Arm::Baseline,
        "H",
        "baseline",
        &fixtures.launcher,
        &fixtures.upstream,
    );
    let candidate = arm_fixture(
        root,
        &state,
        "candidate",
        Arm::Candidate,
        "H+A",
        "candidate",
        &fixtures.launcher,
        &fixtures.upstream,
    );
    let catalogue = root.join("model-catalogue.json");
    fs::write(&catalogue, br#"{"models":[{"name":"fixture-glyph-1"}]}"#).unwrap();
    let mut baseline_request = baseline.request;
    let mut candidate_request = candidate.request;
    for request in [&mut baseline_request, &mut candidate_request] {
        request.client = Some(local_client(&catalogue, None));
    }
    let baseline_runtime = install_arm(&baseline_request).unwrap();
    let candidate_runtime = install_arm(&candidate_request).unwrap();
    assert!(baseline_runtime.client_configured());
    assert!(candidate_runtime.client_configured());

    // The shared owned state holds both prepared immutable builds; each arm
    // home owns its own configuration and content.
    let baseline_sources = tree_fingerprint(&baseline.source);
    let candidate_sources = tree_fingerprint(&candidate.source);
    let baseline_build_files = tree_fingerprint(&baseline.build);
    let candidate_build_files = tree_fingerprint(&candidate.build);
    let build_dirs = |state: &Path| {
        let mut names: Vec<String> = fs::read_dir(state.join("builds"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let builds_before = build_dirs(&state);
    let baseline_config = baseline_runtime
        .configuration
        .as_ref()
        .unwrap()
        .path
        .clone();
    let candidate_config = candidate_runtime
        .configuration
        .as_ref()
        .unwrap()
        .path
        .clone();
    assert_ne!(baseline_config, candidate_config);
    let baseline_config_bytes = fs::read(&baseline_config).unwrap();
    let candidate_config_bytes = fs::read(&candidate_config).unwrap();

    // Off/on selection: the reported identity is the artifact the selection
    // owner actually activates and the one this arm home consumes.
    let canonical = |path: &Path| fs::canonicalize(path).unwrap();
    let active = || canonical(&build_selection::selected(&state).unwrap().0);
    let baseline_on = select_arm(&state, &baseline_runtime, false).unwrap();
    assert!(baseline_on.applied);
    assert_eq!(baseline_on.arm, Arm::Baseline);
    assert_eq!(baseline_on.label, "H");
    assert_eq!(baseline_on.model_calls, 0);
    assert!(baseline_on.consumption.model_ready);
    assert_eq!(
        baseline_on.selected.manager_sha256.as_deref(),
        Some(
            build_identity::hash_file(&baseline.build.join("codex-harness.exe"))
                .unwrap()
                .as_str()
        )
    );
    assert_eq!(active(), canonical(&baseline_on.consumption.build));
    assert_eq!(active(), canonical(&baseline_on.selected.build));
    assert_eq!(
        baseline_on.selected.record_sha256,
        baseline_on.consumption.record_sha256
    );

    let repeated = select_arm(&state, &baseline_runtime, false).unwrap();
    assert!(!repeated.applied, "an unchanged variant is not reselected");
    assert_eq!(repeated.consumption, baseline_on.consumption);
    assert_eq!(repeated.selected.build, baseline_on.selected.build);

    let candidate_on = select_arm(&state, &candidate_runtime, false).unwrap();
    assert!(candidate_on.applied);
    assert_eq!(candidate_on.arm, Arm::Candidate);
    assert_eq!(candidate_on.label, "H+A");
    assert_eq!(candidate_on.model_calls, 0);
    assert_eq!(active(), canonical(&candidate_on.consumption.build));
    assert_ne!(candidate_on.selected.build, baseline_on.selected.build);
    let candidate_repeated = select_arm(&state, &candidate_runtime, false).unwrap();
    assert!(!candidate_repeated.applied);

    let baseline_back = select_arm(&state, &baseline_runtime, false).unwrap();
    assert!(baseline_back.applied);
    assert_eq!(
        baseline_back.consumption.build,
        baseline_on.consumption.build
    );
    assert_eq!(active(), canonical(&baseline_on.consumption.build));
    let candidate_again = select_arm(&state, &candidate_runtime, false).unwrap();
    assert!(candidate_again.applied);
    assert_eq!(active(), canonical(&candidate_on.consumption.build));

    // An active measured attempt freezes whichever runtime it holds: every
    // selection attempt is refused and the active pointer does not move.
    for runtime in [&baseline_runtime, &candidate_runtime] {
        let error = select_arm(&state, runtime, true).unwrap_err().to_string();
        assert!(error.contains("active"), "{error}");
    }
    assert_eq!(active(), canonical(&candidate_on.consumption.build));

    // No source edit and no rebuild: sources, prepared builds, the published
    // build set and both arm configurations are byte-identical afterwards.
    assert_eq!(tree_fingerprint(&baseline.source), baseline_sources);
    assert_eq!(tree_fingerprint(&candidate.source), candidate_sources);
    assert_eq!(tree_fingerprint(&baseline.build), baseline_build_files);
    assert_eq!(tree_fingerprint(&candidate.build), candidate_build_files);
    assert_eq!(build_dirs(&state), builds_before);
    assert_eq!(fs::read(&baseline_config).unwrap(), baseline_config_bytes);
    assert_eq!(fs::read(&candidate_config).unwrap(), candidate_config_bytes);

    // Per-arm isolation survived the switching: each home still consumes its
    // own instructions, skills and build.
    let baseline_text = fs::read_to_string(baseline_runtime.home.join("AGENTS.md")).unwrap();
    let candidate_text = fs::read_to_string(candidate_runtime.home.join("AGENTS.md")).unwrap();
    assert!(baseline_text.contains("baseline arm instructions"));
    assert!(candidate_text.contains("candidate arm instructions"));
    verify_consumption(&baseline_runtime).unwrap();
    verify_consumption(&candidate_runtime).unwrap();

    // A drifted arm refuses selection without moving the pointer and without
    // repairing or rewriting its source; the other arm stays selectable.
    let skill = candidate.source.join(".agents/skills/arm-skill/SKILL.md");
    let original_skill = fs::read(&skill).unwrap();
    let drifted_skill = [original_skill.as_slice(), b"\ndrift"].concat();
    fs::write(&skill, &drifted_skill).unwrap();
    let error = select_arm(&state, &candidate_runtime, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("changed since preparation"), "{error}");
    assert_eq!(active(), canonical(&candidate_on.consumption.build));
    assert_eq!(fs::read(&skill).unwrap(), drifted_skill);
    let baseline_again = select_arm(&state, &baseline_runtime, false).unwrap();
    assert!(baseline_again.applied);
    assert_eq!(active(), canonical(&baseline_on.consumption.build));
    fs::write(&skill, &original_skill).unwrap();
    let candidate_restored = select_arm(&state, &candidate_runtime, false).unwrap();
    assert!(candidate_restored.applied);
    assert_eq!(active(), canonical(&candidate_on.consumption.build));

    retire_arm(&baseline_runtime).unwrap();
    retire_arm(&candidate_runtime).unwrap();
}
