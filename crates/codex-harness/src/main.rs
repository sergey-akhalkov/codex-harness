use harness_core::{build_identity, build_selection, native_build};
use std::{collections::BTreeMap, env, ffi::OsString, io, path::PathBuf};

mod delegation_usage;
#[cfg(windows)]
mod dependency_cli;
#[cfg(windows)]
mod dependency_selection_cli;
#[cfg(windows)]
mod install_cli;
#[cfg(windows)]
mod mcp_cli;
#[cfg(windows)]
mod native_read_rpc;
#[cfg(windows)]
mod outcome_arm;
mod outcome_case_fixture;
mod outcome_discovery;
#[cfg(windows)]
mod outcome_oracle;
mod outcome_prepare;
mod outcome_report_cli;
mod outcome_run;
#[cfg(windows)]
mod source_diagnostics;
#[cfg(windows)]
mod source_diagnostics_view;

fn verify_manager() -> io::Result<()> {
    verify_entrypoint(false)
}

fn verify_runtime() -> io::Result<()> {
    verify_entrypoint(true)
}

fn verify_entrypoint(require_runtime: bool) -> io::Result<()> {
    // Cargo bootstrap has no adjacent receipt. Resolve installed command links
    // before finding the immutable build's identity record.
    let executable = env::current_exe()?.canonicalize()?;
    if let Some(parent) = executable.parent()
        && parent.join("build.json").try_exists()?
    {
        let own = build_identity::check(parent, None);
        if !own.management_allowed || (require_runtime && !own.runtime_allowed) {
            return Err(io::Error::other(own.action));
        }
        if parent.join("codex-harness.exe").canonicalize()? != executable {
            return Err(io::Error::other(
                "this manager is not the recorded native executable",
            ));
        }
    }
    Ok(())
}

