//! Owned native counterexamples for the independent real-task oracle.
use std::{fs, path::PathBuf};

fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 3 || arguments[0] != "check" {
        std::process::exit(2);
    }
    let workspace = PathBuf::from(&arguments[1]);
    let expected = fs::read(PathBuf::from(&arguments[2])).unwrap_or_default();
    let actual = fs::read(workspace.join("answer.txt")).unwrap_or_default();
    if expected.is_empty() || actual != expected {
        eprintln!("real task result differs from the independently held expected result");
        std::process::exit(1);
    }
    println!("independent task acceptance passed");
}
