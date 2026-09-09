//! Integrity-valid predecessor with an indeterminate Check, for recovery tests.
use std::{env, fs, time::Duration};

fn main() {
    let mode = fs::read_to_string(
        env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("failure-mode.txt"),
    )
    .unwrap();
    match mode.trim() {
        "malformed" => println!("not a Check response"),
        "timeout" => std::thread::sleep(Duration::from_secs(60)),
        _ => panic!("unknown owned predecessor mode"),
    }
}
