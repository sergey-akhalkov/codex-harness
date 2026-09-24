fn main() {
    #[cfg(windows)]
    if let Some(code) = bootstrap_control() {
        std::process::exit(code);
    }
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

/// Existing service-helper commands used to place one uncapped payload outside
/// an already capped caller's job. These never join the shared allowance.
#[cfg(windows)]
fn bootstrap_control() -> Option<i32> {
    use harness_core::native_launcher::{
        CPU_EXCEPTION_LAUNCH, cpu_exception_launch_entry, hold_cpu_exception_anchor,
    };
    use harness_core::process_service::{CREATE_ARGUMENT, RUN_ARGUMENT};
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == CREATE_ARGUMENT {
        harness_core::process_service::create_helper_entry()
    }
    if args.first().is_some_and(|arg| arg == RUN_ARGUMENT) {
        hold_cpu_exception_anchor()
    }
    if args.first().is_some_and(|arg| arg == CPU_EXCEPTION_LAUNCH) {
        return Some(cpu_exception_launch_entry());
    }
    None
}
