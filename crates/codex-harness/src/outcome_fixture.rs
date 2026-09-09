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
            &json!({"argv":args,"prompt":prompt,"home":home,"cwd":env::current_dir()?}),
        )?,
    )?;
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
            "grok-4.6"
        } else {
            "gpt-6-astra"
        };
        let recorded_id = if mode == "misnamed-rollout" {
            child
        } else {
            ID
        };
        let records = [
            json!({"type":"session_meta","payload":{"id":recorded_id}}),
            json!({"type":"turn_context","payload":{"model":model,"effort":"xhigh"}}),
            json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":20,"cached_input_tokens":5,"output_tokens":7,"reasoning_output_tokens":2,"total_tokens":27}}}}),
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
    emit(json!({"type":"item.completed","item":{"type":"agent_message","text":"Tests passed"}}))?;
    emit(
        json!({"type":"item.started","item":{"type":"command_execution","exit_code":0,"command":"native verify"}}),
    )?;
    emit(
        json!({"type":"item.completed","item":{"type":"command_execution","exit_code":null,"command":"native verify"}}),
    )?;
    emit(
        json!({"type":"item.completed","item":{"type":"command_execution","id":"read","exit_code":0,"command":"pwd"}}),
    )?;
    emit(
        json!({"type":"item.completed","item":{"type":"command_execution","id":"check","exit_code":0,"command":"native verify"}}),
    )?;
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
