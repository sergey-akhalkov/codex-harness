//! One explicit native outcome attempt. Correctness remains the caller's
//! oracle. The launcher runs hidden with captured streams: this route creates
//! no visible terminal or conversation, which only the dispatch path provides.
#[path = "outcome_events.rs"]
mod events;
#[path = "process_result.rs"]
mod process_result;

use harness_core::build_identity::{hash_file, ordinary};
use harness_core::outcome_qualification::{
    ApiObservationPlan, ApiObservations, ApiObservedPolicy, ApiObservedQualification, ClientInput,
    LocalRunner, MATERIAL_FIELDS, QualificationAttempt, RunnerRecord, WIRE_API,
    collect_observations, observation_changes, qualification_attempt, qualify_api_observed,
    recheck_observations,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
};

const MODEL: &str = "gpt-6-astra";
const EFFORT: &str = "xhigh";
const XAI_MODEL: &str = "grok-4.6";
const INPUT_LIMIT: u64 = 4 * 1024 * 1024;
const PROMPT_LIMIT: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    case_root: PathBuf,
    codex_home: PathBuf,
    /// The caller selects the native linked launcher, never a PATH search.
    launcher: PathBuf,
    prompt: String,
    #[serde(default = "default_timeout")]
    timeout: u64,
    #[serde(default = "default_output_limit")]
    output_limit: u64,
    #[serde(default)]
    extra_config: BTreeMap<String, Value>,
    useful_command_pattern: Option<String>,
    cancel_file: Option<PathBuf>,
    #[serde(default)]
    user_home: Option<PathBuf>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    xai_auth: Option<XaiAuth>,
    /// Explicit local runner configuration. Mutually exclusive with `profile`.
    #[serde(default)]
    runner: Option<LocalRunner>,
    /// Optional declared API-observation plan for the explicit local runner:
    /// bounded declared JSON fields from explicit local API sources plus
    /// effective client inputs observed by digest. Collection is model-free
    /// and fails the attempt before launch when a required observation is
    /// unavailable; the declared plan never changes the route or its model.
    #[serde(default)]
    api_observations: Option<ApiObservationPlan>,
    /// Explicit effective client inputs (profile, catalogue files) for the
    /// declared observation plan. Paths stay private evidence.
    #[serde(default)]
    client_inputs: Vec<ClientInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct XaiAuth {
    command: PathBuf,
    home: PathBuf,
}
fn default_timeout() -> u64 {
    600
}
fn default_output_limit() -> u64 {
    128 * 1024 * 1024
}

/// One explicit invocation of this command. The observation modes never
/// launch a process or call a model; only the native attempt mode runs the
/// selected launcher.
enum Invocation {
    Attempt { request: PathBuf, probes: bool },
    Observations(PathBuf),
    Qualify(PathBuf),
    Recheck(PathBuf),
    Skipped,
}

/// Declared collection inputs for the model-free `--observations` mode.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationsRequest {
    runner: LocalRunner,
    plan: ApiObservationPlan,
    #[serde(default)]
    client_inputs: Vec<ClientInput>,
}

/// Declared qualification inputs for the model-free `--qualify` mode: the
/// policy fixed before repeats, the collected observations it binds to, and
/// the retained attempts. No launcher, case or evidence root is involved.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QualifyRequest {
    runner: LocalRunner,
    policy: ApiObservedPolicy,
    observations: ApiObservations,
    #[serde(default)]
    attempts: Vec<RetainedAttempt>,
}

/// One retained attempt: either an already-extracted attempt record or the
/// retained native result together with its required-output digests, which are
/// extracted through the same conservative owner the controller uses.
#[derive(Deserialize)]
#[serde(untagged)]
enum RetainedAttempt {
    Record(Box<QualificationAttempt>),
    Result {
        result: Value,
        #[serde(default)]
        outputs: BTreeMap<String, String>,
    },
}

impl RetainedAttempt {
    fn extract(&self) -> io::Result<QualificationAttempt> {
        match self {
            Self::Record(attempt) => Ok((**attempt).clone()),
            Self::Result { result, outputs } => qualification_attempt(result, outputs.clone()),
        }
    }
}

/// Declared pre-arm recheck inputs for the model-free `--recheck` mode: the
/// retained qualification, the current declared runner and policy, and the
/// explicit client inputs of the declaration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecheckRequest {
    qualification: ApiObservedQualification,
    runner: LocalRunner,
    policy: ApiObservedPolicy,
    #[serde(default)]
    client_inputs: Vec<ClientInput>,
}

fn read_input<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    let bytes = bounded_read(path, INPUT_LIMIT).map_err(|_| invalid())?;
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

