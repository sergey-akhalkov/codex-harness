//! Model-free skill-evolution helpers.

use harness_core::{orchestration_config, role_context::Role};
use serde_json::json;
use skill_evolution::isolation::{self, Request};
use skill_evolution::{catalogue, delivery, identity, ledger, session, usage};
#[cfg(windows)]
use std::time::Duration;
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use crate::outcome_run::{bounded_read, repository, write_new};

fn invalid() -> io::Error {
    io::Error::other("invalid skills command")
}

const CATALOGUE_TIMEOUT: u64 = 30;
const CATALOGUE_LIMIT: u64 = 16 * 1024;

/// Usage line and detail paragraph per subcommand, in dispatch order.
const COMMANDS: [(&str, &str); 5] = [
    (
        "catalogue",
        "codex-harness skills catalogue [--case DIRECTORY] [--codex-home DIRECTORY] [--upstream PATH] [--role lead|delegated-lead|executor] [--profile ID] [--timeout SECONDS] [--limit BYTES]\nModel-free native skills/list for one project: effective skills with kit revision identity, disablement, conflicts, duplicate links and explicit coverage gaps; raw native evidence stays in a private temporary root and a missing native read reports unavailable rather than a local scan. --role composes the installed build's recorded responsibility role (developer text and skill memberships) onto the native read exactly as a managed session receives it, so the catalogue shows that role's effective set; it refuses when the role inputs are missing instead of reporting a different set. --profile ID adds the native profile layer the managed session selected for that role (the installed profile file or its profiles.<name> table; default adds nothing), so the layers that profile carries reach both the resolved role context and the native read, and the composed role and profile are reported as role_context=role=...;profile=...;revision=...; no model routing is composed. --limit accepts 1024-1048576 bytes (default 16384); rerun with a larger --limit for omitted entries.",
    ),
    (
        "isolate",
        "codex-harness skills isolate --request PATH\nVerifies that oracle/baseline/control files are outside the writable case and not writable. No model calls.",
    ),
    (
        "usage",
        "codex-harness skills usage [--user-home DIRECTORY] [--codex-home DIRECTORY]\nModel-free project-then-global skill report. Does not change the library.",
    ),
    (
        "publish",
        "codex-harness skills publish --request PATH\nCompare-and-swap a staged skill package. Does not run models.",
    ),
    (
        "identity",
        "codex-harness skills identity --path DIRECTORY [--operation NAME] [--journal PATH] [--codex-home DIRECTORY]\nReads the live skill descriptor. Honors [[skills.config]] enabled=false. Observation is required before delivery is complete. Does not mutate the library or claim tokens were refunded.",
    ),
];

/// Strict `--flag VALUE` pass: unknown or repeated flags and missing values are
/// invalid, so each command rejects leftovers by map emptiness.
fn options(args: &[OsString]) -> io::Result<BTreeMap<&str, &OsString>> {
    let mut options = BTreeMap::new();
    let mut flags = args.iter();
    while let Some(flag) = flags.next() {
        let value = flags.next().ok_or_else(invalid)?;
        if options
            .insert(flag.to_str().ok_or_else(invalid)?, value)
            .is_some()
        {
            return Err(invalid());
        }
    }
    Ok(options)
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    let help = |detail: Option<&str>| -> io::Result<i32> {
        match detail {
            Some(detail) => println!("{detail}"),
            None => COMMANDS
                .iter()
                .for_each(|(_, detail)| println!("{}", detail.lines().next().unwrap_or_default())),
        }
        Ok(0)
    };
    let Some((command, rest)) = args.split_first() else {
        return help(None);
    };
    if command == "--help" || command == "-h" {
        return help(None);
    }
    let Some((name, detail)) = COMMANDS.iter().find(|(name, _)| command == *name) else {
        return Err(invalid());
    };
    if rest.iter().any(|arg| arg == "--help" || arg == "-h") {
        return help(Some(detail));
    }
    match *name {
        "catalogue" => catalogue_cmd(rest),
        "isolate" => isolate(rest),
        "usage" => usage_cmd(rest),
        "publish" => publish_cmd(rest),
        _ => identity_cmd(rest),
    }
}

