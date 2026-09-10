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
    if args == ["codebase-memory", "--help"] || args == ["--help"] {
        println!(
            "codex-harness mcp codebase-memory --executable FILE --cache DIRECTORY --runtime DIRECTORY --account DIRECTORY --catalogue-file JSON --connection-seconds SECONDS\nServe one bounded MCP stdio connection using a saved cbm-catalogue report. Initialize and tools/list read that explicit file without launching CBM. Tool calls may change the selected graph. Infrastructure failure closes the connection; shared broker/global activation are separate."
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
    let (input, output) = mcp_stdio::standard_files()?;
    cbm_stdio::serve(
        configuration,
        input,
        output,
        &Cancellation::default(),
        Deadline::after(Duration::from_secs(seconds))?,
    )?;
    Ok(0)
}