fn print_result(value: &Value) -> io::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn option(iter: &mut std::slice::Iter<'_, OsString>, slot: &mut Option<PathBuf>) -> io::Result<()> {
    let value = iter.next().ok_or_else(invalid)?;
    if slot.is_some() {
        return Err(invalid());
    }
    *slot = Some(PathBuf::from(value));
    Ok(())
}

fn parse_invocation(args: &[OsString]) -> io::Result<Invocation> {
    let (mut request, mut probes) = (None, false);
    let (mut observations, mut qualify, mut recheck) = (None, None, None);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--request" {
            option(&mut iter, &mut request)?;
        } else if arg == "--run-model-probes" {
            if probes {
                return Err(invalid());
            }
            probes = true;
        } else if arg == "--observations" {
            option(&mut iter, &mut observations)?;
        } else if arg == "--qualify" {
            option(&mut iter, &mut qualify)?;
        } else if arg == "--recheck" {
            option(&mut iter, &mut recheck)?;
        } else {
            return Err(invalid());
        }
    }
    // The model-free observation modes are exclusive: they never combine with
    // the native attempt mode or with each other.
    let declared = [observations.is_some(), qualify.is_some(), recheck.is_some()]
        .iter()
        .filter(|declared| **declared)
        .count();
    if declared > 1
        || (declared == 1 && (request.is_some() || probes))
        || (observations.is_none()
            && qualify.is_none()
            && recheck.is_none()
            && probes
            && request.is_none())
    {
        return Err(invalid());
    }
    Ok(match (request, observations, qualify, recheck) {
        (Some(request), None, None, None) => Invocation::Attempt { request, probes },
        (None, Some(path), None, None) => Invocation::Observations(path),
        (None, None, Some(path), None) => Invocation::Qualify(path),
        (None, None, None, Some(path)) => Invocation::Recheck(path),
        (None, None, None, None) => Invocation::Skipped,
        _ => return Err(invalid()),
    })
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "codex-harness outcome-run [--request PATH --run-model-probes | --observations PATH | --qualify PATH | --recheck PATH]\n\
             Runs one explicitly selected native launcher hidden with captured streams in an isolated temporary case/home (no visible terminal; visible dispatch is a separate path). An explicit `runner` selects the local endpoint/model (Responses API); provider changes stay invalid treatment settings and no other route is used as a fallback. An explicit `api_observations` plan collects bounded declared local API fields and effective client-input digests, model-free, before and after the attempt.\n\
             --observations PATH collects the declared bounded local API fields and effective client-input digests without launching a process or calling a model.\n\
             --qualify PATH evaluates retained native attempts under the declared API-observed policy without launching a process or calling a model.\n\
             --recheck PATH re-collects the declared observations and refuses dependent comparisons when a retained qualification is internally inconsistent or no longer matches; it launches nothing and calls no model.\n\
             The model-free modes print one JSON object on stdout and exit 0 (collected/qualified/unchanged), 1 (failed/blocked/drifted) or 2 (invalid input). Private evidence is retained; correctness requires a separate oracle."
        );
        return Ok(0);
    }
    match parse_invocation(args)? {
        Invocation::Attempt {
            request,
            probes: true,
        } => {
            let request: Request = read_input(&request)?;
            let result = attempt(request)?;
            let code = if result["status"] == "completed" {
                0
            } else {
                1
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(code)
        }
        Invocation::Observations(path) => observations_mode(&read_input(&path)?),
        Invocation::Qualify(path) => qualify_mode(&read_input(&path)?),
        Invocation::Recheck(path) => recheck_mode(&read_input(&path)?),
        // No request read, discovery, model or evidence-directory creation by
        // default.
        Invocation::Attempt { probes: false, .. } | Invocation::Skipped => {
            println!(
                "{}",
                json!({"status":"skipped","reason":"explicit --run-model-probes required","model_calls":0})
            );
            Ok(0)
        }
    }
}

/// Collects the declared bounded local API fields and effective client-input
/// digests. Model-free and process-free: it performs only the declared reads.
fn observations_mode(request: &ObservationsRequest) -> io::Result<i32> {
    validate_local_runner(&request.runner)?;
    match collect_observations(&request.runner, &request.plan, &request.client_inputs) {
        Ok(observations) => {
            print_result(&json!({"status": "collected", "observations": observations}))?;
            Ok(0)
        }
        Err(failure) => {
            print_result(&json!({"status": "failed", "observation_failure": failure}))?;
            Ok(1)
        }
    }
}

/// Qualifies retained native attempts under the declared API-observed policy.
/// Model-free and process-free: it consumes the retained attempts and the
/// collected observations as declared evidence.
fn qualify_mode(request: &QualifyRequest) -> io::Result<i32> {
    validate_local_runner(&request.runner)?;
    let attempts: Vec<QualificationAttempt> = request
        .attempts
        .iter()
        .map(RetainedAttempt::extract)
        .collect::<io::Result<_>>()?;
    let qualification = qualify_api_observed(
        &request.runner,
        &request.policy,
        &request.observations,
        &attempts,
    )?;
    let code = if qualification.qualified() { 0 } else { 1 };
    print_result(&json!({
        "status": if qualification.qualified() { "qualified" } else { "blocked" },
        "qualification": qualification,
    }))?;
    Ok(code)
}