fn catalogue_cmd(args: &[OsString]) -> io::Result<i32> {
    let mut opts = options(args)?;
    let case = match opts.remove("--case") {
        Some(path) => PathBuf::from(path).canonicalize()?,
        None => env::current_dir()?.canonicalize()?,
    };
    let home = match opts.remove("--codex-home") {
        Some(path) => PathBuf::from(path).canonicalize()?,
        None => default_codex_home()?.canonicalize()?,
    };
    let upstream = opts.remove("--upstream").map(PathBuf::from);
    let role = match opts.remove("--role") {
        Some(value) if value == "lead" => Some(Role::Lead),
        Some(value) if value == "delegated-lead" => Some(Role::DelegatedLead),
        Some(value) if value == "executor" => Some(Role::Executor),
        Some(_) => return Err(invalid()),
        None => None,
    };
    let profile = match opts.remove("--profile") {
        Some(value) => Some(value.to_str().ok_or_else(invalid)?.to_owned()),
        None => None,
    };
    let timeout = number(opts.remove("--timeout"))?.unwrap_or(CATALOGUE_TIMEOUT);
    let limit = number(opts.remove("--limit"))?.unwrap_or(CATALOGUE_LIMIT);
    if !opts.is_empty() || !(1..=300).contains(&timeout) || !(1024..=1024 * 1024).contains(&limit) {
        return Err(invalid());
    }
    if profile.is_some() && role.is_none() {
        return Err(io::Error::other(
            "--profile applies one selected native profile layer to the composed role context; pass --role lead|delegated-lead|executor with it",
        ));
    }
    // An explicitly requested role is resolved from the installed build's
    // recorded source before the native read, with the selected profile's
    // layers, so the read sees the same effective context a managed session
    // receives; a missing role or profile input refuses the command instead
    // of reporting a set the managed session would not see.
    let (extra, report) = match role {
        Some(role) => {
            let (args, report) = role_context_args(&home, &case, profile.as_deref(), role)?;
            (args, Some(report))
        }
        None => (Vec::new(), None),
    };
    let private = tempfile::Builder::new()
        .prefix("codex-skills-catalogue-")
        .tempdir()?;
    let root = private.path().canonicalize()?;
    if root.starts_with(repository()) {
        return Err(invalid());
    }
    let print_view = |view: &catalogue::View| -> io::Result<()> {
        let mut out = String::new();
        if let Some(report) = &report {
            out.push_str(report);
            out.push('\n');
        }
        out.push_str(&delivery::render(view, limit as usize)?);
        print!("{out}");
        Ok(())
    };
    match native_catalogue(
        &case,
        &home,
        upstream.as_deref(),
        &extra,
        timeout,
        limit as usize,
        &root,
    ) {
        Ok(view) => {
            print_view(&view)?;
            Ok(0)
        }
        Err(error) => {
            let evidence = private.keep().canonicalize()?;
            let view = catalogue::View::unavailable(error.to_string());
            print_view(&view)?;
            eprintln!(
                "native skill discovery failed; private evidence: {}",
                evidence.display()
            );
            Ok(1)
        }
    }
}

fn number(value: Option<&OsString>) -> io::Result<Option<u64>> {
    value
        .map(|value| {
            value
                .to_str()
                .and_then(|value| value.parse().ok())
                .ok_or_else(invalid)
        })
        .transpose()
}

fn default_codex_home() -> io::Result<PathBuf> {
    if let Some(home) = env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(home));
    }
    let profile = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .ok_or_else(invalid)?;
    Ok(PathBuf::from(profile).join(".codex"))
}

fn registered_upstream(home: &Path) -> io::Result<PathBuf> {
    let path = home.join("harness/native-launch.json");
    let detail = |detail: String| {
        io::Error::other(format!(
            "native launch registration at {} {detail}",
            path.display()
        ))
    };
    let bytes = std::fs::read(&path).map_err(|error| {
        io::Error::other(format!(
            "native launch registration is unreadable at {} ({error})",
            path.display()
        ))
    })?;
    let registration: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| detail(format!("is not JSON ({error})")))?;
    registration["upstream"]["executable"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| detail("records no upstream executable".to_owned()))
}

