//! Explicit stdio endpoints; no global registration or implicit provisioning.
use harness_core::{
    cbm_stdio,
    dependency_discovery::local_path,
    mcp_stdio,
    process::{Cancellation, Deadline},
};
use std::{collections::BTreeMap, ffi::OsString, io, path::Path, time::Duration};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid native MCP command options",
    )
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    if args == ["retire-codegraph"] {
        let result = match harness_core::codegraph_account::existing_root()? {
            None => serde_json::json!({"status":"absent"}),
            Some(path) => {
                let root = harness_core::broker_state::BrokerRoot::open(&path)?;
                let result = harness_core::broker_launch::retire(
                    &root,
                    Deadline::after(Duration::from_secs(10))?,
                    &Cancellation::default(),
                )?;
                if matches!(
                    result,
                    harness_core::broker_launch::Retirement::Pending { .. }
                ) {
                    return Err(io::Error::other(
                        "CodeGraph service retirement remains pending",
                    ));
                }
                serde_json::json!({"status":"retired","retirement":result})
            }
        };
        println!("{result}");
        return Ok(0);
    }
    if args.first().is_some_and(|arg| arg == "prepare-codegraph") {
        return prepare_codegraph(&args[1..]);
    }
    if args
        .first()
        .is_some_and(|arg| arg == "apply-codegraph-registration")
    {
        return apply_codegraph_registration(&args[1..]);
    }
    if args.first().is_some_and(|arg| arg == "codegraph") {
        return codegraph(&args[1..]);
    }
    if args == ["broker-prepare"] {
        let root = harness_core::broker_state::BrokerRoot::prepare()?.keep();
        println!("{}", serde_json::json!({"broker_root":root.path()}));
        return Ok(0);
    }
    if args.first().is_some_and(|arg| arg == "broker-retire") {
        if args.len() != 3 || args[1] != "--root" {
            return Err(invalid());
        }
        let path = local_path(Path::new(&args[2]))?;
        let root = harness_core::broker_state::BrokerRoot::open(&path)?;
        let result = harness_core::broker_launch::retire(
            &root,
            Deadline::after(Duration::from_secs(8))?,
            &Cancellation::default(),
        )?;
        println!("{}", serde_json::to_string(&result)?);
        return Ok(
            if matches!(
                result,
                harness_core::broker_launch::Retirement::Pending { .. }
            ) {
                2
            } else {
                0
            },
        );
    }
    if args == ["codebase-memory", "--help"] || args == ["--help"] {
        println!(
            "codex-harness mcp broker-prepare\ncodex-harness mcp broker-retire --root DIRECTORY"
        );
        println!(
            "codex-harness mcp codebase-memory --executable FILE --cache DIRECTORY --runtime DIRECTORY --account DIRECTORY --catalogue-file JSON --connection-seconds SECONDS [--broker-root DIRECTORY]\nServe one bounded MCP stdio connection using a saved cbm-catalogue report. Initialize and tools/list stay local. Tool calls may change the selected graph. Infrastructure failure closes the connection. Optional shared mode requires a fresh private root from 'codex-harness mcp broker-prepare'; share that root across clients during one broker lifetime. After its exit, prepare a new root. Global registration and automatic root/log lifecycle are separate."
        );
        return Ok(0);
    }
    if args.first().is_none_or(|value| value != "codebase-memory") {
        return Err(invalid());
    }
    let mut options = BTreeMap::new();
    let mut rest = args[1..].iter();
    while let Some(key) = rest.next() {
        if !matches!(
            key.to_str(),
            Some(
                "--executable"
                    | "--cache"
                    | "--runtime"
                    | "--account"
                    | "--catalogue-file"
                    | "--connection-seconds"
                    | "--broker-root"
            )
        ) {
            return Err(invalid());
        }
        let value = rest.next().ok_or_else(invalid)?;
        if options.insert(key.clone(), value.clone()).is_some() {
            return Err(invalid());
        }
    }
    let path = |name: &str| -> io::Result<_> {
        local_path(Path::new(
            options.get(&OsString::from(name)).ok_or_else(invalid)?,
        ))
    };
    let seconds: u64 = options
        .get(&OsString::from("--connection-seconds"))
        .and_then(|value| value.to_str())
        .and_then(|value| value.parse().ok())
        .filter(|value| (1..=86400).contains(value))
        .ok_or_else(invalid)?;
    let configuration = cbm_stdio::Configuration {
        executable: path("--executable")?,
        cache: path("--cache")?,
        runtime: path("--runtime")?,
        account: path("--account")?,
        catalogue: path("--catalogue-file")?,
    };
    let broker_root = options
        .get(&OsString::from("--broker-root"))
        .map(|root| local_path(Path::new(root)))
        .transpose()?;
    let (input, output) = mcp_stdio::standard_files()?;
    let cancellation = Cancellation::default();
    let deadline = Deadline::after(Duration::from_secs(seconds))?;
    if let Some(root) = broker_root {
        cbm_stdio::serve_shared(configuration, root, input, output, &cancellation, deadline)?;
    } else {
        cbm_stdio::serve(configuration, input, output, &cancellation, deadline)?;
    }
    Ok(0)
}

