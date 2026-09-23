//! Explicit stdio endpoints; no global registration or implicit provisioning.
use harness_core::{
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
    if args.first().is_some_and(|arg| arg == "prepare-mcp") {
        return prepare_mcp(&args[1..]);
    }
    if args.first().is_some_and(|arg| arg == "apply-registration") {
        return apply_registration(&args[1..]);
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
    if args == ["--help"] {
        println!(
            "codex-harness mcp broker-prepare\ncodex-harness mcp broker-retire --root DIRECTORY\ncodex-harness mcp serena --help"
        );
        println!("codex-harness mcp nuphus --help");
        return Ok(0);
    }
    Err(invalid())
}

fn serena(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp serena --serena FILE --registry FILE --codex-home DIRECTORY --source-root DIRECTORY --connection-seconds SECONDS\nServe one Serena stdio connection as a client of the authenticated shared broker for that CODEX_HOME. The adopted Serena console entry point runs with a generated harness-owned home that pins every adopted language backend, so a session never provisions or updates a package. Forwarded native arguments come from the selected catalogue. Requests are serialized per project worker; memory and onboarding tools stay hidden unless HARNESS_SERENA_UNFILTERED=1."
        );
        return Ok(0);
    }
    let mut options = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(key) = rest.next() {
        if !matches!(
            key.to_str(),
            Some(
                "--serena"
                    | "--registry"
                    | "--codex-home"
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
        serena: path("--serena")?,
        registry: path("--registry")?,
        codex_home: path("--codex-home")?,
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

fn prepare_mcp(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp prepare-mcp --mode Install|Update|Check|Recover --codex-home DIRECTORY [--source DIRECTORY]\nPrepare the native MCP projection for the existing installer. Check is read-only. With an explicit source root and an adopted interpreter, the projection switches the Serena connection to the native shared-broker proxy. This command does not write MCP registrations."
        );
        return Ok(0);
    }
    let mut options = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(key) = rest.next() {
        if !matches!(key.to_str(), Some("--mode" | "--codex-home" | "--source",)) {
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
    let request = harness_core::mcp_preparation::Request {
        codex_home: path("--codex-home")?,
        source_root: options
            .get(&OsString::from("--source"))
            .map(|p| local_path(Path::new(p)))
            .transpose()?,
        inventory: None,
        mode: options
            .get(&OsString::from("--mode"))
            .and_then(|v| v.to_str())
            .ok_or_else(invalid)?
            .into(),
    };
    println!(
        "{}",
        harness_core::mcp_preparation::prepare(&request, &std::env::current_exe()?)?
    );
    Ok(0)
}

fn apply_registration(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp apply-registration --mode Install|Update|Check|Disconnect|Recover --codex-home DIRECTORY [--retained-registrations-json JSON] [--defer-commit] [--preview]\nJournal owned MCP registrations in an explicit Codex home. CodeGraph, Graphify and Codebase Memory are retired from the managed selection: Install/Update remove their owned registrations and never re-register them. Check is read-only. Recover restores interrupted activation. Disconnect removes only the owned native block. Optional retained JSON is a planner handoff for selected retained MCP specs. This command does not search for or mutate a default global Codex home."
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
            Some("--mode" | "--codex-home" | "--retained-registrations-json") => {
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
    let request = harness_core::mcp_registration::RegistrationRequest {
        codex_home: path("--codex-home")?,
        mode: options
            .get(&OsString::from("--mode"))
            .and_then(|value| value.to_str())
            .ok_or_else(invalid)?
            .into(),
        defer_commit: flags.contains_key(&OsString::from("--defer-commit")),
        preview: flags.contains_key(&OsString::from("--preview")),
        retained_registrations: retained,
    };
    println!("{}", harness_core::mcp_registration::apply(&request)?);
    Ok(0)
}
