use harness_core::{build_identity, build_selection, native_build};
use std::{collections::BTreeMap, env, ffi::OsString, io, path::PathBuf};

mod delegation_usage;
#[cfg(windows)]
mod outcome_arm;
mod outcome_discovery;
mod outcome_report_cli;
mod outcome_run;

fn run() -> io::Result<i32> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
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
    if args[0] == "outcome-report" {
        return outcome_report_cli::run(&args[1..]);
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
        let _lock = harness_core::installation_lock::InstallationLock::acquire(user)?;
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
        // Cargo bootstrap has no adjacent receipt. Installed managers require
        // verified integrity even when source freshness differs.
        let executable = env::current_exe()?;
        if let Some(parent) = executable
            .parent()
            .filter(|p| p.join("build.json").exists())
        {
            let own = build_identity::check(parent, None);
            if !own.management_allowed {
                return Err(io::Error::other(own.action));
            }
        }
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