fn prepare_codegraph(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp prepare-codegraph --mode Install|Update|Check|Recover --codex-home DIRECTORY --dependency-state DIRECTORY [--package-root DIRECTORY]\nPrepare the native provider projection for the existing installer. Check is read-only; Install/Update may explicitly acquire and probe the pinned published package. This command does not write MCP registrations."
        );
        return Ok(0);
    }
    let mut options = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(key) = rest.next() {
        if !matches!(
            key.to_str(),
            Some("--mode" | "--codex-home" | "--dependency-state" | "--package-root")
        ) {
            return Err(invalid());
        }
        let value = rest.next().ok_or_else(invalid)?;
        if options.insert(key.clone(), value.clone()).is_some() {
            return Err(invalid());
        }
    }
    let path = |name: &str| -> io::Result<_> {
        local_path(Path::new(
            options.get(&OsString::from(name)).ok_or_else(invalid)?,
        ))
    };
    let request = harness_core::codegraph_integration::Request {
        codex_home: path("--codex-home")?,
        dependency_state: path("--dependency-state")?,
        package_root: options
            .get(&OsString::from("--package-root"))
            .map(|p| local_path(Path::new(p)))
            .transpose()?,
        mode: options
            .get(&OsString::from("--mode"))
            .and_then(|v| v.to_str())
            .ok_or_else(invalid)?
            .into(),
    };
    println!(
        "{}",
        harness_core::codegraph_integration::prepare(&request, &std::env::current_exe()?)?
    );
    Ok(0)
}

fn apply_codegraph_registration(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp apply-codegraph-registration --mode Install|Update|Check|Disconnect|Recover --codex-home DIRECTORY [--command FILE] [--package-root DIRECTORY] [--retained-registrations-json JSON] [--defer-commit] [--preview]\nJournal owned CodeGraph MCP registration in an explicit Codex home. Check is read-only. Recover restores interrupted activation. Disconnect removes only the owned native block. Optional retained JSON is a planner handoff for selected retained MCP specs; CodeGraph planning and activation remain native. This command does not search for or mutate a default global Codex home."
        );
        return Ok(0);
    }
    let mut options = BTreeMap::new();
    let mut flags = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(key) = rest.next() {
        match key.to_str() {
            Some("--defer-commit" | "--preview") => {
                if flags.insert(key.clone(), true).is_some() {
                    return Err(invalid());
                }
            }
            Some(
                "--mode"
                | "--codex-home"
                | "--command"
                | "--package-root"
                | "--retained-registrations-json",
            ) => {
                let value = rest.next().ok_or_else(invalid)?;
                if options.insert(key.clone(), value.clone()).is_some() {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    let path = |name: &str| -> io::Result<_> {
        local_path(Path::new(
            options.get(&OsString::from(name)).ok_or_else(invalid)?,
        ))
    };
    let retained = match options.get(&OsString::from("--retained-registrations-json")) {
        None => None,
        Some(value) => {
            let text = value.to_str().ok_or_else(invalid)?;
            Some(serde_json::from_str(text).map_err(|_| invalid())?)
        }
    };
    let request = harness_core::codegraph_registration::RegistrationRequest {
        codex_home: path("--codex-home")?,
        mode: options
            .get(&OsString::from("--mode"))
            .and_then(|value| value.to_str())
            .ok_or_else(invalid)?
            .into(),
        command: options
            .get(&OsString::from("--command"))
            .map(|value| local_path(Path::new(value)))
            .transpose()?,
        package_root: options
            .get(&OsString::from("--package-root"))
            .map(|value| local_path(Path::new(value)))
            .transpose()?,
        defer_commit: flags.contains_key(&OsString::from("--defer-commit")),
        preview: flags.contains_key(&OsString::from("--preview")),
        retained_registrations: retained,
    };
    println!("{}", harness_core::codegraph_registration::apply(&request)?);
    Ok(0)
}

fn codegraph(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp codegraph --package-root DIRECTORY [--project DIRECTORY] [--broker-root DIRECTORY] [--connection-seconds SECONDS]\nServe the verified published package through the native shared worker. The exact current directory is the default project. Indexing is deliberate; indexed projects receive bounded watch/catch-up. The default broker location is shared by this Windows account across Codex homes. No package is downloaded on startup."
        );
        return Ok(0);
    }
    let mut options = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(key) = rest.next() {
        if !matches!(
            key.to_str(),
            Some("--package-root" | "--project" | "--broker-root" | "--connection-seconds")
        ) {
            return Err(invalid());
        }
        let value = rest.next().ok_or_else(invalid)?;
        if options.insert(key.clone(), value.clone()).is_some() {
            return Err(invalid());
        }
    }
    let package = options
        .get(&OsString::from("--package-root"))
        .ok_or_else(invalid)?;
    let inspected = harness_core::dependency_discovery::inspect_package(Path::new(package))?;
    let project = options
        .get(&OsString::from("--project"))
        .map(|value| Path::new(value).to_path_buf())
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)?;
    let configuration = harness_core::codegraph_stdio::configuration(
        &inspected.node,
        &inspected.entry,
        &project,
        harness_core::codegraph_generation::ACTIVE_DIR_NAME.into(),
    )?;
    let seconds = match options.get(&OsString::from("--connection-seconds")) {
        Some(value) => value
            .to_str()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| (1..=86400).contains(v))
            .ok_or_else(invalid)?,
        None => 86400,
    };
    let cancel = Cancellation::default();
    let root = match options.get(&OsString::from("--broker-root")) {
        Some(value) => local_path(Path::new(value))?,
        None => harness_core::codegraph_account::root(
            Deadline::after(Duration::from_secs(30))?,
            &cancel,
        )?,
    };
    let (input, output) = mcp_stdio::standard_files()?;
    harness_core::codegraph_stdio::serve_shared(
        configuration,
        root,
        input,
        output,
        &cancel,
        Deadline::after(Duration::from_secs(seconds))?,
    )?;
    Ok(0)
}
