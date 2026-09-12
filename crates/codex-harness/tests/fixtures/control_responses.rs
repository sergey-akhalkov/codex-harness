//! Owned Responses fixture for native controller acceptance; never calls a model.
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const FINAL: &str = "CONTROL_TOOL_RESULT_CONSUMED";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quota {
    None,
    Refusal,
    Handoff,
}

pub struct Responses {
    pub port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Responses {
    pub fn start(evidence: PathBuf, direct: bool) -> Self {
        Self::configured(evidence, direct, false, Quota::None)
    }

    #[allow(dead_code)] // Also compiled into the independent transport contract.
    pub fn with_view_loss(evidence: PathBuf) -> Self {
        Self::configured(evidence, true, true, Quota::None)
    }

    #[allow(dead_code)] // Also compiled into the independent transport contract.
    pub fn with_quota_refusal(evidence: PathBuf) -> Self {
        Self::configured(evidence, true, false, Quota::Refusal)
    }

    #[allow(dead_code)]
    pub fn with_quota_handoff(evidence: PathBuf) -> Self {
        Self::configured(evidence, true, false, Quota::Handoff)
    }

    fn configured(evidence: PathBuf, direct: bool, view_loss: bool, quota: Quota) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let mut sequence = 0;
            while !stopping.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let body = read_body(&mut stream);
                let request: Value = serde_json::from_slice(&body).unwrap();
                sequence += 1;
                fs::write(evidence.join(format!("provider-{sequence}.json")), &body).unwrap();
                if quota != Quota::None && sequence == 1 {
                    assert_eq!(request["model"], "gpt-6-astra");
                    if quota == Quota::Handoff {
                        let until = Instant::now() + Duration::from_secs(5);
                        while !evidence.join("controller-state.json").is_file()
                            && Instant::now() < until
                        {
                            thread::sleep(Duration::from_millis(20));
                        }
                        assert!(evidence.join("controller-state.json").is_file());
                    }
                    let body = json!({"error":{"type":"usage_limit_reached","message":"Owned quota refusal before any fallback instruction.","plan_type":"pro","resets_in_seconds":3600}}).to_string();
                    fs::write(evidence.join("quota-response.json"), &body).unwrap();
                    write!(stream, "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    continue;
                }
                assert!(
                    quota != Quota::Refusal,
                    "a refused leader must not be probed again"
                );
                if quota == Quota::Handoff {
                    assert_eq!(
                        request["model"], "zai/glm-5.3",
                        "a fresh exact successor must serve every request after GPT refusal"
                    );
                    assert_eq!(request["reasoning"]["effort"], "high");
                    assert!(request["input"].to_string().contains(
                        "Perform the owned proof command and return its consumed result."
                    ));
                    assert!(
                        request["input"]
                            .to_string()
                            .contains("same already authorized task")
                    );
                    if sequence == 2 {
                        assert_successor_visible(&evidence);
                    }
                }
                assert!(
                    sequence <= 5,
                    "unexpected extra provider request; native goal must stay paused in this transport probe"
                );
                let result_seen = request["input"].as_array().unwrap().iter().any(|item| {
                    matches!(item["type"].as_str(), Some("function_call_output" | "custom_tool_call_output")) && item["call_id"] == "control-tool-1"
                });
                let worker = request["input"].as_array().unwrap().iter().any(|item| {
                    item["type"] == "agent_message" && item["recipient"] == "/root/control_worker"
                });
                let spawned = request["input"].as_array().unwrap().iter().any(|item| {
                    item["type"] == "function_call_output" && item["call_id"] == "control-spawn-1"
                });
                if worker && !direct {
                    assert!(
                        request["input"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|item| item["type"] == "agent_message"
                                && item["content"].as_array().is_some_and(|content| content
                                    .iter()
                                    .any(|part| part["type"] == "input_text"
                                        && part["text"].as_str().is_some_and(
                                            |text| text.contains("CONTROL_CHILD_ASSIGNMENT")
                                        )))),
                        "the child's assignment must arrive as visible text"
                    );
                }
                let title = request["input"].as_array().unwrap().iter().any(|item| {
                    item["role"] == "user"
                        && item["content"].as_array().is_some_and(|parts| {
                            parts.iter().any(|part| {
                                part["text"].as_str().is_some_and(|text| {
                                    text.starts_with("Generate a concise, single-line task title")
                                })
                            })
                        })
                });
                let item = if view_loss && sequence > 1 {
                    assert!(
                        request["input"]
                            .to_string()
                            .contains("Continue only the previously authorized task"),
                        "recovery must receive the bounded visible continuation"
                    );
                    if sequence == 2 {
                        json!({"type":"function_call","call_id":"control-read-1","name":"exec_command",
                            "arguments":serde_json::to_string(&json!({"cmd":"Get-Content proof.txt","shell":"powershell","yield_time_ms":10000})).unwrap()})
                    } else {
                        assert!(
                            request["input"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|item| item["type"] == "function_call_output"
                                    && item["call_id"] == "control-read-1"
                                    && item["output"].as_str().is_some_and(|output| output
                                        .contains("Process exited with code 0")
                                        && output.contains("\none"))),
                            "the continuation must consume a successful read of the preserved effect"
                        );
                        json!({"type":"message","id":"msg-recovered","role":"assistant","content":[{"type":"output_text","text":FINAL}]})
                    }
                } else if title {
                    json!({"type":"message","id":"msg-title","role":"assistant","content":[{"type":"output_text","text":"Verify owned tool result"}]})
                } else if !direct && !worker && !spawned {
                    json!({"type":"function_call","call_id":"control-spawn-1","namespace":"collaboration","name":"spawn_agent","encrypted_function_args":[],"arguments":serde_json::to_string(&json!({"task_name":"control_worker","message":"CONTROL_CHILD_ASSIGNMENT: run the owned proof command, then consume and return its result. Do not spawn other agents."})).unwrap()})
                } else if !direct && !worker && request["input"].to_string().contains(FINAL) {
                    json!({"type":"message","id":"msg-parent-final","role":"assistant","content":[{"type":"output_text","text":FINAL}]})
                } else if !direct && !worker {
                    json!({"type":"message","id":"msg-parent","role":"assistant","content":[{"type":"output_text","text":"Owned child dispatched."}]})
                } else if result_seen {
                    json!({"type":"message","id":"msg-control","role":"assistant","content":[{"type":"output_text","text":FINAL}]})
                } else {
                    json!({"type":"function_call","call_id":"control-tool-1","name":"exec_command","arguments":serde_json::to_string(&json!({
                        "cmd":if view_loss { "[IO.File]::AppendAllText('proof.txt', 'one'); for ($n = 0; $n -lt 600 -and !(Test-Path finish-tool); $n++) { Start-Sleep -Milliseconds 100 }; if (!(Test-Path finish-tool)) { throw 'owned tool release deadline' }; Get-Content proof.txt" }
                            else { "[IO.File]::AppendAllText('proof.txt', 'one'); Start-Sleep -Seconds 3; Get-Content proof.txt" },
                        "shell":"powershell","yield_time_ms":if view_loss { 30000 } else { 10000 }
                    })).unwrap()})
                };
                let events = [
                    json!({"type":"response.created","response":{"id":format!("resp-{sequence}")}}),
                    json!({"type":"response.output_item.done","item":item}),
                    json!({"type":"response.completed","response":{"id":format!("resp-{sequence}"),"usage":{"input_tokens":10,"output_tokens":10,"total_tokens":20}}}),
                ];
                let body = events
                    .iter()
                    .map(|event| {
                        format!(
                            "event: {}\ndata: {event}\n\n",
                            event["type"].as_str().unwrap()
                        )
                    })
                    .collect::<String>();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self {
            port,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Responses {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn assert_successor_visible(evidence: &std::path::Path) {
    let pointer: Value =
        serde_json::from_slice(&fs::read(evidence.join("controller-state.json")).unwrap()).unwrap();
    let state = PathBuf::from(pointer["root"].as_str().unwrap());
    let leader: Value =
        serde_json::from_slice(&fs::read(state.join("leader.json")).unwrap()).unwrap();
    let views: Value =
        serde_json::from_slice(&fs::read(state.join("additional-views.json")).unwrap()).unwrap();
    let initial: Value =
        serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    let id = leader["threadId"].as_str().unwrap();
    assert_ne!(initial["threadId"], id);
    assert_eq!(leader["model"], "zai/glm-5.3");
    assert_eq!(leader["previousThreadId"], initial["threadId"]);
    let native = PathBuf::from(std::env::var_os("HARNESS_CONTROL_CODEX_EXE").unwrap());
    let user = harness_core::process_service::current_user().unwrap();
    for snapshot in [&views["threads"][id], &initial["window"]] {
        let snapshot: harness_core::task_view::Snapshot =
            serde_json::from_value(snapshot.clone()).unwrap();
        assert!(
            snapshot.is_visible(&native, &user).unwrap(),
            "both native conversations must be visible at successor dispatch"
        );
    }
    fs::write(evidence.join("successor-visible-at-request.json"), serde_json::to_vec(&json!({"previousThreadId":initial["threadId"],"successorThreadId":id,"bothVisible":true})).unwrap()).unwrap();
}

fn read_body(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let (start, length) = loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
            assert!(
                header.starts_with("POST /v1/responses "),
                "unexpected fixture endpoint"
            );
            let length = header
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .expect("bounded Content-Length");
            assert!(length <= 2 * 1024 * 1024);
            break (end + 4, length);
        }
        assert!(bytes.len() < 64 * 1024, "fixture header limit");
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count != 0, "incomplete fixture header");
        bytes.extend_from_slice(&chunk[..count]);
    };
    while bytes.len() < start + length {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count != 0, "incomplete fixture body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    bytes[start..start + length].to_vec()
}