/// Re-collects the declared observations and rechecks a retained
/// qualification before an arm. Model-free and process-free.
fn recheck_mode(request: &RecheckRequest) -> io::Result<i32> {
    validate_local_runner(&request.runner)?;
    let drift = recheck_observations(
        &request.qualification,
        &request.runner,
        &request.policy,
        &request.client_inputs,
    );
    let code = if drift.drifted { 1 } else { 0 };
    print_result(&json!({
        "status": if drift.drifted { "drifted" } else { "unchanged" },
        "drift": drift,
    }))?;
    Ok(code)
}

fn attempt(request: Request) -> io::Result<Value> {
    let private = tempfile::Builder::new()
        .prefix("codex-outcome-native-")
        .tempdir()?;
    let root = private.path().canonicalize()?;
    if root.starts_with(repository()) {
        return Err(invalid());
    }
    // Preserve every started attempt, including validation/launch failure.
    let _ = private.keep();
    let started = events::now();
    let mut result = json!({"evidence_root":root,"status":"incomplete",
        "started_at":started,"ended_at":null,"thread_id":null,"children":[],
        "rollout_paths":[],"first_useful_signal":null,
        "usage":{"status":"unknown","total_tokens":null},
        "final_path":root.join("final.txt"),"result_path":root.join("result.json")});
    write_new(&root.join("native-started.json"), &result)?;
    if let Err(error) = execute(&request, &root, &mut result) {
        let phase = result
            .as_object_mut()
            .expect("owned result")
            .remove("failure_phase")
            .unwrap_or(json!("validation"));
        result["status"] = json!("failed");
        result["error_type"] = json!("native_outcome_failure");
        // Neither parser text, prompt nor command arguments enter public errors.
        write_new(
            &root.join("failure.json"),
            &json!({"phase":phase,"kind":format!("{:?}",error.kind()),"raw_os_error":error.raw_os_error(),"message":error.to_string()}),
        )?;
    }
    result["ended_at"] = json!(events::now());
    result["elapsed_seconds"] = json!(events::now() - started);
    write_new(&root.join("native.json"), &result)?;
    Ok(result)
}

fn validate(request: &Request) -> io::Result<(PathBuf, PathBuf, Option<PathBuf>, Vec<String>)> {
    if !(1..=604800).contains(&request.timeout)
        || !(1024..=512 * 1024 * 1024).contains(&request.output_limit)
        || request.prompt.len() > PROMPT_LIMIT
        || request.prompt.contains('\0')
        || !request.launcher.is_absolute()
        || !request.launcher.is_file()
        || !request
            .launcher
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
        || request
            .cancel_file
            .as_ref()
            .is_some_and(|p| !p.is_absolute())
        || request
            .useful_command_pattern
            .as_ref()
            .is_some_and(|s| s.len() > 4096)
    {
        return Err(invalid());
    }
    let case = isolated(&request.case_root)?;
    let home = isolated(&request.codex_home)?;
    if case.starts_with(&home) || home.starts_with(&case) {
        return Err(invalid());
    }
    let user = match &request.user_home {
        Some(path) => {
            let user = isolated(path)?;
            if case.starts_with(&user)
                || user.starts_with(&case)
                || home.starts_with(&user)
                || user.starts_with(&home)
            {
                return Err(invalid());
            }
            Some(user)
        }
        None => None,
    };
    if let Some(pattern) = &request.useful_command_pattern {
        regex::RegexBuilder::new(pattern)
            .size_limit(1024 * 1024)
            .build()
            .map_err(|_| invalid())?;
    }
    // The declared observation plan belongs to the explicit local route, and
    // client inputs are only meaningful with a declared plan.
    if request.api_observations.is_some() && request.runner.is_none() {
        return Err(invalid());
    }
    if request.api_observations.is_none() && !request.client_inputs.is_empty() {
        return Err(invalid());
    }
    Ok((case, home, user, config_arguments(&request.extra_config)?))
}

pub(crate) fn isolated(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    let path = path.canonicalize()?;
    let temp = std::env::temp_dir().canonicalize()?;
    if path == temp || !path.is_dir() || !path.starts_with(temp) || path.starts_with(repository()) {
        return Err(invalid());
    }
    Ok(path)
}

/// One explicitly selected route. The selection is exclusive: a conflicting
/// or unknown selection is rejected instead of falling back to another route.
enum Route<'a> {
    Default,
    Xai,
    Local(&'a LocalRunner),
}

