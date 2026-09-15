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
    if args.first().is_some_and(|arg| arg == "codegraph-control") {
        return codegraph_control(&args[1..]);
    }
    if args == ["broker-prepare"] {
        let root = harness_core::broker_state::BrokerRoot::prepare()?.keep();
        println!("{}", serde_json::json!({"broker_root":root.path()}));
        return Ok(0);
    }
    if args.first().is_some_and(|arg| arg == "serena") {
        return serena(&args[1..]);
    }
    if args.first().is_some_and(|arg| arg == "nuphus") {
        return nuphus(&args[1..]);
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
            "codex-harness mcp broker-prepare\ncodex-harness mcp broker-retire --root DIRECTORY\ncodex-harness mcp serena --help"
        );
        println!("codex-harness mcp nuphus --help");
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

fn serena(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp serena --python FILE --entry FILE --registry FILE --codex-home DIRECTORY --serena-home DIRECTORY --source-root DIRECTORY --connection-seconds SECONDS\nServe one Serena stdio connection as a client of the authenticated shared broker for that CODEX_HOME. Forwarded native arguments come from the selected catalogue. Requests are serialized per project worker; memory and onboarding tools stay hidden unless HARNESS_SERENA_UNFILTERED=1."
        );
        return Ok(0);
    }
    let mut options = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(key) = rest.next() {
        if !matches!(
            key.to_str(),
            Some(
                "--python"
                    | "--entry"
                    | "--registry"
                    | "--codex-home"
                    | "--serena-home"
                    | "--source-root"
                    | "--connection-seconds"
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
    let source_root = path("--source-root")?;
    let catalogue_path = source_root.join("global/code-tools.json");
    let catalogue: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&catalogue_path)?).map_err(|_| invalid())?;
    let arguments: Vec<OsString> = catalogue["mcp"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == "serena"))
        .and_then(|item| item["arguments"].as_array())
        .ok_or_else(invalid)?
        .iter()
        .map(|value| value.as_str().ok_or_else(invalid).map(OsString::from))
        .collect::<io::Result<_>>()?;
    let configuration = harness_core::serena_broker::Configuration {
        python: path("--python")?,
        entry: path("--entry")?,
        registry: path("--registry")?,
        codex_home: path("--codex-home")?,
        serena_home: match options.get(&OsString::from("--serena-home")) {
            Some(value) => local_path(Path::new(value))?,
            None => std::env::var_os("USERPROFILE")
                .map(|home| Path::new(&home).join(".serena"))
                .ok_or_else(invalid)?,
        },
        source_root,
    };
    let (input, output) = mcp_stdio::standard_files()?;
    let cancellation = Cancellation::default();
    let deadline = Deadline::after(Duration::from_secs(seconds))?;
    harness_core::serena_stdio::serve(
        configuration,
        arguments,
        input,
        output,
        &cancellation,
        deadline,
    )?;
    Ok(0)
}

