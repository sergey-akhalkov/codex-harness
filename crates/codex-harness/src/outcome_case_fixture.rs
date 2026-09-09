//! Executable Rust counterparts of the controlled outcome case targets.
//! Copy the existing fixture executable into an owned case before invoking it.
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

fn audit(root: &Path, file: &str, row: &Value) -> io::Result<()> {
    let mut out = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(file))?;
    serde_json::to_writer(&mut out, row)?;
    out.write_all(b"\n")?;
    out.flush()
}

fn version(root: &Path, name: &str) -> io::Result<(Vec<u8>, u64)> {
    let bytes = fs::read(root.join(name))?;
    if bytes.len() > 1024 {
        return Err(io::Error::other("owned version input too large"));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    let number = value["version"]
        .as_u64()
        .ok_or_else(|| io::Error::other("invalid owned version"))?;
    Ok((bytes, number))
}

pub fn run() -> io::Result<()> {
    let args: Vec<_> = env::args_os().skip(2).collect();
    if args.len() != 1 {
        return Err(io::Error::other("expected one outcome case mode"));
    }
    let exe = env::current_exe()?;
    let root = exe
        .parent()
        .ok_or_else(|| io::Error::other("case root missing"))?
        .canonicalize()?;
    let temporary = env::temp_dir().canonicalize()?;
    if !root.starts_with(&temporary) || root == temporary {
        return Err(io::Error::other(
            "copy the fixture into an owned temporary case",
        ));
    }
    let mode = args[0]
        .to_str()
        .ok_or_else(|| io::Error::other("invalid case mode"))?;
    if mode == "build" || mode == "cli" {
        let (bytes, number) = version(
            &root,
            if mode == "build" {
                "source.json"
            } else {
                "built.json"
            },
        )?;
        if mode == "build" {
            fs::write(root.join("built.json"), bytes)?;
        }
        audit(
            &root,
            "execution-audit.jsonl",
            &json!({"entrypoint":mode,"version":number}),
        )?;
        if mode == "cli" {
            println!("{number}");
        }
        return Ok(());
    }
    if mode == "descendant" {
        std::thread::sleep(Duration::from_secs(6));
        fs::write(root.join("descendant-survived.txt"), "survived")?;
        return Ok(());
    }
    if !["flood", "fail", "no-ready", "hang"].contains(&mode) {
        return Err(io::Error::other("unknown outcome case mode"));
    }
    audit(
        &root,
        "process-audit.jsonl",
        &json!({"mode":mode,"pid":std::process::id(),"event":"start"}),
    )?;
    match mode {
        "flood" => {
            let stdout =
                std::thread::spawn(|| io::stdout().lock().write_all(&vec![b'a'; 2 * 1024 * 1024]));
            let stderr =
                std::thread::spawn(|| io::stderr().lock().write_all(&vec![b'b'; 2 * 1024 * 1024]));
            stdout
                .join()
                .map_err(|_| io::Error::other("stdout worker failed"))??;
            stderr
                .join()
                .map_err(|_| io::Error::other("stderr worker failed"))??;
        }
        "fail" => {
            eprintln!("natural failure");
            std::process::exit(7);
        }
        "hang" | "no-ready" => {
            if mode == "hang" {
                let _child = Command::new(&exe)
                    .args(["--outcome-case", "descendant"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
            }
            println!("starting");
            io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => unreachable!(),
    }
    audit(
        &root,
        "process-audit.jsonl",
        &json!({"mode":mode,"pid":std::process::id(),"event":"natural-end"}),
    )
}