fn selected_route(request: &Request) -> io::Result<Route<'_>> {
    match (request.profile.as_deref(), request.runner.as_ref()) {
        (Some(_), Some(_)) => Err(invalid()),
        (Some(""), None) | (None, None) => Ok(Route::Default),
        (Some("xai"), None) => {
            let auth = request.xai_auth.as_ref().ok_or_else(invalid)?;
            if !auth.command.is_absolute()
                || !auth.command.is_file()
                || !auth.home.is_absolute()
                || !auth.home.is_dir()
            {
                return Err(invalid());
            }
            Ok(Route::Xai)
        }
        (None, Some(runner)) => {
            validate_local_runner(runner)?;
            Ok(Route::Local(runner))
        }
        _ => Err(invalid()),
    }
}

/// Bounded, control-free declaration text.
fn token(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.chars().count() <= limit && !value.chars().any(char::is_control)
}

/// Documented reasoning levels the installed client advertises. The local
/// serving configuration may support a narrower set; unsupported levels fail
/// visibly during qualification rather than being silently rewritten.
const REASONING_LEVELS: [&str; 8] = [
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

fn validate_local_runner(runner: &LocalRunner) -> io::Result<()> {
    let endpoint = runner.endpoint.as_str();
    let rest = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .ok_or_else(invalid)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty()
        || endpoint.len() > 2048
        || !endpoint.bytes().all(|byte| byte.is_ascii_graphic())
        || authority.contains('@')
        || rest.contains(['?', '#'])
        || !token(&runner.model, 256)
    {
        return Err(invalid());
    }
    for name in MATERIAL_FIELDS {
        if let Some(value) = runner.identity.declared(name)
            && !token(value, 512)
        {
            return Err(invalid());
        }
    }
    if let Some(reasoning) = runner.identity.declared("reasoning")
        && !REASONING_LEVELS.contains(&reasoning)
    {
        return Err(invalid());
    }
    Ok(())
}

/// Writes the explicit local provider configuration into the isolated home.
/// No credentials are written; the route sends no provider authorization.
fn write_local_home(home: &Path, runner: &LocalRunner) -> io::Result<()> {
    let quote = |value: &str| serde_json::to_string(value);
    fs::write(
        home.join("config.toml"),
        format!(
            "model = {}\nmodel_provider = \"local\"\napproval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nweb_search = \"disabled\"\n\n[model_providers.local]\nname = \"Local\"\nbase_url = {}\nwire_api = \"{WIRE_API}\"\n\n[windows]\nsandbox = \"unelevated\"\n\n[features]\nhooks = false\n",
            quote(&runner.model)?,
            quote(&runner.endpoint)?,
        ),
    )
}

fn write_xai_home(home: &Path, auth: &XaiAuth) -> io::Result<()> {
    let command = display_path(&auth.command).replace('\\', "\\\\");
    let token_home = display_path(&auth.home).replace('\\', "\\\\");
    fs::write(
        home.join("config.toml"),
        format!(
            "model = \"{XAI_MODEL}\"\nmodel_provider = \"xai\"\nmodel_reasoning_effort = \"{EFFORT}\"\napproval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nweb_search = \"disabled\"\n\n[model_providers.xai]\nname = \"xAI\"\nbase_url = \"http://127.0.0.1:56122/v1\"\nwire_api = \"responses\"\n\n[model_providers.xai.auth]\ncommand = \"{command}\"\nargs = [\"xai-token\", \"--codex-home\", \"{token_home}\"]\ntimeout_ms = 15000\n\n[windows]\nsandbox = \"unelevated\"\n\n[features]\nhooks = false\n"
        ),
    )
}

fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

fn isolate_user_profile(
    env: &mut BTreeMap<OsString, Option<OsString>>,
    user: &Path,
) -> io::Result<()> {
    fs::create_dir_all(user.join("AppData/Roaming"))?;
    fs::create_dir_all(user.join("AppData/Local/Temp"))?;
    fs::create_dir_all(user.join(".agents/skills"))?;
    let profile = display_path(user);
    env.insert("USERPROFILE".into(), Some(profile.clone().into()));
    env.insert("HOME".into(), Some(profile.clone().into()));
    env.insert("USERNAME".into(), Some("isolated-user".into()));
    env.insert(
        "APPDATA".into(),
        Some(display_path(&user.join("AppData/Roaming")).into()),
    );
    env.insert(
        "LOCALAPPDATA".into(),
        Some(display_path(&user.join("AppData/Local")).into()),
    );
    env.insert(
        "TEMP".into(),
        Some(display_path(&user.join("AppData/Local/Temp")).into()),
    );
    env.insert(
        "TMP".into(),
        Some(display_path(&user.join("AppData/Local/Temp")).into()),
    );
    if profile.len() >= 2 && profile.as_bytes().get(1) == Some(&b':') {
        env.insert("HOMEDRIVE".into(), Some(profile[..2].to_owned().into()));
        env.insert("HOMEPATH".into(), Some(profile[2..].to_owned().into()));
    }
    Ok(())
}

pub(crate) fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

pub(crate) fn config_arguments(config: &BTreeMap<String, Value>) -> io::Result<Vec<String>> {
    let mut args = Vec::new();
    for (key, value) in config {
        if key.is_empty()
            || key.len() > 256
            || key.split('.').any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
            || [
                "model",
                "profile",
                "runner",
                "credential",
                "auth",
                "forced_login",
                "service_tier",
                "openai_base_url",
                "chatgpt_base_url",
                "temperature",
                "top_p",
                "top_k",
                "seed",
                "cache_prompt",
                "cli_auth_credentials_store",
                "oss_provider",
            ]
            .iter()
            .any(|prefix| key.starts_with(prefix))
        {
            return Err(invalid());
        }
        let text = format!("{key}={}", toml_value(value)?);
        toml::from_str::<toml::Table>(&text).map_err(|_| invalid())?;
        args.extend(["-c".into(), text]);
    }
    Ok(args)
}

fn toml_value(value: &Value) -> io::Result<String> {
    Ok(match value {
        Value::Null => return Err(invalid()),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => value.to_string(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(toml_value)
                .collect::<io::Result<Vec<_>>>()?
                .join(",")
        ),
        Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(k, v)| Ok(format!("{}={}", json!(k), toml_value(v)?)))
                .collect::<io::Result<Vec<_>>>()?
                .join(",")
        ),
    })
}

