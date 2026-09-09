//! Adversarial native finalizer for process/protocol rejection tests only.
use std::{env, fs, io::Write, path::Path, time::Duration};

fn main() {
    let args: Vec<_> = env::args_os().collect();
    let request = Path::new(&args[2]);
    let staging = request.parent().unwrap();
    match staging.file_name().unwrap().to_str().unwrap() {
        "missing" => (),
        "malformed" => fs::write(staging.join("finalize-result.json"), "{not-json").unwrap(),
        "reject" => std::process::exit(7),
        "timeout" => std::thread::sleep(Duration::from_secs(30)),
        "flood" => loop {
            println!("{}", "x".repeat(8192));
        },
        mode @ ("bad-binding" | "bad-record" | "false-verdict" | "changed-request") => {
            fs::copy(
                staging.join("record-template.json"),
                staging.join("build.json"),
            )
            .unwrap();
            fs::copy(
                staging.join("response-template.json"),
                staging.join("finalize-result.json"),
            )
            .unwrap();
            if mode == "bad-record" {
                fs::OpenOptions::new()
                    .append(true)
                    .open(staging.join("build.json"))
                    .unwrap()
                    .write_all(b"\n")
                    .unwrap();
            }
            if mode == "changed-request" {
                fs::OpenOptions::new()
                    .append(true)
                    .open(request)
                    .unwrap()
                    .write_all(b"\n")
                    .unwrap();
            }
        }
        _ => panic!("unknown owned fixture scenario"),
    }
}
