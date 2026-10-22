//! Owned Responses fixture for native controller acceptance; never calls a model.
//! The model-catalogue probe a frontend may issue before its first turn is
//! answered without a model and without a recorded request.
#![allow(dead_code)] // Each test binary compiles the subset its own cases use.
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[path = "tool_catalogue.rs"]
pub mod tool_catalogue;

pub const FINAL: &str = "CONTROL_TOOL_RESULT_CONSUMED";
pub const PARENT_FINAL: &str = "CONTROL_PARENT_RESULTS_CONSUMED";

/// A frontend may ask the provider for a one-line title beside the goal turn.
/// It is ephemeral: answer it without consuming a scripted provider sequence.
fn asks_for_task_title(request: &Value) -> bool {
    request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["role"] == "user"
                && item["content"].as_array().is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part["text"].as_str().is_some_and(|text| {
                            text.starts_with("Generate a concise, single-line task title")
                        })
                    })
                })
        })
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quota {
    None,
    Refusal,
    Handoff,
}

#[derive(Clone, Copy)]
enum RecoveryScript {
    Single { overload_at: usize },
    Background,
    Exhaust,
    Repeat,
}

/// The owner's PowerShell 7 executable for fabricated owned native actions:
/// the same resolution order the accepted desktop helpers use, so the request
/// never names Windows PowerShell 5.1.
#[allow(dead_code)] // Only the binaries that fabricate an owned action use it.
pub fn owned_pwsh() -> String {
    if let Some(path) = std::env::var_os("HARNESS_ACCEPTANCE_POWERSHELL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return path.to_string_lossy().into_owned();
        }
    }
    let known = PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe");
    if known.is_file() {
        return known.to_string_lossy().into_owned();
    }
    if let Some(found) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|entry| entry.join("pwsh.exe"))
            .find(|entry| entry.is_file())
    }) {
        return found.to_string_lossy().into_owned();
    }
    panic!("owner PowerShell 7 (pwsh.exe) is required for the owned native action")
}

fn tool_item(request: &Value, call_id: &str, cmd: &str, yield_time_ms: u64) -> Value {
    let arguments = serde_json::to_string(&json!({
        "cmd": cmd,
        "shell": owned_pwsh(),
        "yield_time_ms": yield_time_ms
    }))
    .unwrap();
    let custom_exec = request["tools"].as_array().is_some_and(|tools| {
        tools
            .iter()
            .any(|tool| tool["type"] == "custom" && tool["name"] == "exec")
    });
    if custom_exec {
        json!({"type":"custom_tool_call","call_id":call_id,"name":"exec",
            "input":format!("const result = await tools.exec_command({arguments}); text(result);")})
    } else {
        json!({"type":"function_call","call_id":call_id,"name":"exec_command","arguments":arguments})
    }
}

fn goal_complete(call_id: &str) -> Value {
    json!({"type":"function_call","call_id":call_id,"name":"update_goal",
        "arguments":serde_json::to_string(&json!({"status":"complete"})).unwrap()})
}

fn final_message() -> Value {
    json!({"type":"message","id":"msg-recovered","role":"assistant",
        "content":[{"type":"output_text","text":FINAL}]})
}

