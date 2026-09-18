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
pub const PARENT_FINAL: &str = "CONTROL_PARENT_RESULTS_CONSUMED";

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
                let (headers, request_body) = read_request(&mut stream);
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
                        assert_successor_visible(&evidence, &identity.thread);
                    }
                }
                assert!(
                    sequence <= if pair { 9 } else { 5 },
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
                let second_worker = second_worker || pair && request["model"] == "xai/grok-4.6";
                let worker = second_worker
                    || pair && request["model"] == "zai/glm-5.3"
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
                                part["text"].as_str().is_some_and(|text| {
                                    text.contains("CONTROL_HELPER_ASSIGNMENT")
                                })
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
                    assert_eq!(
                        request["model"],
                        if second_worker {
                            "xai/grok-4.6"
                        } else {
                            "zai/glm-5.3"
                        }
                    );
                    assert_eq!(request["reasoning"]["effort"], "high");
                    let assignment = if second_worker {
                        "run the second owned proof command"
                    } else {
                        "run the owned proof command"
                    };
                    assert!(
                        request["input"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|item| item["role"] == "user"
                                && item["content"].as_array().is_some_and(|parts| parts
                                    .iter()
                                    .any(|part| part["text"]
                                        .as_str()
                                        .is_some_and(|text| text.contains(assignment))))),
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
                let recovering = request["input"]
                    .to_string()
                    .contains("Continue only the previously authorized task");
                let item = if recovering {
                    let read_done = request["input"].as_array().unwrap().iter().any(|item| {
                        item["type"] == "function_call_output"
                            && item["call_id"] == "control-read-1"
                    });
                    if !read_done {
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
                    let mut assignment = json!({"task_name":"control_worker","message":"CONTROL_CHILD_ASSIGNMENT: run the owned proof command, then consume and return its result. Do not spawn other agents."});
                    if pair {
                        assignment.as_object_mut().unwrap().remove("task_name");
                        assignment["fork_context"] = json!(false);
                        assignment["model"] = json!("zai/glm-5.3");
                        assignment["reasoning_effort"] = json!("high");
                    }
                    let mut call = json!({"type":"function_call","call_id":"control-spawn-1","namespace":"collaboration","name":"spawn_agent","encrypted_function_args":[],"arguments":serde_json::to_string(&assignment).unwrap()});
                    if pair {
                        call["namespace"] = json!("multi_agent_v1");
                    }
                    call
                } else if pair && !worker && !second_spawned {
                    json!({"type":"function_call","call_id":"control-spawn-2","namespace":"multi_agent_v1","name":"spawn_agent","encrypted_function_args":[],"arguments":serde_json::to_string(&json!({"fork_context":false,"model":"xai/grok-4.6","reasoning_effort":"high","message":"CONTROL_CHILD_ASSIGNMENT: run the second owned proof command, then consume and return its result. Do not spawn other agents."})).unwrap()})
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
                    let first = output("control-wait-1");
                    let second = output("control-wait-2");
                    if let Some(first) = first {
                        assert!(
                            first.contains("CONTROL_TOOL_RESULT_CONSUMED_ONE"),
                            "leader must receive the first completed result"
                        );
                    }
                    if let Some(second) = second {
                        assert!(
                            second.contains("CONTROL_TOOL_RESULT_CONSUMED_TWO"),
                            "leader must receive the second completed result"
                        );
                        assert!(first.is_some());
                        json!({"type":"message","id":"msg-parent-final","role":"assistant","content":[{"type":"output_text","text":PARENT_FINAL}]})
                    } else {
                        let number = if first.is_some() { 2 } else { 1 };
                        let spawn: Value = serde_json::from_str(
                            output(&format!("control-spawn-{number}"))
                                .expect("native spawn result must identify the child"),
                        )
                        .unwrap();
                        let id = spawn["agent_id"].as_str().expect("native child id");
                        json!({"type":"function_call","call_id":format!("control-wait-{number}"),"namespace":"multi_agent_v1","name":"wait_agent","arguments":serde_json::to_string(&json!({"targets":[id],"timeout_ms":30000})).unwrap()})
                    }
                } else if !direct && !worker && request["input"].to_string().contains(FINAL) {
                    json!({"type":"message","id":"msg-parent-final","role":"assistant","content":[{"type":"output_text","text":FINAL}]})
                } else if !direct && !worker {
                    json!({"type":"message","id":"msg-parent","role":"assistant","content":[{"type":"output_text","text":"Owned child dispatched."}]})
                } else if helper_case && worker && !helper_spawned {
                    json!({"type":"function_call","call_id":"control-spawn-helper","namespace":"multi_agent_v1","name":"spawn_agent","encrypted_function_args":[],"arguments":serde_json::to_string(&json!({"fork_context":false,"message":"CONTROL_HELPER_ASSIGNMENT: run the owned proof command, then consume and return its result. Do not spawn other agents."})).unwrap()})
                } else if helper_case && worker && !helper_waited {
                    let spawn: Value = serde_json::from_str(
                        request["input"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|item| {
                                item["type"] == "function_call_output"
                                    && item["call_id"] == "control-spawn-helper"
                            })
                            .and_then(|item| item["output"].as_str())
                            .expect("native helper spawn result must identify the child"),
                    )
                    .unwrap();
                    let id = spawn["agent_id"].as_str().expect("native helper id");
                    json!({"type":"function_call","call_id":"control-wait-helper","namespace":"multi_agent_v1","name":"wait_agent","arguments":serde_json::to_string(&json!({"targets":[id],"timeout_ms":30000})).unwrap()})
                } else if helper_case && worker {
                    json!({"type":"message","id":"msg-executor-helper","role":"assistant","content":[{"type":"output_text","text":"Owned helper consumed."}]})
                } else if result_seen {
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
                            else { "[IO.File]::AppendAllText('proof.txt', 'one'); Start-Sleep -Seconds 3; Get-Content proof.txt" },
                        "shell":"powershell","yield_time_ms":if background { 1000 } else if view_loss || close_view || pair { 30000 } else { 10000 }
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
    for value in
        std::iter::once(&initial["window"]).chain(views["threads"].as_object().unwrap().values())
    {
        let snapshot: harness_core::task_view::Snapshot =
            serde_json::from_value(value.clone()).unwrap();
        assert!(
            snapshot.is_visible(&native, &user).unwrap(),
            "all established conversations must be visible at executor dispatch"
        );
    }
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
    assert_eq!(leader["model"], "zai/glm-5.3");
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
        let (headers, body) = read_request(&mut socket);
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

fn read_request(stream: &mut TcpStream) -> (Vec<(String, String)>, Vec<u8>) {
    let mut bytes = Vec::new();
    let (start, length, headers) = loop {
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
    (headers, bytes[start..start + length].to_vec())
}
