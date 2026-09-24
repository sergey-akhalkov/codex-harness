//! Owned Responses fixture for instruction-refresh succession acceptance.
//! Serves canned events only; never contacts a model or subscription.
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
    time::Duration,
};

pub const SEED_FINAL: &str = "SUCCESSION_SEED_DONE";
pub const CONTINUATION_FINAL: &str = "SUCCESSION_CONTINUATION_CONSUMED";
pub const CONTINUATION_MARKER: &str = "SUCCESSION_CONTINUATION";

pub struct Responses {
    pub port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Responses {
    pub fn start(evidence: PathBuf) -> Self {
        let cache_loss = evidence.join("cache-loss").is_file();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let mut sequence = 0usize;
            let mut pending_read: Option<String> = None;
            while !stopping.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(error) => panic!("succession fixture accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let (_headers, body) = read_request(&mut stream);
                let request: Value = serde_json::from_slice(&body).unwrap();
                sequence += 1;
                fs::write(evidence.join(format!("provider-{sequence}.json")), &body).unwrap();
                if sequence == 1 && evidence.join("hold-seed").is_file() {
                    // Keep the seed turn in flight so a succession attempt has
                    // to defer until the predecessor settles.
                    let until = std::time::Instant::now() + Duration::from_secs(60);
                    while !stopping.load(Ordering::Relaxed)
                        && !evidence.join("release-seed").is_file()
                        && std::time::Instant::now() < until
                    {
                        thread::sleep(Duration::from_millis(50));
                    }
                }
                let continuation = request["input"].to_string().contains(CONTINUATION_MARKER);
                let output_for = |call: &str| {
                    request["input"].as_array().unwrap().iter().any(|item| {
                        matches!(
                            item["type"].as_str(),
                            Some("function_call_output" | "custom_tool_call_output")
                        ) && item["call_id"] == call
                    })
                };
                let item = if cache_loss {
                    let shell = std::env::var("HARNESS_ACCEPTANCE_POWERSHELL")
                        .expect("owner PowerShell 7 for cache-loss fixture");
                    let arguments = serde_json::to_string(&json!({"cmd":"Write-Output cache-fixture; Start-Sleep -Milliseconds 200", "shell":shell,"yield_time_ms":1000})).unwrap();
                    let call = format!("cache-tool-{sequence}");
                    if request["tools"].as_array().is_some_and(|tools| {
                        tools
                            .iter()
                            .any(|tool| tool["type"] == "custom" && tool["name"] == "exec")
                    }) {
                        json!({"type":"custom_tool_call","call_id":call,"name":"exec","input":format!("text(await tools.exec_command({arguments}));")})
                    } else {
                        json!({"type":"function_call","call_id":call,"name":"exec_command","arguments":arguments})
                    }
                } else if sequence == 1 {
                    assert!(
                        !continuation,
                        "the seed turn must not be a succession continuation"
                    );
                    json!({"type":"message","id":"msg-seed","role":"assistant",
                        "content":[{"type":"output_text","text":SEED_FINAL}]})
                } else {
                    assert!(
                        continuation,
                        "every later turn must be the succession continuation"
                    );
                    assert!(
                        request["input"]
                            .to_string()
                            .contains("never blindly replay"),
                        "the continuation must carry the reconcile instruction"
                    );
                    if pending_read.as_deref().is_some_and(output_for) {
                        let call = pending_read.take().unwrap();
                        assert!(
                            request["input"].to_string().contains("one"),
                            "the continuation must observe the preserved partial work"
                        );
                        assert!(
                            request["input"].to_string().contains(&call),
                            "the observed output must belong to this continuation"
                        );
                        json!({"type":"message","id":"msg-successor","role":"assistant",
                            "content":[{"type":"output_text","text":CONTINUATION_FINAL}]})
                    } else if pending_read.is_none() {
                        let call = format!("succession-read-{sequence}");
                        let arguments = serde_json::to_string(&json!({
                            "cmd":"Get-Content proof.txt","shell":"powershell","yield_time_ms":10000
                        }))
                        .unwrap();
                        let call_item = if request["tools"].as_array().is_some_and(|tools| {
                            tools
                                .iter()
                                .any(|tool| tool["type"] == "custom" && tool["name"] == "exec")
                        }) {
                            json!({"type":"custom_tool_call","call_id":call,"name":"exec",
                                "input":format!("const result = await tools.exec_command({arguments}); text(result);")})
                        } else {
                            json!({"type":"function_call","call_id":call,
                                "name":"exec_command","arguments":arguments})
                        };
                        pending_read = Some(format!("succession-read-{sequence}"));
                        call_item
                    } else {
                        panic!("unexpected request while a continuation read is pending")
                    }
                };
                let usage = if cache_loss {
                    json!({"input_tokens":200_000,"input_tokens_details":{"cached_tokens":if sequence == 1 {199_000} else {4_000}},"output_tokens":10,"total_tokens":200_010})
                } else {
                    json!({"input_tokens":10,"output_tokens":10,"total_tokens":20})
                };
                let events = [
                    json!({"type":"response.created","response":{"id":format!("resp-{sequence}")}}),
                    json!({"type":"response.output_item.done","item":item}),
                    json!({"type":"response.completed","response":{"id":format!("resp-{sequence}"),
                        "usage":usage}}),
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
}

impl Drop for Responses {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
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
            assert!(length <= 4 * 1024 * 1024);
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
