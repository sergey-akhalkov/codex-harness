//! Model-free skill-evolution helpers.

use serde_json::json;
use skill_evolution::isolation::{self, Request};
use skill_evolution::{catalogue, delivery, identity, ledger, session, usage};
#[cfg(windows)]
use std::time::Duration;
use std::{
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

pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!(
            "codex-harness skills catalogue [--case DIRECTORY] [--codex-home DIRECTORY] [--upstream PATH] [--timeout SECONDS] [--limit BYTES]"
        );
        println!("codex-harness skills isolate --request PATH");
        println!("codex-harness skills usage [--user-home DIRECTORY] [--codex-home DIRECTORY]");
        println!("codex-harness skills publish --request PATH");
        println!(
            "codex-harness skills identity --path DIRECTORY [--operation NAME] [--journal PATH] [--codex-home DIRECTORY]"
        );
        return Ok(0);
    }
    if args[0] == "catalogue" {
        return catalogue_cmd(&args[1..]);
    }
    if args[0] == "isolate" {
        return isolate(&args[1..]);
    }
    if args[0] == "usage" {
        return usage_cmd(&args[1..]);
    }
    if args[0] == "publish" {
        return publish_cmd(&args[1..]);
    }
    if args[0] == "identity" {
        return identity_cmd(&args[1..]);
    }
    Err(invalid())
}

fn catalogue_cmd(args: &[OsString]) -> io::Result<i32> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "codex-harness skills catalogue [--case DIRECTORY] [--codex-home DIRECTORY] [--upstream PATH] [--timeout SECONDS] [--limit BYTES]\nModel-free native skills/list for one project: effective skills with kit revision identity, disablement, conflicts, duplicate links and explicit coverage gaps. Raw native evidence stays in a private temporary root. A missing native read is reported as unavailable; it never falls back to a complete-looking local scan. --limit accepts 1024-1048576 bytes (default 16384); entries beyond it stay explicit and are retrieved by rerunning with a larger --limit."
        );
        return Ok(0);
    }
    let mut case = None;
    let mut codex_home = None;
    let mut upstream = None;
    let mut timeout = CATALOGUE_TIMEOUT;
    let mut limit = CATALOGUE_LIMIT;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.to_str() {
            Some("--case") if case.is_none() => {
                case = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
            }
            Some("--codex-home") if codex_home.is_none() => {
                codex_home = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
            }
            Some("--upstream") if upstream.is_none() => {
                upstream = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
            }
            Some("--timeout") => timeout = number(iter.next())?,
            Some("--limit") => limit = number(iter.next())?,
            _ => return Err(invalid()),
        }
    }
    if !(1..=300).contains(&timeout) || !(1024..=1024 * 1024).contains(&limit) {
        return Err(invalid());
    }
    let case = case.unwrap_or(env::current_dir()?).canonicalize()?;
    let home = codex_home.unwrap_or(default_codex_home()?).canonicalize()?;
    let private = tempfile::Builder::new()
        .prefix("codex-skills-catalogue-")
        .tempdir()?;
    let root = private.path().canonicalize()?;
    if root.starts_with(repository()) {
        return Err(invalid());
    }
    match native_catalogue(
        &case,
        &home,
        upstream.as_deref(),
        timeout,
        limit as usize,
        &root,
    ) {
        Ok(view) => {
            print!("{}", delivery::render(&view, limit as usize)?);
            Ok(0)
        }
        Err(error) => {
            let evidence = private.keep().canonicalize()?;
            let view = catalogue::View::unavailable(error.to_string());
            print!("{}", delivery::render(&view, limit as usize)?);
            eprintln!(
                "native skill discovery failed; private evidence: {}",
                evidence.display()
            );
            Ok(1)
        }
    }
}

fn number(value: Option<&OsString>) -> io::Result<u64> {
    value
        .and_then(|value| value.to_str())
        .and_then(|value| value.parse().ok())
        .ok_or_else(invalid)
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
    let bytes = std::fs::read(&path).map_err(|error| {
        io::Error::other(format!(
            "native launch registration is unreadable at {} ({error})",
            path.display()
        ))
    })?;
    let registration: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::other(format!(
            "native launch registration at {} is not JSON ({error})",
            path.display()
        ))
    })?;
    registration["upstream"]["executable"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::other(format!(
                "native launch registration at {} records no upstream executable",
                path.display()
            ))
        })
}