fn nuphus(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp nuphus --executable FILE --expected-digest DIGEST --codex-home DIRECTORY --account DIRECTORY --source-root DIRECTORY --connection-seconds SECONDS [--idle-seconds SECONDS] [--server-name NAME]\nServe one Nuphus stdio connection through the audited original binary. Handshake and tools/list stay local when the account catalogue cache matches that digest. Browser tools use a private CDP endpoint unless NUPHUS_MCP_BROWSER_CDP_URL is already set; desktop tools take the account-wide admission lock. Live global registration remains a later lifecycle task."
        );
        return Ok(0);
    }
    let mut options = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(key) = rest.next() {
        if !matches!(
            key.to_str(),
            Some(
                "--executable"
                    | "--expected-digest"
                    | "--codex-home"
                    | "--account"
                    | "--source-root"
                    | "--connection-seconds"
                    | "--idle-seconds"
                    | "--server-name"
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
    let idle = match options.get(&OsString::from("--idle-seconds")) {
        None => None,
        Some(value) => Some(
            value
                .to_str()
                .and_then(|value| value.parse().ok())
                .filter(|value| (1..=3600).contains(value))
                .ok_or_else(invalid)?,
        ),
    };
    let digest = options
        .get(&OsString::from("--expected-digest"))
        .and_then(|value| value.to_str())
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(invalid)?;
    let expected_server = match options.get(&OsString::from("--server-name")) {
        Some(value) => Some(
            value
                .to_str()
                .filter(|name| !name.is_empty())
                .ok_or_else(invalid)?
                .to_owned(),
        ),
        None => Some("nuphus-mcp".into()),
    };
    let configuration = harness_core::nuphus_stdio::Configuration {
        executable: path("--executable")?,
        expected_digest: digest.to_owned(),
        codex_home: path("--codex-home")?,
        account: path("--account")?,
        source_root: path("--source-root")?,
        expected_server,
        idle: idle.map(Duration::from_secs),
        worker_args: Vec::new(),
        browser_cdp_url: None,
    };
    let (input, output) = mcp_stdio::standard_files()?;
    let cancellation = Cancellation::default();
    let deadline = Deadline::after(Duration::from_secs(seconds))?;
    harness_core::nuphus_stdio::serve(configuration, input, output, &cancellation, deadline)?;
    Ok(0)
}

fn prepare_codegraph(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp prepare-codegraph --mode Install|Update|Check|Recover --codex-home DIRECTORY --dependency-state DIRECTORY [--package-root DIRECTORY] [--source DIRECTORY]\nPrepare the native provider projection for the existing installer. Check is read-only; Install/Update may explicitly acquire and probe the pinned published package. With an explicit source root and an adopted interpreter, the projection also switches the Serena connection to the native shared-broker proxy. This command does not write MCP registrations."
        );
        return Ok(0);
    }
    let mut options = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(key) = rest.next() {
        if !matches!(
            key.to_str(),
            Some("--mode" | "--codex-home" | "--dependency-state" | "--package-root" | "--source",)
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
        source_root: options
            .get(&OsString::from("--source"))
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

fn codegraph_control(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp codegraph-control --package-root DIRECTORY [--project DIRECTORY] [--broker-root DIRECTORY] --operation index|sync|status\nRun one deliberate CodeGraph maintenance operation outside a model session through the same bounded runtime and account slot. The exact current directory is the default project. With an explicit broker root the operation routes through that bounded shared worker instead of a fresh direct runtime. No package is downloaded and no model is involved."
        );
        return Ok(0);
    }
    let mut options = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(key) = rest.next() {
        if !matches!(
            key.to_str(),
            Some("--package-root" | "--project" | "--operation" | "--broker-root")
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
    let operation = options
        .get(&OsString::from("--operation"))
        .and_then(|value| value.to_str())
        .ok_or_else(invalid)?;
    let name = match operation {
        "index" => "codegraph_index",
        "sync" => "codegraph_sync",
        "status" => "codegraph_status",
        _ => return Err(invalid()),
    };
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
    let cancel = Cancellation::default();
    let seconds = if operation == "status" { 60 } else { 600 };
    let deadline = Deadline::after(Duration::from_secs(seconds))?;
    if let Some(root) = options.get(&OsString::from("--broker-root")) {
        let root = local_path(Path::new(root))?;
        let client = harness_core::codegraph_broker::Client::new(configuration, root)?;
        client.connect(deadline, &cancel)?;
        let arguments = serde_json::json!({});
        let value = client.call(name, &arguments, deadline, &cancel)?;
        client.disconnect()?;
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(0);
    }
    let mut runtime = harness_core::codegraph_runtime::Runtime::new(configuration)?;
    let called = runtime.call(name, serde_json::json!({}), deadline, &cancel);
    let closed = runtime.close();
    let value = called?;
    closed?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(0)
}