#[cfg(windows)]
fn execute(request: &Request, root: &Path, result: &mut Value) -> io::Result<()> {
    use harness_core::process::{Cancellation, CommandSpec, Deadline, Job, Limits};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    result["failure_phase"] = json!("validation");
    let (case, home, user, mut extra) = validate(request)?;
    let route = selected_route(request)?;
    let model = match &route {
        Route::Default => MODEL.to_owned(),
        Route::Xai => XAI_MODEL.to_owned(),
        Route::Local(runner) => runner.model.clone(),
    };
    let reasoning = match &route {
        Route::Local(runner) => runner.identity.declared("reasoning").map(str::to_owned),
        _ => Some(EFFORT.to_owned()),
    };
    match &route {
        Route::Xai => {
            write_xai_home(&home, request.xai_auth.as_ref().ok_or_else(invalid)?)?;
            extra.splice(
                0..0,
                [
                    "-c".into(),
                    format!("model_provider={}", json!("xai")),
                    "--disable".into(),
                    "hooks".into(),
                ],
            );
        }
        Route::Local(runner) => {
            write_local_home(&home, runner)?;
            extra.splice(
                0..0,
                [
                    "-c".into(),
                    format!("model_provider={}", json!("local")),
                    "--disable".into(),
                    "hooks".into(),
                ],
            );
        }
        Route::Default => {}
    }
    if root.starts_with(&case)
        || root.starts_with(&home)
        || user.as_ref().is_some_and(|user| root.starts_with(user))
    {
        return Err(invalid());
    }
    result["failure_phase"] = json!("preparation");
    let final_path = root.join("final.txt");
    let stdout = root.join("events.jsonl");
    let stderr = root.join("stderr.txt");
    let stdin = root.join("stdin.txt");
    let mut args: Vec<String> = [
        "exec",
        "--strict-config",
        "--skip-git-repo-check",
        "--json",
        "-C",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([display_path(&case), "-m".into(), model.clone()])
    .collect();
    if let Some(reasoning) = &reasoning {
        args.extend([
            "-c".into(),
            format!("model_reasoning_effort={}", json!(reasoning)),
        ]);
    }
    args.extend(extra);
    args.extend([
        "--output-last-message".into(),
        display_path(&final_path),
        "-".into(),
    ]);
    create(&stdin)?.write_all(request.prompt.as_bytes())?;
    let mut spec = CommandSpec::new(&request.launcher);
    spec.args = args.iter().map(OsString::from).collect();
    spec.current_dir = Some(case.clone());
    spec.env
        .insert("CODEX_HOME".into(), Some(display_path(&home).into()));
    if matches!(route, Route::Local(_)) {
        // The explicit local route never inherits ambient provider credentials:
        // an unattended fallback to a cloud endpoint or billing route is refused.
        for name in ["OPENAI_API_KEY", "OPENAI_BASE_URL"] {
            spec.env.insert(name.into(), None);
        }
    }
    if let Some(user) = &user {
        spec.env
            .insert("USERPROFILE".into(), Some(display_path(user).into()));
        spec.env
            .insert("HOME".into(), Some(display_path(user).into()));
        isolate_user_profile(&mut spec.env, user)?;
    }
    spec.stdin = Some(File::open(&stdin)?);
    spec.stdout = Some(create(&stdout)?);
    spec.stderr = Some(create(&stderr)?);
    result["executable_sha256"] = json!(hash_file(&request.launcher)?);
    result["model"] = json!(model);
    result["effort"] = json!(reasoning);
    if let Route::Local(runner) = &route {
        result["runner"] = serde_json::to_value(RunnerRecord::new(runner))?;
        // Verified against the installed client: its Responses request carries
        // model/instructions/input/tools/reasoning and no sampling or prompt
        // cache fields, so a per-request fixed seed, `cache_prompt=false` or
        // deterministic decoding cannot be expressed through this transport.
        // The route records that limitation instead of pretending to control
        // decoding; supplied serving settings stay declared runner identity.
        result["determinism"] = json!({
            "client_overrides": [],
            "effect": "server-side defaults apply",
            "unsupported_request_controls": ["seed", "temperature", "top_p", "cache_prompt"],
            "note": "the route applies no per-request decoding or prompt-cache controls because the installed client transport exposes no such request fields"
        });
    }
    let mut environment = json!({"CODEX_HOME": home});
    if let Some(user) = &user {
        environment["USERPROFILE"] = json!(user);
        environment["HOME"] = json!(user);
    }
    if matches!(route, Route::Local(_)) {
        environment["removedProviderCredentials"] = json!(["OPENAI_API_KEY", "OPENAI_BASE_URL"]);
    }
    write_new(
        &root.join("request.json"),
        &json!({"executable":request.launcher,"arguments":args,
        "workingDirectory":case,"stdoutPath":stdout,"stderrPath":stderr,"stdinPath":stdin,
        "memoryLimitMiB":2048,"timeoutSeconds":request.timeout,"outputLimitBytes":request.output_limit,"environment":environment}),
    )?;
    // Declared API observations are collected before launch: a required
    // observation that cannot be fetched, read, parsed or confirmed suspends
    // the attempt before any model or tool work happens.
    let mut declared_observations = None;
    if let (Route::Local(runner), Some(plan)) = (&route, &request.api_observations) {
        result["failure_phase"] = json!("observations");
        match collect_observations(runner, plan, &request.client_inputs) {
            Ok(observed) => {
                result["observed_api"] = serde_json::to_value(&observed)?;
                declared_observations = Some(observed);
            }
            Err(failure) => {
                result["observation_failure"] = serde_json::to_value(&failure)?;
                return Err(io::Error::other(failure.to_string()));
            }
        }
    }
    let mut telemetry = events::Events::open(
        &stdout,
        &root.join("observed.jsonl"),
        request.useful_command_pattern.as_deref(),
    )?;
    let job = Job::new(Limits {
        memory_bytes: Some(2048 * 1024 * 1024),
        cpu_percent: None,
    })?;
    result["failure_phase"] = json!("launch");
    let suspended = job.spawn_suspended(&spec)?;
    if !job.contains(suspended.process())? {
        return Err(invalid());
    }
    let identity = suspended.process().identity();
    write_new(
        &root.join("started.json"),
        &json!({"processId":identity.pid,"assignedBeforeResume":true}),
    )?;
    let child = suspended.resume()?;
    drop(spec);
    let deadline = Deadline::after(Duration::from_secs(request.timeout))?;
    let cancellation = Cancellation::default();
    let stop = Arc::new(AtomicBool::new(false));
    let limit = Arc::new(AtomicBool::new(false));
    let watcher = {
        let stop = stop.clone();
        let limit = limit.clone();
        let cancellation = cancellation.clone();
        let output_limit = request.output_limit;
        let cancel = request.cancel_file.clone();
        let paths = [stdout.clone(), stderr.clone(), final_path.clone()];
        std::thread::spawn(move || -> io::Result<events::Events> {
            let observed = (|| -> io::Result<()> {
                loop {
                    if process_result::limit_exceeded(output_limit, &paths) {
                        limit.store(true, Ordering::Relaxed);
                        cancellation.cancel();
                    }
                    if cancel.as_ref().is_some_and(|p| p.is_file()) {
                        cancellation.cancel();
                    }
                    let finished = stop.load(Ordering::Relaxed);
                    telemetry.poll(finished)?;
                    if finished {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            })();
            if observed.is_err() {
                cancellation.cancel();
            }
            observed.map(|_| telemetry)
        })
    };
    result["failure_phase"] = json!("observation");
    let outcome = job.wait(&child, deadline, &cancellation, Duration::from_secs(5));
    stop.store(true, Ordering::Relaxed);
    let telemetry = watcher.join().map_err(|_| invalid())?;
    let outcome = outcome?;
    let status = process_result::stop_status(outcome.reason, limit.load(Ordering::Relaxed));
    let receipt = json!({"Status":status,"ExitCode":outcome.exit_code,"ProcessExitCode":outcome.process_exit_code,
        "ProcessId":identity.pid,"AssignedBeforeResume":true,"MemoryLimitBytes":outcome.job.memory_limit_bytes,
        "PeakJobMemoryBytes":outcome.job.peak_job_memory_bytes,"job":outcome.job});
    write_new(&root.join("result.json"), &receipt)?;
    result["process"] = receipt;
    result["status"] = json!(match status {
        "exited" if outcome.exit_code == 0 => "completed",
        "timeout" => "timeout",
        "cancelled" => "incomplete",
        _ => "failed",
    });
    let telemetry = telemetry?;
    for (key, value) in telemetry.value.as_object().expect("owned telemetry") {
        result[key] = value.clone();
    }
    let mut errors = telemetry.errors;
    match observed_counters(&stdout) {
        Ok((tool_operations, rounds)) => {
            result["tool_operations"] = json!(tool_operations);
            result["rounds"] = json!(rounds);
        }
        Err(_) => {
            errors.insert("missing_observation_counters".into());
        }
    }
    if result["status"] == "completed" && result["turn_completed"] != true {
        result["status"] = json!("incomplete");
    }
    if errors.contains("native_error_event") && result["status"] == "completed" {
        result["status"] = json!("failed");
    }
    result["failure_phase"] = json!("usage");
    let paths = rollout_paths(&home, result, &mut errors)?;
    result["rollout_paths"] = json!(paths);
    result["usage"] = usage(&paths);
    let threads = result["usage"]["threads"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let observed: Vec<Value> = threads
        .iter()
        .map(|t| {
            let mut row = serde_json::Map::new();
            for key in ["id", "parent_id", "model", "reasoning", "provider"] {
                row.insert(key.into(), t[key].clone());
            }
            Value::Object(row)
        })
        .collect();
    let expected: std::collections::BTreeSet<String> = result["thread_id"]
        .as_str()
        .into_iter()
        .chain(
            result["children"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .map(str::to_owned)
        .collect();
    let actual: std::collections::BTreeSet<String> = observed
        .iter()
        .filter_map(|row| row["id"].as_str())
        .map(str::to_owned)
        .collect();
    let identities_match = !expected.is_empty() && expected == actual;
    if !identities_match {
        errors.insert("rollout_identity_mismatch".into());
    }
    if !actual.is_subset(&expected) || actual.len() != observed.len() {
        // A filename match cannot attribute another thread's usage to this run.
        write_new(&root.join("rejected-usage.json"), &result["usage"])?;
        result["usage"] = usage(&[]);
    } else if !identities_match && !paths.is_empty() {
        result["usage"]["partial"] = json!(true);
        result["usage"]["status"] = json!("partial");
    }
    let verified = identities_match
        && errors.is_empty()
        && observed.iter().all(|t| {
            let observed_model = t["model"].as_str().unwrap_or("");
            let model_match = observed_model == model
                || observed_model == format!("openai/{model}")
                || observed_model == format!("xai/{model}");
            let reasoning_match = reasoning.as_deref() == t["reasoning"].as_str();
            let provider_match = match &route {
                Route::Default => t["provider"]
                    .as_str()
                    .is_some_and(|seen| seen.eq_ignore_ascii_case("OpenAI")),
                Route::Xai => t["provider"]
                    .as_str()
                    .is_some_and(|seen| seen.eq_ignore_ascii_case("xai")),
                // Arbitrary local model names stay unattributed; the observed
                // attribution must agree with this model name's own mapping.
                Route::Local(_) => {
                    t["provider"].as_str().map(str::to_owned)
                        == harness_core::rollout_reader::model_provider(&model).map(str::to_owned)
                }
            };
            model_match && reasoning_match && provider_match
        });
    if !verified {
        errors.insert("observed_model_policy_unverified".into());
    }
    // Observed model/effort/route-attribution consistency, not endpoint or
    // authentication proof.
    result["observed_model_metadata_verified"] = json!(verified);
    result["observed_threads"] = json!(observed);
    if result["children"].as_array().is_some_and(|a| !a.is_empty()) {
        errors.insert("unexpected_delegation".into());
    }
    if result["thread_id"].is_null() {
        errors.insert("missing_thread".into());
    }
    // Re-observe the declared API identity after the attempt: the collected
    // set is retained for the record, and a changed or unavailable required
    // observation fails this attempt instead of entering a qualified
    // comparison against a configuration it did not actually run under.
    if let (Route::Local(runner), Some(plan), Some(before)) = (
        &route,
        &request.api_observations,
        declared_observations.as_ref(),
    ) {
        result["failure_phase"] = json!("observations");
        let mut observation_evidence_failed = false;
        match collect_observations(runner, plan, &request.client_inputs) {
            Ok(after) => {
                let verified = after.digest()? == before.digest()?;
                if !verified {
                    result["observation_drift"] = json!(observation_changes(before, &after));
                    errors.insert("api_observation_drift".into());
                    observation_evidence_failed = true;
                }
                result["observed_api_after"] = serde_json::to_value(&after)?;
                result["observed_api_verified"] = json!(verified);
            }
            Err(failure) => {
                result["observed_api_verified"] = json!(false);
                result["observation_after_failure"] = serde_json::to_value(&failure)?;
                errors.insert("api_observation_unavailable".into());
                observation_evidence_failed = true;
            }
        }
        if observation_evidence_failed && result["status"] == "completed" {
            result["status"] = json!("failed");
        }
    }
    if !errors.is_empty() {
        result["evidence_errors"] = json!(errors);
    }
    result
        .as_object_mut()
        .expect("owned result")
        .remove("failure_phase");
    Ok(())
}

#[cfg(not(windows))]
fn execute(_: &Request, _: &Path, _: &mut Value) -> io::Result<()> {
    Err(invalid())
}

/// Counts completed interaction rounds and completed tool items from the
/// retained visible event stream. Only complete JSON lines count, so a failed
/// attempt retains what was actually observed and nothing more. Call and
/// operation counts stay distinguishable from model-request counts, which the
/// rollout usage carries separately.
fn observed_counters(events: &Path) -> io::Result<(u64, u64)> {
    let mut rounds = 0_u64;
    let mut tool_operations = 0_u64;
    for line in io::BufReader::new(File::open(events)?).lines() {
        let Ok(event) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        if event["type"] == "turn.completed" {
            rounds = rounds.saturating_add(1);
        }
        if event["type"] == "item.completed"
            && matches!(
                event["item"]["type"].as_str(),
                Some("command_execution" | "file_change" | "mcp_tool_call" | "collab_tool_call")
            )
        {
            tool_operations = tool_operations.saturating_add(1);
        }
    }
    Ok((tool_operations, rounds))
}

fn rollout_paths(
    home: &Path,
    result: &Value,
    errors: &mut std::collections::BTreeSet<String>,
) -> io::Result<Vec<PathBuf>> {
    let mut ids = Vec::new();
    if let Some(id) = result["thread_id"].as_str() {
        ids.push(id.to_owned());
    }
    for id in result["children"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        ids.push(id.to_owned());
    }
    ids.sort();
    ids.dedup();
    ids.retain(|id| id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'));
    let mut found: BTreeMap<String, Vec<PathBuf>> =
        ids.iter().map(|id| (id.clone(), Vec::new())).collect();
    let sessions = home.join("sessions");
    let mut pending = if sessions.is_dir() {
        vec![sessions]
    } else {
        Vec::new()
    };
    let mut entries = 0_usize;
    while let Some(dir) = pending.pop() {
        ordinary(&dir)?;
        for entry in fs::read_dir(&dir)? {
            entries += 1;
            if entries > 100_000 {
                errors.insert("rollout_discovery_limit".into());
                return Ok(Vec::new());
            }
            let entry = entry?;
            let path = entry.path();
            if ordinary(&path).is_err() {
                errors.insert("rollout_link_skipped".into());
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else if let Some(name) = path.file_name().and_then(|s| s.to_str())
                && let Some(stem) = name.strip_suffix(".jsonl")
                && let Some(suffix) = stem.get(stem.len().saturating_sub(36)..)
                && let Some(paths) = found.get_mut(suffix)
            {
                paths.push(path);
            }
        }
    }
    let mut paths = Vec::new();
    for (id, mut matches) in found {
        if matches.len() == 1 {
            paths.push(matches.pop().expect("one match"));
        } else {
            errors.insert(format!("missing_or_ambiguous_rollout:{id}"));
        }
    }
    Ok(paths)
}

fn usage(paths: &[PathBuf]) -> Value {
    let (mut value, _sources) = crate::delegation_usage::summarize(paths);
    if paths.is_empty() {
        for field in [
            "input_tokens",
            "cached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
            "total_tokens",
        ] {
            value["totals"][field] = Value::Null;
        }
    }
    value["status"] = json!(if paths.is_empty() {
        "unknown"
    } else if value["partial"] == true {
        "partial"
    } else {
        "known"
    });
    value
}

pub(crate) fn bounded_read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(bytes)
}
pub(crate) fn create(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}
pub(crate) fn write_new(path: &Path, value: &Value) -> io::Result<()> {
    let mut file = create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.flush()
}
fn invalid() -> io::Error {
    io::Error::other("Invalid native outcome request.")
}
