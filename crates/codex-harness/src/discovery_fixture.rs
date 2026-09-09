//! Owned app-server double for discovery acceptance tests. No models.
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const SENTINEL: &str = "private discovery fixture sentinel";

pub fn run() -> io::Result<()> {
    let mode = env::var("HARNESS_DISCOVERY_FIXTURE").unwrap_or_else(|_| "success".into());
    if mode == "descendant" {
        std::thread::sleep(Duration::from_secs(3));
        fs::write("descendant-survived.txt", "uncontained")?;
        return Ok(());
    }
    if mode == "no-read" {
        std::thread::sleep(Duration::from_secs(20));
    }
    if mode == "root-exit-child" {
        let descendant = Command::new(env::current_exe()?)
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "discovery")
            .env("HARNESS_DISCOVERY_FIXTURE", "descendant")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        fs::write("descendant-pid.txt", descendant.id().to_string())?;
    }
    let home = PathBuf::from(env::var_os("CODEX_HOME").expect("owned fixture home"));
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("fixture-requests.jsonl")?;
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        writeln!(log, "{line}")?;
        log.flush()?;
        if !serve(&mode, &home, request)? {
            return Ok(());
        }
    }
    if mode == "hang-after-eof" {
        std::thread::sleep(Duration::from_secs(20));
    }
    if mode == "nonzero" {
        std::process::exit(19);
    }
    Ok(())
}

fn serve(mode: &str, home: &Path, request: Value) -> io::Result<bool> {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match method {
        "initialize" => {
            if mode == "malformed" {
                eprintln!("{SENTINEL}");
                println!("{{invalid JSON private content");
                io::stdout().flush()?;
                return Ok(true);
            }
            if mode == "truncated" {
                write!(io::stdout(), r#"{{"id":"#)?;
                io::stdout().flush()?;
                return Ok(false);
            }
            if mode == "init-error" || (mode == "arm-after-error" && arm_published(home)) {
                eprintln!("{SENTINEL}");
                emit(
                    json!({"id": reply_id(mode, &id), "error": {"code": -32603, "message": SENTINEL}}),
                )?;
                return Ok(true);
            }
            if mode == "arm-foreign-edit" && arm_published(home) {
                fs::write(home.join("config.toml"), "# FOREIGN_ARM_WRITER\n")?;
            }
            let observed_home = if mode == "wrong-home" {
                env::current_dir()?
            } else {
                home.to_owned()
            };
            let mut result = json!({"userAgent":"owned-discovery-fixture","codexHome":observed_home,
                "platformFamily":"windows","platformOs":"windows"});
            if mode == "missing-home" {
                result.as_object_mut().unwrap().remove("codexHome");
            }
            emit(json!({"id":reply_id(mode,&id),"result":result}))?;
            if mode == "output-limit" {
                io::stdout().write_all(&vec![b'a'; 2 * 1024 * 1024])?;
                io::stdout().flush()?;
                std::thread::sleep(Duration::from_secs(20));
            }
        }
        "initialized" => {}
        "skills/list" => {
            let requested = request
                .pointer("/params/cwds/0")
                .and_then(Value::as_str)
                .unwrap_or("");
            let cwd = if mode == "cwd-mismatch" {
                PathBuf::from(requested).join("other")
            } else {
                PathBuf::from(requested)
            };
            let errors = if mode == "skills-error" {
                eprintln!("{SENTINEL}");
                json!([SENTINEL])
            } else {
                json!([])
            };
            let skills = if mode.starts_with("arm") {
                arm_skills(home, mode)?
            } else {
                vec![
                    skill(home, "project-verification"),
                    skill(home, "reproduce-regression"),
                ]
            };
            emit(json!({
                "id": reply_id(mode, &id),
                "result": {
                    "data": [{
                        "cwd": cwd,
                        "skills": skills,
                        "errors": errors
                    }]
                }
            }))?;
        }
        "config/read" => {
            if mode == "pause-before-config" {
                wait_for_config_continue()?;
            }
            emit(json!({
                "id": reply_id(mode, &id),
                "result": {
                    "config": {"model": "gpt-6-astra", "model_provider": if mode == "arm-config-change" && arm_published(home) { "foreign-provider" } else { "openai" }},
                    "origins": {},
                    "layers": null
                }
            }))?;
            if mode == "duplicate-final" {
                emit(json!({"id":id,"error":{"code":-32603,"message":SENTINEL}}))?;
            } else if mode == "trailing-malformed" {
                println!("invalid trailing private response");
                io::stdout().flush()?;
            } else if mode == "late-server-request" {
                emit(json!({"id":99,"method":"unsupported/server/request","params":{}}))?;
            }
        }
        _ => {}
    }
    Ok(true)
}

fn arm_published(home: &Path) -> bool {
    fs::read_to_string(home.join("config.toml"))
        .is_ok_and(|text| text.contains("[[skills.config]]"))
}

fn arm_skills(home: &Path, mode: &str) -> io::Result<Vec<Value>> {
    let mut skills: Vec<Value> = serde_json::from_slice(&fs::read("fixture-skills.json")?)?;
    let text = fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let config: toml::Value = toml::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| io::Error::other("invalid owned fixture config"))?;
    if mode != "arm-ignore"
        && let Some(entries) = config
            .get("skills")
            .and_then(|s| s.get("config"))
            .and_then(toml::Value::as_array)
    {
        for entry in entries {
            for skill in &mut skills {
                if entry.get("path").and_then(toml::Value::as_str) == skill["path"].as_str() {
                    skill["enabled"] = json!(entry.get("enabled").and_then(toml::Value::as_bool));
                }
            }
        }
    }
    if mode == "arm-unrelated-change" && arm_published(home) {
        for skill in &mut skills {
            if skill["name"] == "personal-owned" {
                skill["description"] = json!("changed unrelated skill");
            }
        }
    }
    Ok(skills)
}

fn wait_for_config_continue() -> io::Result<()> {
    fs::write("config-read-ready", "ready")?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if Path::new("config-read-continue").is_file() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn skill(home: &Path, name: &str) -> Value {
    json!({
        "name": name,
        "path": home.join("skills").join(name).join("SKILL.md"),
        "description": "fixture",
        "enabled": true,
        "scope": "user"
    })
}

fn reply_id(mode: &str, id: &Value) -> Value {
    if mode != "unknown-id" {
        return id.clone();
    }
    if let Some(n) = id.as_i64() {
        json!(n + 100)
    } else if let Some(n) = id.as_u64() {
        json!(n + 100)
    } else {
        id.clone()
    }
}

fn emit(value: Value) -> io::Result<()> {
    println!("{value}");
    io::stdout().flush()
}