/// The role composition one explicit `--role` request applies to the native
/// read -- the installed build's resolved developer text and skill
/// memberships under the selected profile layer, rendered exactly as the
/// managed host renders them for its app-server child -- plus the honest
/// one-line report of the role, profile and revision that were composed.
fn role_context_args(
    home: &Path,
    case: &Path,
    profile_id: Option<&str>,
    role: Role,
) -> io::Result<(Vec<String>, String)> {
    let profile = profile_id
        .map(|id| selected_profile(home, id))
        .transpose()
        .map_err(|error| role_refusal(role, profile_id, error))?;
    let launch = harness_core::native_launcher::role_launch(
        home,
        case,
        profile.as_ref(),
        &toml::Table::new(),
        role,
    )
    .map_err(|error| role_refusal(role, profile_id, error))?;
    let report = format!(
        "role_context=role={};profile={};revision={}",
        launch.role.label(),
        profile_id.unwrap_or("none"),
        launch.revision
    );
    Ok((launch.args, report))
}

/// The native profile layer a managed session's selected profile supplies,
/// validated exactly as a dispatch validates its binding so a profile that is
/// not installed refuses instead of silently composing nothing. The catalogue
/// stays model-free and role/model independent: the profile's routing values
/// are never resolved or used, only the context layers the role composition
/// retains from it.
fn selected_profile(home: &Path, id: &str) -> io::Result<toml::Table> {
    if id == "default" {
        return Ok(toml::Table::new());
    }
    orchestration_config::binding(home, id)?;
    orchestration_config::profile_config(home, id)
}

/// The catalogue's refusal for a requested role context it cannot compose.
fn role_refusal(role: Role, profile: Option<&str>, error: io::Error) -> io::Error {
    let profile = profile
        .map(|id| format!(" --profile {id}"))
        .unwrap_or_default();
    io::Error::other(format!(
        "role context for --role {}{profile}: {error}; the catalogue refuses to report a set the managed session would not see",
        role.label()
    ))
}

fn native_catalogue(
    case: &Path,
    home: &Path,
    upstream: Option<&Path>,
    extra: &[String],
    timeout: u64,
    limit: usize,
    root: &Path,
) -> io::Result<catalogue::View> {
    #[cfg(windows)]
    {
        let upstream = match upstream {
            Some(path) => path.to_path_buf(),
            None => registered_upstream(home)?,
        };
        if !upstream.is_absolute()
            || !upstream
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            return Err(io::Error::other(
                "native upstream must be an absolute executable path",
            ));
        }
        let mut report = json!({"status":"incomplete","model_calls":0,"evidence_root":root});
        let response = crate::native_read_rpc::exchange(
            crate::native_read_rpc::Request {
                upstream: &upstream,
                case,
                working_directory: case,
                home,
                extra,
                root,
                timeout: Duration::from_secs(timeout),
                output_limit: 16 * 1024 * 1024,
                protocol: crate::native_read_rpc::Protocol::Outcome,
            },
            &mut report,
        )?;
        catalogue::derive(
            &response["listed"],
            case,
            Some(&home.join("config.toml")),
            limit,
        )
    }
    #[cfg(not(windows))]
    Err(io::Error::other(
        "native skills/list requires the Windows app-server job",
    ))
}

