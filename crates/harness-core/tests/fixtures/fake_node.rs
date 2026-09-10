//! Native Node double for BasedPyright --version probing. Never registered as Node.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.last().map(String::as_str) != Some("--version") {
        std::process::exit(2);
    }
    if std::env::var_os("PATH").is_some() {
        std::process::exit(3);
    }
    let mode = std::fs::read_to_string(
        std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.parent()
                    .map(|parent| parent.join("fake-node-mode.txt"))
            })
            .unwrap_or_default(),
    )
    .unwrap_or_default();
    match mode.trim() {
        "wrong-version" => {
            println!("basedpyright 1.39.100");
            println!("based on pyright 1.39.10");
        }
        "fail" => std::process::exit(7),
        "flood" => loop {
            println!("{}", "x".repeat(8192));
        },
        _ => {
            println!("basedpyright 1.39.10");
            println!("based on pyright 1.28.0");
        }
    }
}