pub struct Responses {
    pub port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Responses {
    pub fn start(evidence: PathBuf, direct: bool) -> Self {
        Self::configured(evidence, direct, false, Quota::None, false)
    }

    #[allow(dead_code)] // Also compiled into the independent transport contract.
    pub fn with_view_loss(evidence: PathBuf) -> Self {
        Self::configured(evidence, true, true, Quota::None, false)
    }

    #[allow(dead_code)] // Also compiled into the independent transport contract.
    pub fn with_quota_refusal(evidence: PathBuf) -> Self {
        Self::configured(evidence, true, false, Quota::Refusal, false)
    }

    #[allow(dead_code)]
    pub fn with_quota_handoff(evidence: PathBuf) -> Self {
        Self::configured(evidence, true, false, Quota::Handoff, false)
    }

    #[allow(dead_code)]
    pub fn with_background_terminal(evidence: PathBuf) -> Self {
        Self::configured(evidence, true, false, Quota::None, true)
    }

    /// Serves the finite same-thread overload sequence with a long-lived
    /// background terminal. The first provider response starts it; the next
    /// receives the terminal overload; recovery must consume the running
    /// session's existing result rather than restart the process.
    #[allow(dead_code)]
    pub fn with_server_overload_background(evidence: PathBuf) -> Self {
        Self::overload_script(evidence, RecoveryScript::Background)
    }

    /// Serves a finite native goal-recovery sequence. Overload at 2 returns one
    /// owned file effect, a terminal `serverOverloaded` response without retry
    /// advice, the native goal completion call, and the final assistant result.
    /// Overload at 1 terminates the first request, which the stale-resume probe
    /// uses after its deliberate automatic activation. The same thread must
    /// continue across the overload.
    #[allow(dead_code)]
    pub fn with_server_overload(evidence: PathBuf, overload_at: usize) -> Self {
        assert!(
            matches!(overload_at, 1 | 2),
            "the finite owned script overloads on its first or post-tool request"
        );
        Self::overload_script(evidence, RecoveryScript::Single { overload_at })
    }

    /// Serves the initial effect and then a terminal overload for every
    /// automatic recovery turn. Extra requests are retained and refused so a
    /// bug that attempts a sixth resume is observable rather than hanging.
    #[allow(dead_code)]
    pub fn with_server_overload_exhaustion(evidence: PathBuf) -> Self {
        Self::overload_script(evidence, RecoveryScript::Exhaust)
    }

    /// Completes two successive goals on the same thread. Each goal performs
    /// exactly one owned file effect, then overloads, recovers, completes, and
    /// returns its final result.
    #[allow(dead_code)]
    pub fn with_server_overload_repeat(evidence: PathBuf) -> Self {
        Self::overload_script(evidence, RecoveryScript::Repeat)
    }

    fn overload_script(evidence: PathBuf, script: RecoveryScript) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let mut sequence = 0;
            let mut owned_thread = None;
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
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(30)))
                    .unwrap();
                let Some((headers, request_body)) = read_request(&mut stream) else {
                    respond_models(&mut stream);
                    continue;
                };
                let request: Value = serde_json::from_slice(&request_body).unwrap();
                if asks_for_task_title(&request) {
                    fs::write(evidence.join("title-request.json"), &request_body).unwrap();
                    stream
                        .write_all(
                            sse_response(
                                0,
                                json!({"type":"message","id":"msg-title","role":"assistant",
                                    "content":[{"type":"output_text","text":"Owned background recovery"}]}),
                            )
                            .as_bytes(),
                        )
                        .unwrap();
                    continue;
                }
                let identity = harness_core::task_request::RequestIdentity::from_request(&request)
                    .expect("native provider request must carry consistent conversation identity");
                if let Some(expected) = &owned_thread {
                    assert_eq!(
                        &identity.thread, expected,
                        "the recovery continuation must stay on the owned thread"
                    );
                } else {
                    owned_thread = Some(identity.thread);
                }
                sequence += 1;
                fs::write(
                    evidence.join(format!("provider-{sequence}.json")),
                    &request_body,
                )
                .unwrap();
                let timestamp = json!({
                    "sequence": sequence,
                    "atUnixMs": SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|value| value.as_millis())
                        .unwrap_or_default(),
                });
                if let Ok(mut file) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(evidence.join("provider-requests.jsonl"))
                {
                    let _ = writeln!(file, "{timestamp}");
                }
                let tool_result = request["input"].as_array().unwrap().iter().any(|item| {
                    matches!(
                        item["type"].as_str(),
                        Some("function_call_output" | "custom_tool_call_output")
                    ) && item["call_id"] == "control-tool-1"
                });
                let second_tool_result = request["input"].as_array().unwrap().iter().any(|item| {
                    matches!(
                        item["type"].as_str(),
                        Some("function_call_output" | "custom_tool_call_output")
                    ) && item["call_id"] == "control-tool-2"
                });
                let goal_result = |call_id: &str| {
                    request["input"].as_array().unwrap().iter().any(|item| {
                        item["type"] == "function_call_output" && item["call_id"] == call_id
                    })
                };
                let overload = |stream: &mut TcpStream| {
                    let body = json!({
                        "error": {
                            "message": "Synthetic terminal server overload.",
                            "type": "server_error",
                            "param": null,
                            "code": "server_is_overloaded"
                        }
                    })
                    .to_string();
                    fs::write(evidence.join("overload-response.json"), &body).unwrap();
                    send_response(
                        stream,
                        &evidence,
                        &headers,
                        &request_body,
                        format!(
                            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        ),
                        sequence,
                    );
                };
                let item = match script {
                    RecoveryScript::Single { overload_at } => {
                        if sequence == overload_at {
                            if overload_at == 2 {
                                assert!(
                                    tool_result,
                                    "the overload must follow the owned tool result"
                                );
                            }
                            overload(&mut stream);
                            continue;
                        }
                        assert_ne!(
                            overload_at, 1,
                            "the overload-at-one script has no successful request"
                        );
                        if sequence == 1 {
                            assert!(!tool_result);
                            assert!(
                                request["input"]
                                    .to_string()
                                    .contains("Perform the owned proof command"),
                                "the initial turn must carry the explicit owned prompt"
                            );
                            tool_item(
                                &request,
                                "control-tool-1",
                                &format!(
                                    "[IO.File]::AppendAllText('proof.txt', 'one'); Get-Content proof.txt"
                                ),
                                10_000,
                            )
                        } else if sequence == overload_at + 1 {
                            assert!(
                                tool_result,
                                "the continuation must consume the existing tool result"
                            );
                            goal_complete("goal-complete-1")
                        } else if sequence == overload_at + 2 {
                            assert!(
                                goal_result("goal-complete-1"),
                                "the final response must follow goal completion"
                            );
                            final_message()
                        } else {
                            panic!(
                                "unexpected provider request {sequence}; the finite recovery script ended"
                            );
                        }
                    }
                    RecoveryScript::Background => {
                        if sequence == 2 {
                            assert!(
                                tool_result,
                                "the overload must follow the owned tool result"
                            );
                            overload(&mut stream);
                            continue;
                        }
                        if sequence == 1 {
                            assert!(!tool_result);
                            tool_item(
                                &request,
                                "control-tool-1",
                                "[IO.File]::AppendAllText('proof.txt', 'one'); for ($n = 0; $n -lt 600 -and !(Test-Path finish-terminal); $n++) { Start-Sleep -Milliseconds 100 }; if (!(Test-Path finish-terminal)) { throw 'owned terminal release deadline' }; Get-Content proof.txt",
                                1_000,
                            )
                        } else if sequence == 3 {
                            assert!(
                                tool_result,
                                "the continuation must consume the existing tool result"
                            );
                            let output = request["input"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .find(|item| {
                                    matches!(
                                        item["type"].as_str(),
                                        Some("function_call_output" | "custom_tool_call_output")
                                    ) && item["call_id"] == "control-tool-1"
                                })
                                .and_then(|item| item["output"].as_str())
                                .expect(
                                    "the recovered request must carry the running terminal result",
                                );
                            assert!(
                                output.contains("Process running with session ID"),
                                "the recovered request must consume the same running session: {output}"
                            );
                            goal_complete("goal-complete-1")
                        } else if sequence == 4 {
                            assert!(
                                goal_result("goal-complete-1"),
                                "the final response must follow goal completion"
                            );
                            final_message()
                        } else {
                            panic!(
                                "unexpected provider request {sequence}; the finite background recovery script ended"
                            );
                        }
                    }
                    RecoveryScript::Exhaust => {
                        if sequence == 1 {
                            assert!(!tool_result);
                            assert!(
                                request["input"]
                                    .to_string()
                                    .contains("Perform the owned proof command"),
                                "the initial turn must carry the explicit owned prompt"
                            );
                            tool_item(
                                &request,
                                "control-tool-1",
                                "[IO.File]::AppendAllText('proof.txt', 'one'); Get-Content proof.txt",
                                10_000,
                            )
                        } else {
                            assert!(
                                tool_result,
                                "every exhausted continuation retains the completed tool result"
                            );
                            overload(&mut stream);
                            continue;
                        }
                    }
                    RecoveryScript::Repeat => match sequence {
                        1 => {
                            assert!(!tool_result);
                            assert!(
                                request["input"]
                                    .to_string()
                                    .contains("Perform the owned proof command"),
                                "the first goal must carry its explicit objective"
                            );
                            tool_item(
                                &request,
                                "control-tool-1",
                                "[IO.File]::AppendAllText('proof-first.txt', 'one'); Get-Content proof-first.txt",
                                10_000,
                            )
                        }
                        2 => {
                            assert!(
                                tool_result,
                                "the first overload follows its completed effect"
                            );
                            overload(&mut stream);
                            continue;
                        }
                        3 => {
                            assert!(
                                tool_result && !second_tool_result,
                                "the first recovery consumes only the first effect"
                            );
                            goal_complete("goal-complete-1")
                        }
                        4 => {
                            assert!(
                                goal_result("goal-complete-1"),
                                "the first final response follows completion"
                            );
                            final_message()
                        }
                        5 => {
                            assert!(
                                tool_result && !second_tool_result,
                                "the second goal starts before its own tool result"
                            );
                            assert!(
                                request["input"]
                                    .to_string()
                                    .contains("Complete the second owned proof"),
                                "the second goal must carry its explicit objective"
                            );
                            tool_item(
                                &request,
                                "control-tool-2",
                                "[IO.File]::AppendAllText('proof-second.txt', 'two'); Get-Content proof-second.txt",
                                10_000,
                            )
                        }
                        6 => {
                            assert!(
                                second_tool_result,
                                "the second overload follows its completed effect"
                            );
                            overload(&mut stream);
                            continue;
                        }
                        7 => {
                            assert!(
                                second_tool_result,
                                "the second recovery consumes the second effect"
                            );
                            goal_complete("goal-complete-2")
                        }
                        8 => {
                            assert!(
                                goal_result("goal-complete-2"),
                                "the second final response follows completion"
                            );
                            final_message()
                        }
                        _ => panic!(
                            "unexpected provider request {sequence}; the finite repeat script ended"
                        ),
                    },
                };
                send_response(
                    &mut stream,
                    &evidence,
                    &headers,
                    &request_body,
                    sse_response(sequence, item),
                    sequence,
                );
            }
        });
        Self {
            port,
            stop,
            worker: Some(worker),
        }
    }

    /// Serves one owned provider whose answer to every request comes from the
    /// caller's handler: the request identity is verified and the exact body is
    /// kept beside the evidence before the handler's item is streamed back,
    /// exactly as the scripted cases above are served. The installed two-lead
    /// isolation acceptance uses it where one owned provider serves both native
    /// sessions of one lead root, so a request for any other conversation
    /// reaches the handler instead of a script that assumes its own thread.
    #[allow(dead_code)] // Also compiled into the independent transport contract.
    pub fn routed(evidence: PathBuf, handler: impl Fn(&Value) -> Value + Send + 'static) -> Self {
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
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(30)))
                    .unwrap();
                let Some((_headers, request_body)) = read_request(&mut stream) else {
                    respond_models(&mut stream);
                    continue;
                };
                let request: Value = serde_json::from_slice(&request_body).unwrap();
                let _identity = harness_core::task_request::RequestIdentity::from_request(&request)
                    .expect("native provider request must carry consistent conversation and attempt identity");
                sequence += 1;
                fs::create_dir_all(&evidence).unwrap();
                fs::write(
                    evidence.join(format!("provider-{sequence}.json")),
                    &request_body,
                )
                .unwrap();
                let item = handler(&request);
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
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            }
        });
        Self {
            port,
            stop,
            worker: Some(worker),
        }
    }

    fn configured(
        evidence: PathBuf,
        direct: bool,
        view_loss: bool,
        quota: Quota,
        background: bool,
    ) -> Self {
        let pair = evidence.join("two-executors").is_file();
        let helper_case = evidence.join("helper-window").is_file();
        let close_view = evidence.join("close-view").is_file();
        let catalogue_probe = evidence.join("catalogue-probe").is_file();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let mut sequence = 0;
            let mut probed = HashSet::new();
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
                let Some((headers, request_body)) = read_request(&mut stream) else {
                    respond_models(&mut stream);
                    continue;
                };
                let request: Value = serde_json::from_slice(&request_body).unwrap();
                let identity = harness_core::task_request::RequestIdentity::from_request(&request)
                    .expect("native provider request must carry consistent conversation and attempt identity");
                sequence += 1;
                fs::write(
                    evidence.join(format!("provider-{sequence}.json")),
                    &request_body,
                )
                .unwrap();
                if sequence == 1 && evidence.join("require-initial-view").is_file() {
                    assert_initial_visible(&evidence, &identity.thread);
                }
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
                    send_response(
                        &mut stream,
                        &evidence,
                        &headers,
                        &request_body,
                        format!(
                            "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        ),
                        sequence,
                    );
                    continue;
                }
                assert!(
                    quota != Quota::Refusal,
                    "a refused leader must not be probed again"
                );
                if quota == Quota::Handoff {
                    assert_eq!(
                        request["model"], "deepseek-flash",
                        "a fresh exact successor must serve every request after GPT refusal"
                    );
                    assert_eq!(request["reasoning"]["effort"], "max");
                    assert!(request["input"].to_string().contains(
                        "Perform the owned proof command and return its consumed result."
                    ));
                    assert!(
                        request["input"]
                            .to_string()
                            .contains("same already authorized task")
                    );
                    if sequence == 2 {
                        assert_successor_visible(&evidence, &identity.thread);
                    }
                }
                assert!(
                    sequence
                        <= if pair {
                            9
                        } else if helper_case {
                            7
                        } else {
                            5
                        },
                    "unexpected extra provider request; native goal must stay paused in this transport probe"
                );
                let result_seen = request["input"].as_array().unwrap().iter().any(|item| {
                    matches!(
                        item["type"].as_str(),
                        Some("function_call_output" | "custom_tool_call_output")
                    ) && item["call_id"] == "control-tool-1"
                });
                let second_worker = pair
                    && request["input"].as_array().unwrap().iter().any(|item| {
                        item["type"] == "agent_message"
                            && item["recipient"] == "/root/control_worker_two"
                    });
                // Both executors use the same configured model. Identify their
                // actual assignment instead of conflating a model with a role.
                let assigned = |marker: &str| {
                    request["input"].as_array().unwrap().iter().any(|item| {
                        (item["type"] == "agent_message" || item["role"] == "user")
                            && item["content"].as_array().is_some_and(|content| {
                                content.iter().any(|part| {
                                    part["text"]
                                        .as_str()
                                        .is_some_and(|text| text.contains(marker))
                                })
                            })
                    })
                };
                let second_worker = second_worker
                    || pair
                        && assigned("CONTROL_CHILD_ASSIGNMENT: run the second owned proof command");
                let worker = second_worker
                    || pair && assigned("CONTROL_CHILD_ASSIGNMENT: run the owned proof command")
                    || request["input"].as_array().unwrap().iter().any(|item| {
                        item["type"] == "agent_message"
                            && item["recipient"] == "/root/control_worker"
                    });
                let spawned = request["input"].as_array().unwrap().iter().any(|item| {
                    item["type"] == "function_call_output" && item["call_id"] == "control-spawn-1"
                });
                let second_spawned = request["input"].as_array().unwrap().iter().any(|item| {
                    item["type"] == "function_call_output" && item["call_id"] == "control-spawn-2"
                });
                let helper_worker = request["input"].as_array().unwrap().iter().any(|item| {
                    (item["type"] == "agent_message" || item["role"] == "user")
                        && item["content"].as_array().is_some_and(|content| {
                            content.iter().any(|part| {
                                part["text"]
                                    .as_str()
                                    .is_some_and(|text| text.contains("CONTROL_HELPER_ASSIGNMENT"))
                            })
                        })
                });
                let helper_spawned = request["input"].as_array().unwrap().iter().any(|item| {
                    item["type"] == "function_call_output"
                        && item["call_id"] == "control-spawn-helper"
                });
                let helper_waited = request["input"].as_array().unwrap().iter().any(|item| {
                    item["type"] == "function_call_output"
                        && item["call_id"] == "control-wait-helper"
                });
                let worker = worker && !helper_worker;
                if pair && worker {
                    assert_eq!(request["model"], "deepseek-flash");
                    assert_eq!(request["reasoning"]["effort"], "max");
                    let assignment = if second_worker {
                        "run the second owned proof command"
                    } else {
                        "run the owned proof command"
                    };
                    assert!(
                        request["input"].as_array().unwrap().iter().any(|item| {
                            (item["type"] == "agent_message" || item["role"] == "user")
                                && (item["text"]
                                    .as_str()
                                    .is_some_and(|text| text.contains(assignment))
                                    || item["content"].as_array().is_some_and(|parts| {
                                        parts.iter().any(|part| {
                                            part["text"]
                                                .as_str()
                                                .is_some_and(|text| text.contains(assignment))
                                        })
                                    }))
                        }),
                        "the selected model must receive its own assignment"
                    );
                    assert_executor_visible(&evidence, &identity.thread);
                }
                if worker && !direct {
                    assert!(
                        request["input"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|item| (item["type"] == "agent_message"
                                || pair && item["role"] == "user")
                                && item["content"].as_array().is_some_and(|content| content
                                    .iter()
                                    .any(|part| part["type"] == "input_text"
                                        && part["text"].as_str().is_some_and(
                                            |text| text.contains("CONTROL_CHILD_ASSIGNMENT")
                                        )))),
                        "the child's assignment must arrive as visible text"
                    );
                }
                if helper_worker {
                    assert_helper_visible(&evidence, &identity.thread);
                } else if helper_case && worker {
                    assert_executor_visible(&evidence, &identity.thread);
                }
                let title = asks_for_task_title(&request);
                let recovering = request["input"]
                    .to_string()
                    .contains("Continue only the previously authorized task");
                let item = if recovering && (view_loss || close_view) {
                    let read_done = request["input"].as_array().unwrap().iter().any(|item| {
                        item["type"] == "function_call_output"
                            && item["call_id"] == "control-read-1"
                    });
                    if !read_done {
                        json!({"type":"function_call","call_id":"control-read-1","name":"exec_command",
                            "arguments":serde_json::to_string(&json!({"cmd":"Get-Content proof.txt","shell":owned_pwsh(),"yield_time_ms":10000})).unwrap()})
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
                } else if helper_worker && result_seen {
                    json!({"type":"message","id":"msg-helper","role":"assistant","content":[{"type":"output_text","text":"CONTROL_HELPER_RESULT_CONSUMED"}]})
                } else if helper_worker {
                    let arguments=serde_json::to_string(&json!({
                        "cmd":"[IO.File]::AppendAllText('proof.txt', 'one'); Start-Sleep -Seconds 3; Get-Content proof.txt",
                        "shell":owned_pwsh(),"yield_time_ms":10000
                    })).unwrap();
                    if request["tools"].as_array().is_some_and(|tools| {
                        tools
                            .iter()
                            .any(|tool| tool["type"] == "custom" && tool["name"] == "exec")
                    }) {
                        json!({"type":"custom_tool_call","call_id":"control-tool-1","name":"exec","input":format!("const result = await tools.exec_command({arguments}); text(result);")})
                    } else {
                        json!({"type":"function_call","call_id":"control-tool-1","name":"exec_command","arguments":arguments})
                    }
                } else if !direct && !worker && !helper_worker && !spawned {
                    let mut assignment = json!({"task_name":"control_worker","message":"CONTROL_CHILD_ASSIGNMENT: run the owned proof command, then consume and return its result. Do not spawn other agents."});
                    if pair {
                        assignment["model"] = json!("deepseek-flash");
                        assignment["reasoning_effort"] = json!("max");
                        assignment["fork_turns"] = json!("none");
                    }
                    json!({"type":"function_call","call_id":"control-spawn-1","namespace":"collaboration","name":"spawn_agent","encrypted_function_args":[],"arguments":serde_json::to_string(&assignment).unwrap()})
                } else if pair && !worker && !second_spawned {
                    json!({"type":"function_call","call_id":"control-spawn-2","namespace":"collaboration","name":"spawn_agent","encrypted_function_args":[],"arguments":serde_json::to_string(&json!({"task_name":"control_worker_two","model":"deepseek-flash","reasoning_effort":"max","fork_turns":"none","message":"CONTROL_CHILD_ASSIGNMENT: run the second owned proof command, then consume and return its result. Do not spawn other agents."})).unwrap()})
                } else if pair && !worker {
                    let input = request["input"].as_array().unwrap();
                    let output = |call: &str| {
                        input
                            .iter()
                            .find(|item| {
                                item["type"] == "function_call_output" && item["call_id"] == call
                            })
                            .and_then(|item| item["output"].as_str())
                    };
                    for call in ["control-spawn-1", "control-spawn-2"] {
                        if let Some(text) = output(call) {
                            assert!(
                                !text.contains("unsupported call"),
                                "nested executor spawn must be a native collaboration call: {text}"
                            );
                        }
                    }
                    let transcript = request["input"].to_string();
                    let waited = input
                        .iter()
                        .filter(|item| {
                            item["type"] == "function_call_output"
                                && item["call_id"]
                                    .as_str()
                                    .is_some_and(|id| id.starts_with("control-wait-"))
                        })
                        .count();
                    if (transcript.contains("CONTROL_TOOL_RESULT_CONSUMED_ONE")
                        && transcript.contains("CONTROL_TOOL_RESULT_CONSUMED_TWO"))
                        || waited >= 2
                    {
                        json!({"type":"message","id":"msg-parent-final","role":"assistant","content":[{"type":"output_text","text":PARENT_FINAL}]})
                    } else {
                        json!({"type":"function_call","call_id":format!("control-wait-{}", waited + 1),"namespace":"collaboration","name":"wait_agent","arguments":serde_json::to_string(&json!({"timeout_ms":30000})).unwrap()})
                    }
                } else if !direct && !worker && request["input"].to_string().contains(FINAL) {
                    json!({"type":"message","id":"msg-parent-final","role":"assistant","content":[{"type":"output_text","text":FINAL}]})
                } else if !direct && !worker && !helper_worker {
                    json!({"type":"message","id":"msg-parent","role":"assistant","content":[{"type":"output_text","text":"Owned child dispatched."}]})
                } else if helper_case && worker && !helper_spawned {
                    json!({"type":"function_call","call_id":"control-spawn-helper","namespace":"collaboration","name":"spawn_agent","encrypted_function_args":[],"arguments":serde_json::to_string(&json!({"task_name":"control_helper","message":"CONTROL_HELPER_ASSIGNMENT: run the owned proof command, then consume and return its result. Do not spawn other agents."})).unwrap()})
                } else if helper_case && worker && !helper_waited {
                    let output = request["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|item| {
                            item["type"] == "function_call_output"
                                && item["call_id"] == "control-spawn-helper"
                        })
                        .and_then(|item| item["output"].as_str())
                        .expect("native helper spawn result must identify the child");
                    assert!(
                        !output.contains("unsupported call"),
                        "nested helper spawn must be a native collaboration call: {output}"
                    );
                    json!({"type":"function_call","call_id":"control-wait-helper","namespace":"collaboration","name":"wait_agent","arguments":serde_json::to_string(&json!({"timeout_ms":30000})).unwrap()})
                } else if helper_case && worker {
                    json!({"type":"message","id":"msg-executor-helper","role":"assistant","content":[{"type":"output_text","text":FINAL}]})
                } else if result_seen {
                    if probed.contains(&identity.thread) {
                        // The session preserves the invocation and its result
                        // as two items sharing the call id, so the result of
                        // the observed code-mode `exec` route is selected by
                        // its type (`custom_tool_call_output`), never the
                        // invocation.
                        let output = request["input"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|item| {
                                item["type"] == "custom_tool_call_output"
                                    && item["call_id"] == "control-tool-1"
                            })
                            .and_then(|item| item.get("output"))
                            .expect("the native session must return the owned exec call's output");
                        let names = tool_catalogue::probe_names(output).unwrap_or_else(|| {
                            panic!(
                                "the code-mode exec probe returned no runtime catalogue marker; the session reported no runtime tool names"
                            )
                        });
                        fs::write(
                            evidence.join("catalogue.json"),
                            serde_json::to_vec(&json!({
                                "schema":1,
                                "callId":"control-tool-1",
                                "sequence":sequence,
                                "names":names,
                            }))
                            .unwrap(),
                        )
                        .unwrap();
                    }
                    if pair {
                        assert!(
                            request["input"].as_array().unwrap().iter().any(|item| {
                                item["call_id"] == "control-tool-1"
                                    && (item["output"].as_str().is_some_and(|output| {
                                        output.contains("Process exited with code 0")
                                            && output.contains("\none")
                                    }) || item["output"].as_array().is_some_and(|parts| {
                                        parts.iter().any(|part| {
                                            part["text"]
                                                .as_str()
                                                .and_then(|text| {
                                                    serde_json::from_str::<Value>(text).ok()
                                                })
                                                .is_some_and(|result| {
                                                    result["exit_code"] == 0
                                                        && result["output"].as_str().is_some_and(
                                                            |output| output.trim() == "one",
                                                        )
                                                })
                                        })
                                    }))
                            }),
                            "both executor tools must finish successfully after observing their peer"
                        );
                    }
                    let result = if pair {
                        format!("{FINAL}_{}", if second_worker { "TWO" } else { "ONE" })
                    } else {
                        FINAL.to_owned()
                    };
                    json!({"type":"message","id":"msg-control","role":"assistant","content":[{"type":"output_text","text":result}]})
                } else {
                    let pair_command = format!(
                        "[IO.File]::AppendAllText('{}', 'one'); for ($n = 0; $n -lt 200 -and !(Test-Path '{}'); $n++) {{ Start-Sleep -Milliseconds 100 }}; if (!(Test-Path '{}')) {{ throw 'second executor did not run concurrently' }}; Get-Content '{}'",
                        if second_worker {
                            "proof-two.txt"
                        } else {
                            "proof.txt"
                        },
                        if second_worker {
                            "proof.txt"
                        } else {
                            "proof-two.txt"
                        },
                        if second_worker {
                            "proof.txt"
                        } else {
                            "proof-two.txt"
                        },
                        if second_worker {
                            "proof-two.txt"
                        } else {
                            "proof.txt"
                        }
                    );
                    let arguments=serde_json::to_string(&json!({
                        "cmd":if pair { pair_command.as_str() } else if view_loss || close_view || background { "[IO.File]::AppendAllText('proof.txt', 'one'); for ($n = 0; $n -lt 600 -and !(Test-Path finish-tool); $n++) { Start-Sleep -Milliseconds 100 }; if (!(Test-Path finish-tool)) { throw 'owned tool release deadline' }; Get-Content proof.txt" }
                            else { "[IO.File]::AppendAllText('proof.txt', 'one'); [IO.File]::WriteAllText('shell-execution.json', (@{exe=(Get-Process -Id $PID).Path; edition=$PSVersionTable.PSEdition; major=$PSVersionTable.PSVersion.Major; version=$PSVersionTable.PSVersion.ToString()} | ConvertTo-Json -Compress)); Start-Sleep -Seconds 3; Get-Content proof.txt" },
                        "shell":owned_pwsh(),"yield_time_ms":if background { 1000 } else if view_loss || close_view || pair { 30000 } else { 10000 }
                    })).unwrap();
                    // The owned action may run through the session's own
                    // code-mode `exec` orchestrator. The oldest native shape
                    // advertises that custom tool as a top-level `tools` entry;
                    // the observed shape advertises it inside the developer
                    // `additional_tools` namespace. Only a case that owns the
                    // runtime catalogue probe recognises the newer shape and
                    // prepends the shared probe script, so every other case
                    // keeps its exact former tool call.
                    let custom_exec = request["tools"].as_array().is_some_and(|tools| {
                        tools
                            .iter()
                            .any(|tool| tool["type"] == "custom" && tool["name"] == "exec")
                    }) || catalogue_probe && advertises_custom_exec(&request);
                    if custom_exec {
                        let input = if catalogue_probe {
                            probed.insert(identity.thread.clone());
                            format!(
                                "{}\nconst result = await tools.exec_command({arguments}); text(result);",
                                tool_catalogue::all_tools_probe_script()
                            )
                        } else {
                            format!(
                                "const result = await tools.exec_command({arguments}); text(result);"
                            )
                        };
                        json!({"type":"custom_tool_call","call_id":"control-tool-1","name":"exec","input":input})
                    } else {
                        json!({"type":"function_call","call_id":"control-tool-1","name":"exec_command","arguments":arguments})
                    }
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
                send_response(
                    &mut stream,
                    &evidence,
                    &headers,
                    &request_body,
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    ),
                    sequence,
                );
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

fn reveal_snapshot(snapshot: &harness_core::task_view::Snapshot) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    unsafe {
        SetWindowPos(
            snapshot.window as _,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

fn view_executable(evidence: &std::path::Path) -> PathBuf {
    std::env::var_os("HARNESS_CONTROL_CODEX_EXE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| evidence.join("build").join("codex.exe"))
}

fn assert_initial_visible(evidence: &std::path::Path, requested_thread: &str) {
    let pointer: Value = serde_json::from_slice(
        &fs::read(evidence.join("controller-state.json"))
            .expect("controller must be discoverable at first request"),
    )
    .unwrap();
    let root = std::path::Path::new(pointer["root"].as_str().unwrap());
    let view: Value = serde_json::from_slice(
        &fs::read(root.join("view.json")).expect("initial native view must precede first request"),
    )
    .unwrap();
    let dispatch: Value = serde_json::from_slice(
        &fs::read(root.join("initial-dispatch.json"))
            .expect("controller must own initial input dispatch"),
    )
    .unwrap();
    assert_eq!(view["threadId"], dispatch["threadId"]);
    assert_eq!(view["threadId"], requested_thread);
    let snapshot: harness_core::task_view::Snapshot =
        serde_json::from_value(view["window"].clone()).unwrap();
    let native = view_executable(evidence);
    reveal_snapshot(&snapshot);
    assert!(
        snapshot
            .is_visible(
                &native,
                &harness_core::process_service::current_user().unwrap()
            )
            .unwrap()
    );
    fs::write(
        evidence.join("initial-visible-at-request.json"),
        serde_json::to_vec(
            &json!({"threadId":view["threadId"],"visible":true,"controllerDispatch":true}),
        )
        .unwrap(),
    )
    .unwrap();
}

fn assert_executor_visible(evidence: &std::path::Path, requested_thread: &str) {
    let pointer: Value =
        serde_json::from_slice(&fs::read(evidence.join("controller-state.json")).unwrap()).unwrap();
    let state = PathBuf::from(pointer["root"].as_str().unwrap());
    let views: Value =
        serde_json::from_slice(&fs::read(state.join("additional-views.json")).unwrap()).unwrap();
    let initial: Value =
        serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    assert_ne!(initial["threadId"], requested_thread);
    assert!(!views["threads"][requested_thread].is_null());
    let native = view_executable(evidence);
    let user = harness_core::process_service::current_user().unwrap();
    let mut checked = Vec::new();
    let mut hidden = Vec::new();
    for value in
        std::iter::once(&initial["window"]).chain(views["threads"].as_object().unwrap().values())
    {
        let snapshot: harness_core::task_view::Snapshot =
            serde_json::from_value(value.clone()).unwrap();
        let inspect = harness_core::process_service::ServiceProcess::inspect(
            snapshot.process,
            &native,
            &user,
        );
        let (visible, inspect_state) = match inspect {
            Ok(Some(_)) => (
                snapshot.is_visible(&native, &user).unwrap_or(false),
                json!("running"),
            ),
            Ok(None) => (false, json!("missing")),
            Err(error) => (false, json!(error.to_string())),
        };
        let record = json!({
            "pid": snapshot.process.pid,
            "window": snapshot.window,
            "bounds": snapshot.bounds,
            "visible": visible,
            "inspect": inspect_state,
        });
        if !visible {
            hidden.push(record.clone());
        }
        checked.push(record);
    }
    let _ = fs::write(
        evidence.join("visibility-at-dispatch.json"),
        serde_json::to_vec_pretty(&json!({
            "requestedThread": requested_thread,
            "native": native,
            "conversations": checked,
        }))
        .unwrap(),
    );
    assert!(
        checked.iter().all(|record| record["inspect"] == "running"),
        "all established conversations must keep a live window: {}",
        Value::Array(hidden)
    );
}

fn assert_helper_visible(evidence: &std::path::Path, requested_thread: &str) {
    assert_executor_visible(evidence, requested_thread);
    let pointer: Value =
        serde_json::from_slice(&fs::read(evidence.join("controller-state.json")).unwrap()).unwrap();
    let state = PathBuf::from(pointer["root"].as_str().unwrap());
    let requests: Value =
        serde_json::from_slice(&fs::read(state.join("child-view-requests.json")).unwrap()).unwrap();
    assert_eq!(requests[requested_thread]["title"], "Helper 1");
    assert!(
        requests[requested_thread]["slot"].as_u64().unwrap() >= 4,
        "helpers occupy panes after the two executor slots"
    );
}

fn assert_successor_visible(evidence: &std::path::Path, requested_thread: &str) {
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
    assert_eq!(id, requested_thread);
    assert_ne!(initial["threadId"], id);
    assert_eq!(leader["model"], "deepseek-flash");
    assert_eq!(leader["previousThreadId"], initial["threadId"]);
    let native = view_executable(evidence);
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

fn send_response(
    stream: &mut TcpStream,
    evidence: &std::path::Path,
    headers: &[(String, String)],
    request_body: &[u8],
    response: String,
    sequence: usize,
) {
    if !evidence.join("require-initial-view").is_file() {
        stream.write_all(response.as_bytes()).unwrap();
        return;
    }
    let pointer: Value =
        serde_json::from_slice(&fs::read(evidence.join("controller-state.json")).unwrap()).unwrap();
    let root = harness_core::broker_state::BrokerRoot::open(std::path::Path::new(
        pointer["root"].as_str().unwrap(),
    ))
    .unwrap();
    let native = view_executable(evidence);
    let mut gate = harness_core::task_admission::Gate::new(&native).unwrap();
    let forwarder = harness_core::task_forward::Forwarder::new().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!(
        "http://127.0.0.1:{}/v1/responses",
        listener.local_addr().unwrap().port()
    );
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let expected_body = request_body.to_vec();
    let expected_auth = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.clone());
    let upstream = thread::spawn(move || {
        let mut socket = loop {
            if stopping.load(Ordering::Relaxed) {
                return false;
            }
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("owned gated upstream: {error}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let Some((headers, body)) = read_request(&mut socket) else {
            panic!(
                "owned gated upstream received a model-catalogue probe instead of the one Responses POST"
            );
        };
        assert_eq!(body, expected_body, "gate must not rewrite model request");
        let auth = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone());
        assert_eq!(
            auth, expected_auth,
            "native authorization must be preserved"
        );
        socket.write_all(response.as_bytes()).unwrap();
        true
    });
    let forwarded = gate.forward(
        &forwarder,
        &root,
        harness_core::task_forward::ForwardRequest {
            url: &url,
            headers,
            body: request_body,
        },
        harness_core::process::Deadline::after(Duration::from_secs(15)).unwrap(),
        &harness_core::process::Cancellation::default(),
        |bytes| stream.write_all(bytes),
    );
    stop.store(true, Ordering::Relaxed);
    let accepted = upstream.join().unwrap();
    forwarded.expect("native request must pass owned attempt/view admission");
    assert!(accepted);
    fs::write(
        evidence.join(format!("forwarded-{sequence}.json")),
        b"{\"nativeRequestUnchanged\":true,\"authorizationPreserved\":true,\"viewAdmitted\":true}",
    )
    .unwrap();
}

fn sse_response(sequence: usize, item: Value) -> String {
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
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Answers one frontend model-catalogue probe without a model and without a
/// recorded request; the Responses contract of this fixture is unchanged.
fn respond_models(stream: &mut TcpStream) {
    let body = json!({
        "object": "list",
        "data": [{"id": "gpt-6-astra", "object": "model", "owned_by": "canned"}]
    })
    .to_string();
    stream
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
}

/// Whether one native request advertises the session's code-mode `exec`
/// custom tool inside the developer `additional_tools` input namespace, the
/// shape observed from the current runtime. The tool may sit directly in a
/// namespace's entries or deeper, so the entries are visited recursively.
fn advertises_custom_exec(request: &Value) -> bool {
    fn visit(items: &[Value]) -> bool {
        items.iter().any(|item| {
            (item["type"] == "custom" && item["name"] == "exec")
                || item
                    .get("tools")
                    .and_then(Value::as_array)
                    .is_some_and(|nested| visit(nested))
        })
    }
    request["input"].as_array().is_some_and(|items| {
        items
            .iter()
            .filter(|item| item["type"] == "additional_tools")
            .any(|item| {
                item.get("tools")
                    .and_then(Value::as_array)
                    .is_some_and(|tools| visit(tools))
            })
    })
}

/// Headers (`name: value` pairs, original spelling) and raw body of one
/// accepted fixture request.
type RawRequest = (Vec<(String, String)>, Vec<u8>);

/// One accepted request: the scripted Responses POST, or the model-catalogue
/// probe a frontend issues before its first turn (`None`).
fn read_request(stream: &mut TcpStream) -> Option<RawRequest> {
    let mut bytes = Vec::new();
    let (start, length, headers) = loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
            let request_line = header.lines().next().unwrap_or_default();
            if request_line.starts_with("GET ") {
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .split('?')
                    .next()
                    .unwrap_or_default();
                assert!(
                    path.ends_with("/models"),
                    "unexpected fixture endpoint: {request_line}"
                );
                return None;
            }
            assert!(
                header.starts_with("POST /v1/responses "),
                "unexpected fixture endpoint: {request_line}"
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
            let headers = header
                .lines()
                .skip(1)
                .map(|line| {
                    let (key, value) = line.split_once(':').expect("valid native HTTP header");
                    (key.to_owned(), value.trim().to_owned())
                })
                .collect();
            break (end + 4, length, headers);
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
    Some((headers, bytes[start..start + length].to_vec()))
}
