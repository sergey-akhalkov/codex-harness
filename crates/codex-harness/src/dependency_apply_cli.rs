//! Explicit native apply/update dispatcher. Check and preview stay silent.
use std::{collections::BTreeMap, env, ffi::OsString, io, path::PathBuf};

fn invalid() -> io::Error {
    io::Error::other("invalid dependency apply options")
}

pub(crate) fn run(args: &[OsString]) -> Option<io::Result<i32>> {
    let operation = args.first()?.to_str()?;
    if !matches!(operation, "apply" | "update") {
        return None;
    }
    Some(execute(operation, &args[1..]))
}

fn execute(_operation: &str, args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness dependencies apply|update --source CHECKOUT --user-home DIRECTORY --state DIRECTORY [--preview|--check] [--node FILE --node-sha256 DIGEST]\nCheck and preview plan without staging, selecting, acquiring packages or stopping shared OpenCode consumers. Apply stages and selects native CodeGraph and, with an explicit Node path and digest, BasedPyright. Remaining backends stay pending for Python apply_selected."
        );
        return Ok(0);
    }
    let mut values = BTreeMap::new();
    let mut preview = false;
    let mut check = false;
    let mut arguments = args.iter();
    while let Some(option) = arguments.next() {
        match option.to_str() {
            Some("--preview") if !preview && !check => preview = true,
            Some("--check") if !check && !preview => check = true,
            Some("--source" | "--user-home" | "--state" | "--node" | "--node-sha256") => {
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
    let value = |name: &str| values.get(&OsString::from(name)).ok_or_else(invalid);
    let node = values.get(&OsString::from("--node")).map(PathBuf::from);
    let node_sha256 = values
        .get(&OsString::from("--node-sha256"))
        .map(|value| value.to_str().ok_or_else(invalid).map(str::to_owned))
        .transpose()?;
    if node.is_some() != node_sha256.is_some() {
        return Err(invalid());
    }
    let report = harness_core::dependency_apply::apply(&harness_core::dependency_apply::Request {
        source: PathBuf::from(value("--source")?),
        user_home: PathBuf::from(value("--user-home")?),
        state: PathBuf::from(value("--state")?),
        manager: env::current_exe()?,
        preview,
        check,
        node,
        node_sha256,
    })?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(0)
}
