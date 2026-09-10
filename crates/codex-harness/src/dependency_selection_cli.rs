//! Explicit retained-candidate validation and selection commands.
use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf};

fn invalid() -> io::Error {
    io::Error::other("invalid dependency selection options")
}

pub(crate) fn run(args: &[OsString]) -> Option<io::Result<i32>> {
    let operation = args.first()?.to_str()?;
    if !matches!(
        operation,
        "validate" | "select" | "selected" | "recover-selection" | "rollback-selection"
    ) {
        return None;
    }
    Some(execute(operation, &args[1..]))
}

fn execute(operation: &str, args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness dependencies validate --stage DIRECTORY --manifest-sha256 DIGEST [--node FILE --node-sha256 DIGEST]\ncodex-harness dependencies select --state DIRECTORY --slot codebase-memory|nuphus|basedpyright --stage DIRECTORY --manifest-sha256 DIGEST [--node FILE --node-sha256 DIGEST]\ncodex-harness dependencies selected|recover-selection --state DIRECTORY --slot NAME\ncodex-harness dependencies rollback-selection --state DIRECTORY --slot NAME --receipt-sha256 DIGEST\nValidation executes the explicit candidate in a bounded private process. Selection retains candidate directories and journals local metadata; global registration is a separate installation step. Selected performs integrity inspection without running package code. The manifest digest must come from the trusted preparation result."
        );
        return Ok(0);
    }
    let allowed: &[&str] = match operation {
        "validate" => &["--stage", "--manifest-sha256", "--node", "--node-sha256"],
        "select" => &[
            "--state",
            "--slot",
            "--stage",
            "--manifest-sha256",
            "--node",
            "--node-sha256",
        ],
        "selected" | "recover-selection" => &["--state", "--slot"],
        "rollback-selection" => &["--state", "--slot", "--receipt-sha256"],
        _ => return Err(invalid()),
    };
    let mut values = BTreeMap::new();
    let mut args = args.iter();
    while let Some(option) = args.next() {
        if !allowed.contains(&option.to_str().ok_or_else(invalid)?)
            || values
                .insert(option.clone(), args.next().ok_or_else(invalid)?.clone())
                .is_some()
        {
            return Err(invalid());
        }
    }
    let value = |name: &str| values.get(&OsString::from(name)).ok_or_else(invalid);
    let text = |name: &str| value(name)?.to_str().ok_or_else(invalid);
    let node_path = values.get(&OsString::from("--node")).map(PathBuf::from);
    let node_sha256 = values.get(&OsString::from("--node-sha256"));
    let node = match (&node_path, node_sha256) {
        (Some(path), Some(hash)) => Some((path.as_path(), hash.to_str().ok_or_else(invalid)?)),
        (None, None) => None,
        _ => return Err(invalid()),
    };
    use harness_core::{dependency_candidate, dependency_selection};
    let report = match operation {
        "validate" => dependency_candidate::validate(
            &PathBuf::from(value("--stage")?),
            text("--manifest-sha256")?,
            node,
        )?
        .report()
        .clone(),
        "select" => dependency_selection::activate(
            &PathBuf::from(value("--state")?),
            text("--slot")?,
            &PathBuf::from(value("--stage")?),
            text("--manifest-sha256")?,
            node,
        )?,
        "selected" => {
            dependency_selection::selected(&PathBuf::from(value("--state")?), text("--slot")?)?
        }
        "recover-selection" => {
            dependency_selection::recover(&PathBuf::from(value("--state")?), text("--slot")?)?
        }
        "rollback-selection" => dependency_selection::rollback(
            &PathBuf::from(value("--state")?),
            text("--slot")?,
            text("--receipt-sha256")?,
        )?,
        _ => return Err(invalid()),
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}
