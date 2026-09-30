//! Owned executable double for the outcome-run acceptance tests. No models.
use serde_json::json;
use std::{
    env, fs,
    io::{self, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

pub fn run() -> io::Result<()> {
    let mode = env::var("HARNESS_OUTCOME_FIXTURE").unwrap_or_else(|_| "success".into());
    if mode == "descendant" {
        std::thread::sleep(Duration::from_secs(3));
        fs::write("descendant-survived.txt", "uncontained")?;
        return Ok(());
    }
    let mut prompt = String::new();
    io::stdin().read_to_string(&mut prompt)?;
    let args: Vec<String> = env::args().skip(1).collect();
    let home = PathBuf::from(env::var_os("CODEX_HOME").expect("owned fixture home"));
    fs::write(
        "fixture-call.json",
        serde_json::to_vec(
            &json!({"argv":args,"prompt":prompt,"home":home,"cwd":env::current_dir()?,
                "userprofile":env::var("USERPROFILE").ok(),
                "openai_api_key_present":env::var_os("OPENAI_API_KEY").is_some()}),
        )?,
    )?;
    // Echo the requested model and reasoning so a synthetic local runner can
    // verify its recorded identity through the real rollout-reader plumbing.
    let requested_model = args
        .windows(2)
        .find(|pair| pair[0] == "-m" || pair[0] == "--model")
        .map(|pair| pair[1].clone());
    let requested_effort = args
        .windows(2)
        .find(|pair| pair[0] == "-c" && pair[1].starts_with("model_reasoning_effort="))
        .map(|pair| {
            pair[1]
                .trim_start_matches("model_reasoning_effort=")
                .trim_matches('"')
                .to_owned()
        });
    // A controlled required solution output; the test supplies identical or
    // divergent content across repeated controlled inputs.
    if let Ok(solution) = env::var("HARNESS_OUTCOME_SOLUTION")
        && !solution.is_empty()
    {
        fs::write("solution.txt", solution)?;
    }
    let final_path = args
        .windows(2)
        .find(|pair| pair[0] == "--output-last-message")
        .map(|pair| PathBuf::from(&pair[1]))
        .expect("final path");
    fs::write(&final_path, "Unverified fixture answer")?;
    eprintln!("private fixture credential: must remain in stderr evidence");
    const ID: &str = "00000000-1111-2222-3333-444444444444";
    let child = "00000000-1111-2222-3333-555555555555";
    if mode != "no-rollout" {
        let sessions = home.join("sessions/2026/09/09");
        fs::create_dir_all(&sessions)?;
        let model = if mode == "wrong-model" {
            "grok-4.6".to_owned()
        } else {
            requested_model
                .clone()
                .unwrap_or_else(|| "gpt-6-astra".to_owned())
        };
        let effort = requested_effort
            .clone()
            .unwrap_or_else(|| "xhigh".to_owned());
        // An intentionally overlapping token subset for the accounting cases:
        // cached input above total input and reasoning above output.
        let (cached, reasoning_tokens) = if env::var("HARNESS_OUTCOME_TOKEN_OVERLAP").is_ok() {
            (25, 9)
        } else {
            (5, 2)
        };
        let recorded_id = if mode == "misnamed-rollout" {
            child
        } else {
            ID
        };
        let records = [
            json!({"type":"session_meta","payload":{"id":recorded_id}}),
            json!({"type":"turn_context","payload":{"model":model,"effort":effort}}),
            json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":20,"cached_input_tokens":cached,"output_tokens":7,"reasoning_output_tokens":reasoning_tokens,"total_tokens":27}}}}),
            json!({"type":"token_usage_record","payload":{"response_id":"response-1","turn_id":"turn-1","usage":{"input_tokens":20,"cached_input_tokens":cached,"output_tokens":7,"reasoning_output_tokens":reasoning_tokens,"total_tokens":27}}}),
        ];
        let bytes = records.iter().map(|r| format!("{r}\n")).collect::<String>();
        fs::write(sessions.join(format!("rollout-{ID}.jsonl")), &bytes)?;
        if mode == "duplicate-rollout" {
            fs::write(sessions.join(format!("other-{ID}.jsonl")), &bytes)?;
        }
    }
    if mode != "missing-thread" {
        emit(json!({"type":"thread.started","thread_id":ID}))?;
    }
    if mode == "conflicting-thread" {
        emit(json!({"type":"thread.started","thread_id":child}))?;
    }
    emit(
        json!({"type":"item.completed","item":{"type":"agent_message","id":"message","text":"Tests passed"}}),
    )?;
    // A text-only completion exercises the unsupported-tool-exchange path:
    // the model answers without any tool round-trip.
    if mode != "text-only" {
        emit(
            json!({"type":"item.started","item":{"type":"command_execution","id":"starting","status":"in_progress","aggregated_output":"","exit_code":0,"command":"native verify"}}),
        )?;
        emit(
            json!({"type":"item.completed","item":{"type":"command_execution","id":"unknown","status":"completed","aggregated_output":"","exit_code":null,"command":"native verify"}}),
        )?;
        emit(
            json!({"type":"item.completed","item":{"type":"command_execution","id":"read","status":"completed","aggregated_output":"","exit_code":0,"command":"pwd"}}),
        )?;
        emit(
            json!({"type":"item.completed","item":{"type":"command_execution","id":"check","status":"completed","aggregated_output":"","exit_code":0,"command":"native verify"}}),
        )?;
    }
    if mode == "oracle"
        && let Ok(skill) = env::var("HARNESS_OUTCOME_ORACLE_SKILL")
    {
        emit(
            json!({"type":"item.completed","item":{"type":"command_execution","id":"skill",
            "status":"completed","aggregated_output":"owned fixture signal",
            "exit_code":0,"command":format!("Get-Content C:/owned/skills/{skill}/SKILL.md")}}),
        )?;
    }
    if mode == "children" {
        emit(
            json!({"type":"item.completed","item":{"type":"collab_tool_call","receiver_thread_ids":[child,child]}}),
        )?;
    }
    if mode == "corrupt" {
        println!("{{invalid JSON private content");
    }
    if mode == "error" {
        emit(json!({"type":"error","message":"private failure"}))?;
    }
    if mode == "failed-turn" {
        emit(json!({"type":"turn.failed","error":{"message":"private failure"}}))?;
    }
    if mode != "incomplete" {
        emit(json!({"type":"turn.completed","usage":{"input_tokens":20,"output_tokens":7}}))?;
    }
    if mode == "truncated" {
        print!("{{\"type\":\"item.");
        io::stdout().flush()?;
    }
    if mode == "timeout" || mode == "cancel" || mode == "root-exit-child" {
        let descendant = Command::new(env::current_exe()?)
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "outcome")
            .env("HARNESS_OUTCOME_FIXTURE", "descendant")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        fs::write("descendant-pid.txt", descendant.id().to_string())?;
        if mode != "root-exit-child" {
            std::thread::sleep(Duration::from_secs(20));
        }
    }
    if mode == "output-limit" {
        io::stdout().write_all(&vec![b'a'; 2 * 1024 * 1024])?;
        io::stdout().flush()?;
        std::thread::sleep(Duration::from_secs(20));
    }
    if mode == "final-limit" {
        fs::write(&final_path, vec![b'b'; 2 * 1024 * 1024])?;
        std::thread::sleep(Duration::from_secs(20));
    }
    if mode == "nonzero" {
        std::process::exit(19);
    }
    Ok(())
}
fn emit(value: serde_json::Value) -> io::Result<()> {
    println!("{value}");
    io::stdout().flush()
}
