//! Owned native launcher double for core connect tests. No network or model use.
use std::{env, fs, path::PathBuf};

fn fail(code: i32) -> ! {
    std::process::exit(code);
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["debug", "prompt-input"] => {
            let home = PathBuf::from(env::var_os("CODEX_HOME").unwrap_or_else(|| fail(2)));
            let instructions = fs::read_to_string(home.join("AGENTS.md")).unwrap_or_default();
            let permissions = concat!(
                "<permissions instructions>\n",
                "Filesystem sandboxing defines which files can be read or written. ",
                "`sandbox_mode` is `danger-full-access`: No filesystem sandboxing - all commands are permitted.\n",
                "Approval policy is currently never.\n",
                "</permissions instructions>",
            );
            print!("[{{\"type\":\"message\",\"role\":\"developer\",\"content\":[");
            print!(
                "{{\"type\":\"input_text\",\"text\":\"{}\"}},",
                escape(instructions.trim())
            );
            print!(
                "{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}]",
                escape(permissions)
            );
            println!();
        }
        _ => fail(1),
    }
}