fn run() -> io::Result<i32> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    #[cfg(windows)]
    if args
        .first()
        .is_some_and(|a| a == "config-overrides" || a == "config-localize")
    {
        verify_runtime()?;
        if args.len() != 5 || args[1] != "--source" || args[3] != "--codex-home" {
            return Err(io::Error::other(
                "Expected config-overrides|config-localize --source CHECKOUT --codex-home DIRECTORY",
            ));
        }
        let shared = PathBuf::from(&args[2]).join("global/harness.config.toml");
        let home = PathBuf::from(&args[4]);
        if args[0] == "config-localize" {
            let conflicts = harness_core::profile_state::migrate(&shared, &home)?;
            if conflicts > 0 {
                eprintln!(
                    "{conflicts} configuration conflicts: existing local values retained; originals preserved in CODEX_HOME/harness/private-profile-migration."
                );
            }
        } else {
            let values = harness_core::portable_config::overrides(&shared, &home)?;
            println!(
                "{}",
                serde_json::to_string(
                    &values
                        .iter()
                        .map(|v| v.to_string_lossy())
                        .collect::<Vec<_>>()
                )?
            );
        }
        return Ok(0);
    }
    #[cfg(windows)]
    if args == [harness_core::process_service::CREATE_ARGUMENT] {
        verify_runtime()?;
        harness_core::process_service::create_helper_entry()
    }
    #[cfg(windows)]
    if args
        .first()
        .is_some_and(|arg| arg == harness_core::process_service::RUN_ARGUMENT)
    {
        // WMI does not inherit the first client's Job. Contain the service
        // before runtime verification, configuration reads or provider work.
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native service arguments",
            )
        };
        let until = args
            .get(1)
            .and_then(|arg| arg.to_str())
            .and_then(|arg| arg.parse().ok())
            .ok_or_else(invalid)?;
        let account = args
            .get(2)
            .and_then(|arg| arg.to_str())
            .ok_or_else(invalid)?;
        let guard = harness_core::process_service::ServiceGuard::enter(
            until,
            account,
            harness_core::process::Limits {
                memory_bytes: Some(2048 * 1024 * 1024),
                cpu_percent: Some(25.0),
            },
        )?;
        verify_runtime()?;
        if args.len() != 6 {
            return Err(invalid());
        }
        let expected = args[4].to_str().ok_or_else(invalid)?;
        let encoded = args[5].to_str().ok_or_else(invalid)?;
        match args[3].to_str() {
            Some("codebase-memory") => harness_core::cbm_broker::serve(
                guard,
                expected,
                serde_json::from_str(encoded).map_err(|_| invalid())?,
            )?,
            Some("codegraph") => harness_core::codegraph_broker::serve(
                guard,
                expected,
                serde_json::from_str(encoded).map_err(|_| invalid())?,
            )?,
            _ => return Err(invalid()),
        }
        return Ok(0);
    }
    #[cfg(windows)]
    if args.len() == 2 && args[0] == "dependency-stage-worker-v1" {
        verify_manager()?;
        let report = harness_core::dependency_stage::worker(&PathBuf::from(&args[1]))?;
        println!("{}", serde_json::to_string(&report)?);
        return Ok(0);
    }
    #[cfg(windows)]
    if args.len() == 2 && args[0] == "dependency-audit-worker-v1" {
        verify_manager()?;
        let report = harness_core::dependency_audit::worker(&PathBuf::from(&args[1]))?;
        println!("{}", serde_json::to_string(&report)?);
        return Ok(0);
    }
    #[cfg(windows)]
    if env::args_os().next().is_some_and(|arg| {
        PathBuf::from(arg)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("codex-harness-check.exe"))
    }) {
        verify_manager()?;
        return source_diagnostics::run(&args);
    }
    if args.is_empty() || args[0] == "--help" {
        println!("codex-harness mcp codebase-memory --help (explicit native stdio connection)");
        println!(
            "codex-harness diagnose [--project DIRECTORY] [--codex-home DIRECTORY] [--source CHECKOUT]"
        );
        println!("codex-harness check --diagnose [DIAGNOSE_OPTIONS]");
        println!(
            "codex-harness dependencies <discover|plan> --source CHECKOUT [--user-home DIRECTORY]"
        );
        println!("codex-harness dependencies audit --package-root DIRECTORY");
        println!(
            "codex-harness dependencies probe --executable FILE --kind codebase-memory|nuphus --sha256 DIGEST"
        );
        println!(
            "codex-harness dependencies stage --package NAME --version VERSION --state DIRECTORY"
        );
        println!(
            "codex-harness check --core-only --codex-home DIRECTORY --user-home DIRECTORY [--dependency-user-home DIRECTORY] [--timeout-seconds SECONDS]"
        );
        println!(
            "codex-harness disconnect --core-only --codex-home DIRECTORY --user-home DIRECTORY [--dependency-user-home DIRECTORY] [--preview]"
        );
        println!(
            "codex-harness recover --core-only [--preview] --codex-home DIRECTORY --user-home DIRECTORY [--dependency-user-home DIRECTORY]"
        );
        println!(
            "codex-harness install|update --core-only --source CHECKOUT --build DIRECTORY --codex-home DIRECTORY --user-home DIRECTORY [--upstream EXECUTABLE_OR_PACKAGE] [--dependency-user-home DIRECTORY] [--path-scope User|Process] [--preview]"
        );
        println!("codex-harness outcome-prepare --case CASE [--observer ABSOLUTE_EXE]");
        println!("codex-harness outcome-oracle --request PATH");
        println!("codex-harness outcome-arm --request PATH");
        println!("codex-harness outcome-discover --request PATH");
        println!("codex-harness outcome-run --request PATH --run-model-probes");
        println!(
            "codex-harness build --source CHECKOUT --state DIRECTORY [--cargo EXECUTABLE]\ncodex-harness check --build DIRECTORY [--source CHECKOUT]\ncodex-harness activate-build --state DIRECTORY --build DIRECTORY\ncodex-harness recover-build --state DIRECTORY\ncodex-harness inventory --source CHECKOUT --codex-home DIRECTORY --user-home DIRECTORY\ncodex-harness inspect-installation --codex-home DIRECTORY --user-home DIRECTORY [--dependency-user-home DIRECTORY]\ncodex-harness outcome-report --input PATH [--markdown]\ncodex-harness delegation-usage [ROLLOUT ...] [--output PATH] [--format json|markdown] [--private-sources PATH]\nNative migration candidate: immutable builds, selection, identity and core data inventory. Global lifecycle remains on its existing installation."
        );
        return Ok(0);
    }
    if args[0] == "--version" {
        println!("codex-harness {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    #[cfg(windows)]
    if args[0] == "mcp" {
        if args.get(1).is_some_and(|arg| {
            matches!(
                arg.to_str(),
                Some("broker-retire" | "retire-codegraph" | "prepare-codegraph")
            )
        }) {
            verify_manager()?;
        } else {
            verify_runtime()?;
        }
        return mcp_cli::run(&args[1..]);
    }
    #[cfg(windows)]
    if args[0] == "dependencies" {
        verify_manager()?;
        return dependency_cli::run(&args[1..]);
    }
    #[cfg(windows)]
    if args[0] == "diagnose" {
        verify_manager()?;
        return source_diagnostics::run(&args[1..]);
    }
    #[cfg(windows)]
    if args[0] == "check"
        && let Some(index) = args[1..].iter().position(|arg| arg == "--diagnose")
    {
        verify_manager()?;
        let mut options = args[1..].to_vec();
        options.remove(index);
        return source_diagnostics::run(&options);
    }
    #[cfg(windows)]
    if args[0] == "check" && args[1..].iter().any(|arg| arg == "--core-only") {
        verify_manager()?;
        return install_cli::check(&args[1..]);
    }
    #[cfg(windows)]
    if args[0] == "disconnect" {
        verify_manager()?;
        return install_cli::disconnect(&args[1..]);
    }
    #[cfg(windows)]
    if args[0] == "install" || args[0] == "update" {
        verify_manager()?;
        return install_cli::run(&args[1..]);
    }
    #[cfg(windows)]
    if args[0] == "recover" {
        verify_manager()?;
        return install_cli::recover(&args[1..]);
    }
    if args[0] == "outcome-report" {
        return outcome_report_cli::run(&args[1..]);
    }
    if args[0] == "outcome-prepare" {
        return outcome_prepare::run(&args[1..]);
    }
    #[cfg(windows)]
    if args[0] == "outcome-oracle" {
        return outcome_oracle::run(&args[1..]);
    }
    if args[0] == "--outcome-case" {
        outcome_case_fixture::run()?;
        return Ok(0);
    }
    if args[0] == "outcome-run" {
        return outcome_run::run(&args[1..]);
    }
    if args[0] == "outcome-discover" {
        return outcome_discovery::run(&args[1..]);
    }
    #[cfg(windows)]
    if args[0] == "outcome-arm" {
        return outcome_arm::run(&args[1..]);
    }
    if args[0] == "delegation-usage" {
        return delegation_usage::run(&args[1..]);
    }
    if args[0] == "inventory" {
        let mut options = BTreeMap::new();
        let mut iter = args[1..].iter();
        while let Some(key) = iter.next() {
            if !["--source", "--codex-home", "--user-home"]
                .iter()
                .any(|value| key == value)
            {
                return Err(io::Error::other("unknown inventory option"));
            }
            let value = iter
                .next()
                .ok_or_else(|| io::Error::other("missing inventory option value"))?;
            if options.insert(key.clone(), PathBuf::from(value)).is_some() {
                return Err(io::Error::other("duplicate inventory option"));
            }
        }
        let required = |name: &str| {
            options
                .get(&OsString::from(name))
                .ok_or_else(|| io::Error::other(format!("{name} is required")))
        };
        let _installation_lock =
            harness_core::installation_lock::InstallationLock::acquire(required("--user-home")?)?;
        let report = harness_core::inventory::read(
            required("--source")?,
            required("--codex-home")?,
            required("--user-home")?,
        )?;
        #[cfg(windows)]
        harness_core::agent_config::check(required("--codex-home")?, &report.agents)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(0);
    }
    #[cfg(windows)]
    if args[0] == "inspect-installation" {
        let mut options = BTreeMap::new();
        let mut iter = args[1..].iter();
        while let Some(key) = iter.next() {
            if !["--codex-home", "--user-home", "--dependency-user-home"]
                .iter()
                .any(|v| key == v)
            {
                return Err(io::Error::other("unknown installation inspection option"));
            }
            let value = iter
                .next()
                .ok_or_else(|| io::Error::other("missing inspection option value"))?;
            if options.insert(key.clone(), PathBuf::from(value)).is_some() {
                return Err(io::Error::other("duplicate installation inspection option"));
            }
        }
        let required = |name: &str| {
            options
                .get(&OsString::from(name))
                .ok_or_else(|| io::Error::other(format!("{name} is required")))
        };
        let user = required("--user-home")?;
        let codex_home = required("--codex-home")?;
        let dependency_user = options
            .get(&OsString::from("--dependency-user-home"))
            .unwrap_or(user);
        let _locks =
            harness_core::installation_lock::InstallationLocks::acquire(user, dependency_user)?;
        let installation = harness_core::installation_state::LegacyInstallation::read(
            codex_home,
            user,
            dependency_user,
        )?;
        if let Some(state) = &installation {
            state.verify_unchanged()?;
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&installation.as_ref().map(|s| s.summary()))?
        );
        return Ok(0);
    }
    if args[0] == "finalize-build-v1" {
        if args.len() != 2 {
            return Err(io::Error::other(
                "finalize-build-v1 requires one request path",
            ));
        }
        native_build::finalize(&PathBuf::from(&args[1]))?;
        return Ok(0);
    }
    let building = args[0] == "build";
    let activating = args[0] == "activate-build";
    let recovering = args[0] == "recover-build";
    if !building && !activating && !recovering && args[0] != "check" {
        return Err(io::Error::other("unsupported command; use --help"));
    }
    let mut options = BTreeMap::new();
    let mut iter = args[1..].iter();
    while let Some(arg) = iter.next() {
        let valid = if building {
            ["--source", "--state", "--cargo"]
                .iter()
                .any(|value| arg == value)
        } else if activating {
            ["--state", "--build"].iter().any(|value| arg == value)
        } else if recovering {
            arg == "--state"
        } else {
            ["--source", "--build"].iter().any(|value| arg == value)
        };
        if !valid {
            return Err(io::Error::other("unknown option"));
        }
        let value = iter
            .next()
            .ok_or_else(|| io::Error::other("missing option value"))?;
        if options.insert(arg.clone(), value.clone()).is_some() {
            return Err(io::Error::other("duplicate option"));
        }
    }
    let required = |name: &str| {
        options
            .get(&OsString::from(name))
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other(format!("{name} is required")))
    };
    let source = options.get(&OsString::from("--source")).map(PathBuf::from);
    if building || activating || recovering {
        verify_manager()?;
    }
    if activating || recovering {
        let state = required("--state")?;
        let selection = if activating {
            native_build::activate_candidate(&state, &required("--build")?)?
        } else {
            build_selection::recover(&state)?
        };
        println!("{}", serde_json::to_string_pretty(&selection)?);
        return Ok(0);
    }
    if building {
        let source = required("--source")?;
        let state = required("--state")?;
        let cargo = options
            .get(&OsString::from("--cargo"))
            .cloned()
            .unwrap_or_else(|| "cargo".into());
        let prepared = native_build::prepare(&source, &state, &cargo)?;
        println!("{}", serde_json::to_string_pretty(&prepared)?);
        return Ok(0);
    }
    let build = required("--build")?;
    let report = build_identity::check(&build, source.as_deref());
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(if report.runtime_allowed { 0 } else { 1 })
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("codex-harness: {e}");
            std::process::exit(2);
        }
    }
}
