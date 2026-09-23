use harness_core::{
    dependency_discovery::{self, Request},
    dependency_releases,
};
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn invalid() -> io::Error {
    io::Error::other("invalid dependency command options")
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    if args.first().is_some_and(|arg| arg == "recover-npm") {
        if args == ["recover-npm", "--help"] {
            println!(
                "codex-harness dependencies recover-npm --state DIRECTORY [--rollback-committed]\nRecover interrupted shared npm MCP activation journals. Committed installations stay in place by default; interrupted preparations roll back to prior absence. Foreign or damaged journals are reported and preserved without mutation. No package is acquired or shared consumer stopped."
            );
            return Ok(0);
        }
        let mut values = BTreeMap::new();
        let mut rollback = false;
        let mut options = args[1..].iter();
        while let Some(option) = options.next() {
            match option.to_str() {
                Some("--rollback-committed") if !rollback => rollback = true,
                Some("--state") => {
                    if values
                        .insert(option.clone(), options.next().ok_or_else(invalid)?.clone())
                        .is_some()
                    {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
        let state = values.get(&OsString::from("--state")).ok_or_else(invalid)?;
        let report =
            harness_core::dependency_npm_install::recover_all(&PathBuf::from(state), rollback)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(0);
    }
    if args.first().is_some_and(|arg| arg == "stage-python") {
        if args == ["stage-python", "--help"] {
            println!(
                "codex-harness dependencies stage-python --uv FILE --uv-sha256 DIGEST --python FILE --python-sha256 DIGEST --state DIRECTORY\nCreate a new empty offline UV environment in owned native state using explicitly trusted executable hashes. Package installation, full Python runtime verification and activation are separate steps."
            );
            return Ok(0);
        }
        let mut values = BTreeMap::new();
        let mut arguments = args[1..].iter();
        while let Some(option) = arguments.next() {
            if !matches!(
                option.to_str(),
                Some("--uv" | "--uv-sha256" | "--python" | "--python-sha256" | "--state")
            ) || values
                .insert(
                    option.clone(),
                    arguments.next().ok_or_else(invalid)?.clone(),
                )
                .is_some()
            {
                return Err(invalid());
            }
        }
        let value = |key: &str| values.get(&OsString::from(key)).ok_or_else(invalid);
        let uv = PathBuf::from(value("--uv")?);
        let python = PathBuf::from(value("--python")?);
        let state = PathBuf::from(value("--state")?);
        let staged = harness_core::dependency_python_stage::stage_uv_venv(
            harness_core::dependency_python_stage::PythonVenvStageRequest {
                uv_exe: &uv,
                expected_uv_sha256: value("--uv-sha256")?.to_str().ok_or_else(invalid)?,
                python_exe: &python,
                expected_python_sha256: value("--python-sha256")?.to_str().ok_or_else(invalid)?,
                state: &state,
            },
        )?;
        println!("{}", serde_json::to_string_pretty(&staged.report)?);
        return Ok(0);
    }
    if let Some(result) = crate::dependency_apply_cli::run(args) {
        return result;
    }
    if let Some(result) = crate::dependency_selection_cli::run(args) {
        return result;
    }
    if args.first().is_some_and(|arg| arg == "probe") {
        if args == ["probe", "--help"] {
            println!(
                "codex-harness dependencies probe --executable FILE --kind nuphus --sha256 DIGEST\nRun an explicitly verified native artifact in an owned bounded process. Nuphus negotiates and lists tools without desktop/browser calls."
            );
            return Ok(0);
        }
        let mut values = BTreeMap::new();
        let mut arguments = args[1..].iter();
        while let Some(option) = arguments.next() {
            if !matches!(
                option.to_str(),
                Some("--executable" | "--kind" | "--sha256")
            ) || values
                .insert(
                    option.clone(),
                    arguments.next().ok_or_else(invalid)?.clone(),
                )
                .is_some()
            {
                return Err(invalid());
            }
        }
        let value = |key: &str| values.get(&OsString::from(key)).ok_or_else(invalid);
        use harness_core::dependency_mcp_probe::{ProbeKind, probe};
        let kind = match value("--kind")?.to_str() {
            Some("nuphus") => ProbeKind::Nuphus,
            _ => return Err(invalid()),
        };
        let report = probe(
            &PathBuf::from(value("--executable")?),
            kind,
            value("--sha256")?.to_str().ok_or_else(invalid)?,
        )?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(0);
    }
    if args.first().is_some_and(|arg| arg == "stage") {
        if args == ["stage", "--help"] {
            println!(
                "codex-harness dependencies stage --package NAME --version VERSION --state DIRECTORY\nPrepare one official npm package in native owned staging. Runtime validation and activation are separate operations."
            );
            return Ok(0);
        }
        let mut values = BTreeMap::new();
        let mut arguments = args[1..].iter();
        while let Some(option) = arguments.next() {
            if !matches!(option.to_str(), Some("--package" | "--version" | "--state"))
                || values
                    .insert(
                        option.clone(),
                        arguments.next().ok_or_else(invalid)?.clone(),
                    )
                    .is_some()
            {
                return Err(invalid());
            }
        }
        let value = |key: &str| values.get(&OsString::from(key)).ok_or_else(invalid);
        let report = harness_core::dependency_stage::prepare(
            &env::current_exe()?,
            value("--package")?.to_str().ok_or_else(invalid)?,
            value("--version")?.to_str().ok_or_else(invalid)?,
            &PathBuf::from(value("--state")?),
        )?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(0);
    }
    if args.first().is_some_and(|arg| arg == "audit") {
        if args == ["audit", "--help"] {
            println!(
                "codex-harness dependencies audit --package-root DIRECTORY\nExplicitly compare a selected npm tool's files with its exact official release. Reads public metadata and archive in a bounded worker; preserves every installed file. Extra files and runtime compatibility require separate acceptance."
            );
            return Ok(0);
        }
        if args.len() != 3 || args[1] != "--package-root" {
            return Err(invalid());
        }
        let report =
            harness_core::dependency_audit::audit(&env::current_exe()?, &PathBuf::from(&args[2]))?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(0);
    }
    if args == ["--help"] || args == ["discover", "--help"] || args == ["plan", "--help"] {
        println!(
            "codex-harness dependencies stage-python --help (explicit native runtime prerequisites)"
        );
        println!(
            "codex-harness dependencies <validate|select|selected|recover-selection|rollback-selection> --help (retained native candidates)"
        );
        println!(
            "codex-harness dependencies apply|update --source CHECKOUT --user-home DIRECTORY --state DIRECTORY [--preview|--check] [--node FILE --node-sha256 DIGEST]"
        );
        println!(
            "codex-harness dependencies stage --package NAME --version VERSION --state DIRECTORY"
        );
        println!();
        println!(
            "codex-harness dependencies audit --package-root DIRECTORY (explicit official archive comparison)"
        );
        println!(
            "codex-harness dependencies <discover|plan> --source CHECKOUT [--user-home DIRECTORY] [--npm-prefix DIRECTORY ...] [--uv-tools-dir DIRECTORY] [--serena-cache DIRECTORY] [--rustup-home DIRECTORY] [--nuphus-models DIRECTORY] [--full-records] [--probe-versions] [--processes] [--include-process-environment|--no-process-environment]\nDefault discovery executes no packages. --probe-versions requests a bounded native Rust analyzer version read. --processes observes existing host consumers without retaining arguments. Plan explicitly reads official release metadata through system curl and proposes actions requiring further compatibility checks. Neither command installs packages or changes a project."
        );
        return Ok(0);
    }
    let planning = args.first().is_some_and(|arg| arg == "plan");
    if !planning && args.first().is_none_or(|arg| arg != "discover") {
        return Err(invalid());
    }
    let mut values = BTreeMap::new();
    let mut prefixes = Vec::new();
    let mut full = false;
    let mut probes = false;
    let mut processes = false;
    let mut environment = None;
    let mut arguments = args[1..].iter();
    while let Some(option) = arguments.next() {
        match option.to_str() {
            Some("--full-records") if !full => full = true,
            Some("--probe-versions") if !probes => probes = true,
            Some("--processes") if !processes => processes = true,
            Some("--include-process-environment" | "--no-process-environment")
                if environment.is_none() =>
            {
                environment = Some(option == "--include-process-environment")
            }
            Some("--npm-prefix") => {
                prefixes.push(PathBuf::from(arguments.next().ok_or_else(invalid)?))
            }
            Some(
                "--source" | "--user-home" | "--uv-tools-dir" | "--serena-cache" | "--rustup-home"
                | "--nuphus-models",
            ) => {
                if values
                    .insert(
                        option.clone(),
                        arguments.next().ok_or_else(invalid)?.clone(),
                    )
                    .is_some()
                {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    let path = |name: &str| values.get(&OsString::from(name)).map(PathBuf::from);
    let source = path("--source").ok_or_else(invalid)?;
    let current_home = env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|path| dependency_discovery::local_path(&path))
        .transpose()?;
    let home = dependency_discovery::local_path(
        &path("--user-home")
            .or_else(|| current_home.clone())
            .ok_or_else(invalid)?,
    )?;
    let include = environment.unwrap_or_else(|| {
        current_home
            .as_ref()
            .is_some_and(|current| current.as_os_str().eq_ignore_ascii_case(home.as_os_str()))
    });
    let selected = |option: &str, variable: &str| {
        path(option).or_else(|| {
            if include {
                env::var_os(variable).map(PathBuf::from)
            } else {
                None
            }
        })
    };
    if include && let Some(prefix) = env::var_os("NPM_CONFIG_PREFIX") {
        prefixes.insert(0, prefix.into());
    }
    let request = Request {
        catalogue: source.join("global/code-tools.json"),
        user_home: home,
        npm_prefixes: prefixes,
        uv_tools_dir: selected("--uv-tools-dir", "UV_TOOL_DIR"),
        serena_cache: path("--serena-cache"),
        rustup_home: selected("--rustup-home", "RUSTUP_HOME"),
        path: if include { env::var_os("PATH") } else { None },
        nuphus_models: selected("--nuphus-models", "NUPHUS_MODELS_DIR"),
        full_records: full,
        probe_versions: probes,
        processes,
    };
    let mut report = if planning {
        dependency_releases::plan(&request)?
    } else {
        dependency_discovery::discover(&request)?
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid())?;
    let timestamp = chrono::DateTime::from_timestamp(
        now.as_secs().try_into().map_err(|_| invalid())?,
        now.subsec_nanos(),
    )
    .ok_or_else(invalid)?;
    report["observed_at"] =
        serde_json::json!(timestamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true));
    if planning && let Some(items) = report["items"].as_array_mut() {
        for item in items {
            item["release"]["checked_at"] =
                serde_json::json!(timestamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true));
        }
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}
