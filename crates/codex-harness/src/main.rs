use harness_core::{build_identity, build_selection, native_build};
use std::{collections::BTreeMap, env, ffi::OsString, io, path::PathBuf};

fn run() -> io::Result<i32> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!(
            "codex-harness build --source CHECKOUT --state DIRECTORY [--cargo EXECUTABLE]\ncodex-harness check --build DIRECTORY [--source CHECKOUT]\ncodex-harness activate-build --state DIRECTORY --build DIRECTORY\ncodex-harness recover-build --state DIRECTORY\nNative migration candidate: immutable build preparation, selection and identity checks. Global lifecycle remains on its existing installation."
        );
        return Ok(0);
    }
    if args[0] == "--version" {
        println!("codex-harness {}", env!("CARGO_PKG_VERSION"));
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
            build_selection::activate(&state, &required("--build")?)?
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