#[cfg(windows)]
fn native_catalogue(
    case: &Path,
    home: &Path,
    upstream: Option<&Path>,
    timeout: u64,
    limit: usize,
    root: &Path,
) -> io::Result<catalogue::View> {
    let upstream = match upstream {
        Some(path) => path.to_path_buf(),
        None => registered_upstream(home)?,
    };
    if !upstream.is_absolute()
        || !upstream
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
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
            extra: &[],
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
fn native_catalogue(
    _: &Path,
    _: &Path,
    _: Option<&Path>,
    _: u64,
    _: usize,
    _: &Path,
) -> io::Result<catalogue::View> {
    Err(io::Error::other(
        "native skills/list requires the Windows app-server job",
    ))
}

fn isolate(args: &[OsString]) -> io::Result<i32> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "codex-harness skills isolate --request PATH\nVerifies that oracle/baseline/control files are outside the writable case and not writable. No model calls."
        );
        return Ok(0);
    }
    if args.len() != 2 || args[0] != "--request" {
        return Err(invalid());
    }
    let bytes = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024).map_err(|_| invalid())?;
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
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "codex-harness skills usage [--user-home DIRECTORY] [--codex-home DIRECTORY]\nModel-free project-then-global skill report. Does not change the library."
        );
        return Ok(0);
    }
    let mut user_home = None;
    let mut codex_home = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--user-home" && user_home.is_none() {
            user_home = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
        } else if arg == "--codex-home" && codex_home.is_none() {
            codex_home = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
        } else {
            return Err(invalid());
        }
    }
    let user = user_home.unwrap_or_else(|| {
        PathBuf::from(
            env::var_os("USERPROFILE").unwrap_or_else(|| env::var_os("HOME").unwrap_or_default()),
        )
    });
    let home = codex_home.unwrap_or_else(|| user.join(".codex"));
    let cwd = env::current_dir()?;
    let config = home.join("config.toml");
    let report = usage::report(
        &cwd,
        &user,
        &ledger::default_path(&home),
        None,
        Some(&config),
    )?;
    print!("{}", usage::render(&report));
    Ok(0)
}

fn publish_cmd(args: &[OsString]) -> io::Result<i32> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "codex-harness skills publish --request PATH\nCompare-and-swap a staged skill package. Does not run models."
        );
        return Ok(0);
    }
    if args.len() != 2 || args[0] != "--request" {
        return Err(invalid());
    }
    #[derive(serde::Deserialize)]
    struct Publish {
        staged: PathBuf,
        dest: PathBuf,
        expected_parent: String,
        #[serde(default)]
        journal: Option<PathBuf>,
    }
    let request: Publish =
        serde_json::from_slice(&bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?)
            .map_err(|_| invalid())?;
    match skill_evolution::publication::publish(
        &request.staged,
        &request.dest,
        &request.expected_parent,
    ) {
        Ok(identity) => {
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
        Err(_) => Err(invalid()),
    }
}

fn identity_cmd(args: &[OsString]) -> io::Result<i32> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "codex-harness skills identity --path DIRECTORY [--operation NAME] [--journal PATH] [--codex-home DIRECTORY]\nReads the live skill descriptor. Honors [[skills.config]] enabled=false. Observation is required before delivery is complete. Does not mutate the library or claim tokens were refunded."
        );
        return Ok(0);
    }
    let mut path = None;
    let mut operation = "observe".to_owned();
    let mut journal = None;
    let mut codex_home = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--path" && path.is_none() {
            path = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
        } else if arg == "--operation" {
            operation = iter
                .next()
                .ok_or_else(invalid)?
                .to_string_lossy()
                .into_owned();
        } else if arg == "--journal" && journal.is_none() {
            journal = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
        } else if arg == "--codex-home" && codex_home.is_none() {
            codex_home = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
        } else {
            return Err(invalid());
        }
    }
    let path = path.ok_or_else(invalid)?;
    let identity = identity::from_package(&path, &operation)?;
    let retired = operation == "retire" || operation == "retirement";
    let enabled = match &codex_home {
        Some(home) => !session::config_disables(&home.join("config.toml"), &path),
        None => true,
    };
    let awareness = if let Some(journal_path) = journal {
        let mut awareness = session::load(&journal_path)
            .unwrap_or_else(|_| session::after_publish(identity.clone()));
        if retired || !enabled {
            session::observe(&mut awareness, &identity.revision, true);
            awareness.enabled = enabled;
        } else {
            awareness = session::live_read_enabled(identity, false, enabled);
        }
        session::save(&journal_path, &awareness)?;
        awareness
    } else {
        session::live_read_enabled(identity, retired, enabled)
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&session::report(&awareness))?
    );
    Ok(0)
}
