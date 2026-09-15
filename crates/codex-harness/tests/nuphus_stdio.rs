//! Model-free native Nuphus stdio: catalogue cache, schema repair, screenshot
//! bounding and snapshot-reference isolation against the probe fixture.
#![cfg(windows)]

use harness_core::{
    build_identity,
    cancellable_pipe::anonymous_pipe,
    nuphus_stdio,
    process::{Cancellation, Deadline},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"))
}

fn write_policy(root: &Path, idle: u64) {
    fs::create_dir_all(root.join("global")).unwrap();
    fs::write(
        root.join("global/tool-resources.json"),
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "nuphus": {"idle_seconds": idle}
        }))
        .unwrap(),
    )
    .unwrap();
}

fn read_reply(reader: &mut BufReader<std::fs::File>) -> Value {
    let mut line = String::new();
    let count = reader.read_line(&mut line).unwrap();
    assert!(count > 0, "nuphus proxy closed early");
    serde_json::from_str(line.trim()).unwrap()
}

fn write_request(
    writer: &mut std::io::BufWriter<std::fs::File>,
    id: u64,
    method: &str,
    params: Value,
) {
    writeln!(
        writer,
        "{}",
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
    )
    .unwrap();
    writer.flush().unwrap();
}

#[test]
fn native_nuphus_stdio_repairs_schema_bounds_screenshots_and_expires_refs() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    write_policy(&source, 60);
    let home = root.path().join("home");
    let account = root.path().join("account");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&account).unwrap();
    let digest = build_identity::hash_file(&fixture()).unwrap();
    let (server_input, client_write) = anonymous_pipe(4096).unwrap();
    let (client_read, server_output) = anonymous_pipe(4096).unwrap();
    let configuration = nuphus_stdio::Configuration {
        executable: fixture(),
        expected_digest: digest,
        codex_home: home.clone(),
        account: account.clone(),
        source_root: source,
        expected_server: Some("nuphus-mcp".into()),
        idle: Some(Duration::from_secs(60)),
        worker_args: vec!["--nuphus-session".into()],
        browser_cdp_url: Some("http://127.0.0.1:9".into()),
    };
    let server = thread::spawn(move || {
        nuphus_stdio::serve(
            configuration,
            server_input,
            server_output,
            &Cancellation::default(),
            Deadline::after(Duration::from_secs(60)).unwrap(),
        )
    });
    let mut writer = std::io::BufWriter::new(client_write);
    let mut reader = BufReader::new(client_read);
    write_request(
        &mut writer,
        1,
        "initialize",
        json!({
            "protocolVersion":"2024-11-05",
            "capabilities":{},
            "clientInfo":{"name":"nuphus-test","version":"0.1.0"}
        }),
    );
    let reply = read_reply(&mut reader);
    assert_eq!(reply["result"]["serverInfo"]["name"], "harness-nuphus");
    writeln!(
        writer,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
    )
    .unwrap();
    writer.flush().unwrap();
    write_request(&mut writer, 2, "tools/list", json!({}));
    let listed = read_reply(&mut reader);
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 38);
    for tool in tools {
        if ["browser_click", "browser_type", "browser_drag_files"]
            .contains(&tool["name"].as_str().unwrap())
        {
            assert_eq!(tool["inputSchema"]["anyOf"][0]["type"], "object");
            assert_eq!(tool["inputSchema"]["anyOf"][1]["type"], "object");
        }
    }
    assert!(account.join("nuphus-catalogue.json").is_file());
    let path = root.path().join("owned.png");
    write_request(
        &mut writer,
        3,
        "tools/call",
        json!({"name":"desktop_window_screenshot","arguments":{"hwnd":1,"path": path.to_str().unwrap()}}),
    );
    let path_result = read_reply(&mut reader);
    assert_ne!(path_result["result"]["isError"], true);
    assert_eq!(path_result["result"]["content"][0]["type"], "text");
    let path_text = path_result["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(path_text.contains(path.to_str().unwrap()));
    assert!(!path_text.contains("iVBORw0KGgo"));
    assert!(path.is_file());
    write_request(
        &mut writer,
        4,
        "tools/call",
        json!({"name":"desktop_window_screenshot","arguments":{"hwnd":1}}),
    );
    let visual = read_reply(&mut reader);
    assert_eq!(visual["result"]["content"][0]["type"], "image");
    assert_eq!(visual["result"]["content"][0]["mimeType"], "image/png");
    assert!(visual["result"]["structuredContent"].is_null());
    write_request(
        &mut writer,
        5,
        "tools/call",
        json!({"name":"browser_snapshot","arguments":{}}),
    );
    let snapshot = read_reply(&mut reader);
    let text = snapshot["result"]["content"][0]["text"].as_str().unwrap();
    let reference = text
        .split_whitespace()
        .find(|token| token.starts_with('@') && token.contains(':'))
        .expect("bound snapshot reference")
        .to_owned();
    write_request(
        &mut writer,
        6,
        "tools/call",
        json!({"name":"browser_click","arguments":{"ref": reference, "confirm": true}}),
    );
    let clicked = read_reply(&mut reader);
    assert!(
        clicked["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("clicked @1")
    );
    write_request(
        &mut writer,
        7,
        "tools/call",
        json!({"name":"browser_navigate","arguments":{"url":"http://127.0.0.1/","confirm":true}}),
    );
    let _ = read_reply(&mut reader);
    write_request(
        &mut writer,
        8,
        "tools/call",
        json!({"name":"browser_click","arguments":{"ref": reference, "confirm": true}}),
    );
    let expired = read_reply(&mut reader);
    assert_eq!(expired["result"]["isError"], true);
    assert!(
        expired["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("fresh browser_snapshot")
    );
    drop(writer);
    server.join().unwrap().unwrap();
}

#[test]
fn native_nuphus_help_does_not_open_a_connection() {
    let root = tempfile::tempdir().unwrap();
    let help = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .current_dir(root.path())
        .args(["mcp", "nuphus", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--expected-digest"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn native_nuphus_rejects_unreviewed_digest_before_serving() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    write_policy(&source, 60);
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["mcp", "nuphus", "--executable"])
        .arg(fixture())
        .arg("--expected-digest")
        .arg("0".repeat(64))
        .arg("--codex-home")
        .arg(root.path().join("home"))
        .arg("--account")
        .arg(root.path().join("account"))
        .arg("--source-root")
        .arg(&source)
        .arg("--connection-seconds")
        .arg("5")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("has not passed the supported package integrity audit")
    );
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
#[ignore = "requires HARNESS_NUPHUS_ORIGINAL for the audited official binary plus an existing Chrome/Edge install"]
fn native_nuphus_owned_browser_on_actual_tool() {
    let original = PathBuf::from(
        std::env::var_os("HARNESS_NUPHUS_ORIGINAL").expect("explicit audited original"),
    );
    let digest = harness_core::nuphus_protocol::AUDITED_ORIGINALS[0].1;
    assert_eq!(
        build_identity::hash_file(&original).unwrap(),
        digest,
        "HARNESS_NUPHUS_ORIGINAL is not the audited 0.2.2 original"
    );
    let root = tempfile::tempdir().unwrap();
    let source = repo();
    if !source.join("global/tool-resources.json").is_file() {
        write_policy(&source, 60);
    }
    let home = root.path().join("home");
    let account = root.path().join("account");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&account).unwrap();
    let (server_input, client_write) = anonymous_pipe(4096).unwrap();
    let (client_read, server_output) = anonymous_pipe(4096).unwrap();
    let configuration = nuphus_stdio::Configuration {
        executable: original,
        expected_digest: digest.to_owned(),
        codex_home: home.clone(),
        account: account.clone(),
        source_root: source,
        expected_server: Some("nuphus-mcp".into()),
        idle: Some(Duration::from_secs(60)),
        worker_args: Vec::new(),
        browser_cdp_url: None,
    };
    let server = thread::spawn(move || {
        nuphus_stdio::serve(
            configuration,
            server_input,
            server_output,
            &Cancellation::default(),
            Deadline::after(Duration::from_secs(180)).unwrap(),
        )
    });
    let mut writer = std::io::BufWriter::new(client_write);
    let mut reader = BufReader::new(client_read);
    write_request(
        &mut writer,
        1,
        "initialize",
        json!({
            "protocolVersion":"2024-11-05",
            "capabilities":{},
            "clientInfo":{"name":"nuphus-owned","version":"0.1.0"}
        }),
    );
    let reply = read_reply(&mut reader);
    assert_eq!(reply["result"]["serverInfo"]["name"], "harness-nuphus");
    writeln!(
        writer,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
    )
    .unwrap();
    writer.flush().unwrap();
    write_request(&mut writer, 2, "tools/list", json!({}));
    let listed = read_reply(&mut reader);
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert!(tools.len() >= 38, "{}", tools.len());
    for tool in tools {
        if ["browser_click", "browser_type", "browser_drag_files"]
            .contains(&tool["name"].as_str().unwrap_or_default())
        {
            assert_eq!(tool["inputSchema"]["anyOf"][0]["type"], "object");
            assert_eq!(tool["inputSchema"]["anyOf"][1]["type"], "object");
        }
    }
    let profiles = home.join("harness/runtime/nuphus");
    assert!(
        !profiles.exists()
            || !fs::read_dir(&profiles)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry.path().is_dir()),
        "catalogue must not start an owned browser profile"
    );
    let window_script = repo().join("tests/fixtures/code-tools-native/nuphus-window.ps1");
    let state = root.path().join("window.json");
    let stop = root.path().join("window.stop");
    let mut window = Command::new("pwsh")
        .args(["-NoLogo", "-NoProfile", "-File"])
        .arg(&window_script)
        .arg("-StatePath")
        .arg(&state)
        .arg("-StopPath")
        .arg(&stop)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if state.is_file() {
            break;
        }
        if window.try_wait().unwrap().is_some() {
            panic!("owned window exited before publishing state");
        }
        thread::sleep(Duration::from_millis(50));
    }
    let metadata: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    let hwnd = metadata["hwnd"].clone();
    write_request(
        &mut writer,
        10,
        "tools/call",
        json!({"name":"desktop_window_info","arguments":{"hwnd": hwnd}}),
    );
    let info = read_reply(&mut reader);
    let info_text = info["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        info_text.contains("Harness owned Nuphus fixture"),
        "{info_text}"
    );
    let path_shot = root.path().join("owned-window.png");
    write_request(
        &mut writer,
        11,
        "tools/call",
        json!({"name":"desktop_window_screenshot","arguments":{"hwnd": hwnd, "path": path_shot.to_str().unwrap()}}),
    );
    let path_result = read_reply(&mut reader);
    assert_ne!(path_result["result"]["isError"], true, "{path_result}");
    assert_eq!(path_result["result"]["content"][0]["type"], "text");
    assert!(path_shot.is_file() && path_shot.metadata().unwrap().len() > 100);
    write_request(
        &mut writer,
        12,
        "tools/call",
        json!({"name":"desktop_window_screenshot","arguments":{"hwnd": hwnd}}),
    );
    let visual = read_reply(&mut reader);
    assert_eq!(visual["result"]["content"][0]["type"], "image", "{visual}");
    fs::write(&stop, b"stop").unwrap();
    let _ = window.wait();
    let http = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = http.local_addr().unwrap().port();
    let page = thread::spawn(move || {
        http.set_nonblocking(true).unwrap();
        let body = b"<!doctype html><title>Harness owned page</title><h1>HARNESS OWNED PAGE</h1><button id=apply>Apply</button>";
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(60) {
            match http.accept() {
                Ok((mut stream, _)) => {
                    use std::io::Write as _;
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(body);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    write_request(
        &mut writer,
        3,
        "tools/call",
        json!({"name":"browser_navigate","arguments":{"url": format!("http://127.0.0.1:{port}/"), "confirm": true}}),
    );
    let navigated = read_reply(&mut reader);
    assert_ne!(navigated["result"]["isError"], true, "{navigated}");
    write_request(
        &mut writer,
        4,
        "tools/call",
        json!({"name":"browser_snapshot","arguments":{}}),
    );
    let snapshot = read_reply(&mut reader);
    let text = snapshot["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("HARNESS OWNED PAGE") || text.contains("Apply"),
        "{text}"
    );
    assert!(text.contains('@') && text.contains(':'), "{text}");
    write_request(
        &mut writer,
        5,
        "tools/call",
        json!({"name":"browser_close","arguments":{"confirm": true}}),
    );
    let _ = read_reply(&mut reader);
    drop(writer);
    server.join().unwrap().unwrap();
    let _ = page.join();
    if profiles.exists() {
        let leftover: Vec<_> = fs::read_dir(&profiles)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .collect();
        assert!(
            leftover.is_empty(),
            "owned browser profiles remain: {leftover:?}"
        );
    }
}