fn isolate(args: &[OsString]) -> io::Result<i32> {
    let mut opts = options(args)?;
    let request = opts.remove("--request").ok_or_else(invalid)?;
    if !opts.is_empty() {
        return Err(invalid());
    }
    let bytes = bounded_read(Path::new(request), 4 * 1024 * 1024).map_err(|_| invalid())?;
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let private = tempfile::Builder::new()
        .prefix("codex-skills-isolate-")
        .tempdir()?;
    let root = private.path().canonicalize()?;
    if root.starts_with(repository()) {
        return Err(invalid());
    }
    let _ = private.keep();
    let mut report = json!({"status":"incomplete","model_calls":0,"evidence_root":root});
    write_new(&root.join("started.json"), &report)?;
    match isolation::isolate(&request) {
        Ok(result) => {
            report["status"] = json!(if result.isolation_verified {
                "passed"
            } else {
                "failed"
            });
            report["isolation"] = json!(result);
        }
        Err(error) => {
            write_new(
                &root.join("failure.json"),
                &json!({"kind":format!("{:?}",error.kind()),"raw_os_error":error.raw_os_error(),"message":error.to_string()}),
            )?;
            report["status"] = json!("failed");
            report["error"] = json!("Skill isolation was not verified; inspect private evidence.");
        }
    }
    write_new(&root.join("isolate.json"), &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(if report["status"] == "passed" { 0 } else { 1 })
}

fn usage_cmd(args: &[OsString]) -> io::Result<i32> {
    let mut opts = options(args)?;
    let user = opts
        .remove("--user-home")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                env::var_os("USERPROFILE")
                    .unwrap_or_else(|| env::var_os("HOME").unwrap_or_default()),
            )
        });
    let home = opts
        .remove("--codex-home")
        .map(PathBuf::from)
        .unwrap_or_else(|| user.join(".codex"));
    if !opts.is_empty() {
        return Err(invalid());
    }
    let report = usage::report(
        &env::current_dir()?,
        &user,
        &ledger::default_path(&home),
        None,
        Some(&home.join("config.toml")),
    )?;
    print!("{}", usage::render(&report));
    Ok(0)
}

fn publish_cmd(args: &[OsString]) -> io::Result<i32> {
    let mut opts = options(args)?;
    let request = opts.remove("--request").ok_or_else(invalid)?;
    if !opts.is_empty() {
        return Err(invalid());
    }
    let request: Publish =
        serde_json::from_slice(&bounded_read(Path::new(request), 4 * 1024 * 1024)?)
            .map_err(|_| invalid())?;
    let identity = skill_evolution::publication::publish(
        &request.staged,
        &request.dest,
        &request.expected_parent,
    )
    .map_err(|_| invalid())?;
    let awareness = session::after_publish(identity::Revision {
        name: identity.name.clone(),
        path: request.dest.to_string_lossy().into_owned(),
        revision: identity.revision.clone(),
        operation: "publish".into(),
    });
    if let Some(journal) = request.journal {
        session::save(&journal, &awareness)?;
    }
    let mut report = session::report(&awareness);
    report["files"] = json!(identity.files);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}

#[derive(serde::Deserialize)]
struct Publish {
    staged: PathBuf,
    dest: PathBuf,
    expected_parent: String,
    #[serde(default)]
    journal: Option<PathBuf>,
}

fn identity_cmd(args: &[OsString]) -> io::Result<i32> {
    let mut opts = options(args)?;
    let path = opts
        .remove("--path")
        .map(PathBuf::from)
        .ok_or_else(invalid)?;
    let operation = opts
        .remove("--operation")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "observe".to_owned());
    let journal = opts.remove("--journal").map(PathBuf::from);
    let codex_home = opts.remove("--codex-home").map(PathBuf::from);
    if !opts.is_empty() {
        return Err(invalid());
    }
    let identity = identity::from_package(&path, &operation)?;
    let retired = operation == "retire" || operation == "retirement";
    let enabled = codex_home
        .as_ref()
        .is_none_or(|home| !session::config_disables(&home.join("config.toml"), &path));
    let awareness = match journal {
        Some(journal) => {
            let mut awareness = session::load(&journal)
                .unwrap_or_else(|_| session::after_publish(identity.clone()));
            if retired || !enabled {
                session::observe(&mut awareness, &identity.revision, true);
                awareness.enabled = enabled;
            } else {
                awareness = session::live_read_enabled(identity, false, enabled);
            }
            session::save(&journal, &awareness)?;
            awareness
        }
        None => session::live_read_enabled(identity, retired, enabled),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&session::report(&awareness))?
    );
    Ok(0)
}
