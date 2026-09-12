//! Owned Codex features double for token-workflow lifecycle tests. No network or model use.
use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
};

fn fail(code: i32) -> ! {
    std::process::exit(code);
}

fn config_path() -> PathBuf {
    PathBuf::from(env::var_os("CODEX_HOME").unwrap_or_else(|| fail(2))).join("config.toml")
}

fn read_config() -> String {
    match fs::read_to_string(config_path()) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(_) => fail(1),
    }
}

fn feature_enabled(text: &str, name: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == format!("{name} = true") || trimmed.starts_with(&format!("{name} = true"))
    })
}

fn set_feature(text: &str, name: &str, enabled: bool) -> String {
    let replacement = format!("{name} = {}", if enabled { "true" } else { "false" });
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed == format!("{name} = true")
                || trimmed == format!("{name} = false")
                || trimmed.starts_with(&format!("{name} = "))
            {
                found = true;
                replacement.clone()
            } else {
                line.to_owned()
            }
        })
        .collect();
    if !found {
        if !lines.iter().any(|line| line.trim() == "[features]") {
            if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[features]".into());
        }
        lines.push(replacement);
    }
    let mut body = lines.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body
}

fn write_config(body: &str) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|_| fail(1));
    }
    let mut file = fs::File::create(path).unwrap_or_else(|_| fail(1));
    file.write_all(body.as_bytes()).unwrap_or_else(|_| fail(1));
    file.sync_all().unwrap_or_else(|_| fail(1));
}

fn list() {
    let text = read_config();
    println!(
        "hooks stable {}",
        if feature_enabled(&text, "hooks") {
            "true"
        } else {
            "false"
        }
    );
    println!(
        "code_mode stable {}",
        if feature_enabled(&text, "code_mode") {
            "true"
        } else {
            "false"
        }
    );
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--version"] => println!("codex-cli 0.153.4"),
        ["--help"] => {
            println!("Codex CLI");
            println!("Usage: codex [OPTIONS] [PROMPT]");
            println!("      --profile <PROFILE>          Configuration profile from config.toml");
            println!("      Select <name>.config.toml with --profile.");
        }
        ["features", "list"] => list(),
        ["features", "enable", name] if matches!(*name, "hooks" | "code_mode") => {
            write_config(&set_feature(&read_config(), name, true));
        }
        ["features", "disable", name] if matches!(*name, "hooks" | "code_mode") => {
            write_config(&set_feature(&read_config(), name, false));
        }
        _ => fail(1),
    }
}

