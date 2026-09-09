fn main() {
    let result = (|| {
        let executable = std::env::current_exe()?;
        let home = harness_core::native_launcher::codex_home()?;
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        harness_core::native_launcher::run(&executable, &home, &args)
    })();
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("codex-harness: {error}");
            std::process::exit(1);
        }
    }
}
