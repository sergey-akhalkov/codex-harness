//! Isolated OpenCodex stand-in for native subscription host tests.
#![cfg(windows)]

use std::{
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener},
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("start")
        || args.get(1).map(String::as_str) != Some("--port")
    {
        eprintln!("invalid fixture arguments");
        std::process::exit(2);
    }
    let port: u16 = args
        .get(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    if port == 0 {
        std::process::exit(2);
    }
    let fail = std::env::var_os("HARNESS_SUBSCRIPTION_FIXTURE_FAIL").is_some();
    let linger = std::env::var("HARNESS_SUBSCRIPTION_FIXTURE_LINGER")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(8);
    if fail {
        std::process::exit(7);
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("bind");
    listener.set_nonblocking(true).unwrap();
    let pid = std::process::id();
    if let Ok(home) = std::env::var("OPENCODEX_HOME") {
        let path = PathBuf::from(home);
        let _ = std::fs::create_dir_all(&path);
        let body = serde_json::json!({"pid": pid, "port": port});
        let _ = std::fs::write(
            path.join("runtime-port.json"),
            serde_json::to_vec(&body).unwrap(),
        );
    }
    let until = Instant::now() + Duration::from_secs(linger);
    while Instant::now() < until {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0u8; 512];
            let _ = stream.read(&mut buffer);
            let body =
                serde_json::json!({"status":"ready","service":"opencodex","pid":pid,"port":port});
            let payload = serde_json::to_vec(&body).unwrap();
            let header = format!(
                "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&payload);
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
