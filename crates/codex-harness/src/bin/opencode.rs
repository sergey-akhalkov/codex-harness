//! Installed OpenCode launch entry point: forward to the registered OpenCode
//! executable inside the account shared CPU budget, preserving arguments,
//! environment, working directory, console, streams and exit code.
//!
//! The harness home only supplies the kit-owned registration record and the
//! location of kit commands; nothing from Codex profiles, models, providers or
//! configuration is read or injected, and an unusable record degrades to the
//! warned PATH resolution performed by the launcher itself.
fn main() {
    let result = (|| {
        let executable = std::env::current_exe()?;
        let home = harness_core::native_launcher::codex_home()?;
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        harness_core::native_launcher::run_opencode(&executable, &home, &args)
    })();
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("codex-harness: {error}");
            std::process::exit(1);
        }
    }
}
