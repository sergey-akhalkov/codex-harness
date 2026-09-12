//! Owned upstream double for native launcher tests. No network or model use.
use std::{
    env,
    io::{self, Read, Write},
};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut stdin = String::new();
    let _ = io::stdin().read_to_string(&mut stdin);
    println!("ARGS:{}", args.join("\u{1f}"));
    print!("STDIN:{}", stdin);
    eprint!("ERR:ok");
    let code = env::var("HARNESS_UPSTREAM_EXIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
