//! Owned loopback ingress for one native provider route. Admission and forwarding
//! use the task's observed native identity; a listener address is not authority.
use crate::{
    broker_endpoint::random_key,
    broker_state::BrokerRoot,
    process::{Cancellation, Deadline},
    task_admission::Gate,
    task_forward::{ForwardRequest, Forwarder},
};
use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct Gateway {
    base_url: String,
    cancel: Cancellation,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl Gateway {
    pub fn start(root: &BrokerRoot, executable: &Path, upstream: &str) -> io::Result<Self> {
        let uri: tungstenite::http::Uri = upstream.parse().map_err(|_| invalid())?;
        if uri.authority().is_none_or(|a| a.as_str().contains('@'))
            || !matches!(uri.scheme_str(), Some("https") | Some("http"))
            || (uri.scheme_str() == Some("http") && uri.host() != Some("127.0.0.1"))
            || uri.query().is_some()
            || upstream.contains('#')
        {
            return Err(invalid());
        }
        // Validate the installed transport before advertising a local route.
        Forwarder::new()?;
        Gate::new(executable)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let route = format!("/{}/v1", random_key()?);
        let base_url = format!("http://127.0.0.1:{}{route}", listener.local_addr()?.port());
        let target = format!("{route}/responses");
        let url = format!("{}/responses", upstream.trim_end_matches('/'));
        let path = root.path().to_owned();
        let executable = executable.to_owned();
        let cancel = Cancellation::default();
        let stopping = cancel.clone();
        let worker = thread::spawn(move || {
            let mut workers: Vec<JoinHandle<io::Result<()>>> = Vec::new();
            let mut sequence = 0u64;
            let result = (|| {
                while !stopping.is_cancelled() {
                    let mut index = 0;
                    while index < workers.len() {
                        if workers[index].is_finished() {
                            workers
                                .swap_remove(index)
                                .join()
                                .map_err(|_| io::Error::other("task ingress worker failed"))??;
                        } else {
                            index += 1;
                        }
                    }
                    let (mut socket, _) = match listener.accept() {
                        Ok(pair) => pair,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    socket.set_nonblocking(false)?;
                    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
                    socket.set_write_timeout(Some(Duration::from_millis(250)))?;
                    if workers.len() >= 16 {
                        let _ = socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        continue;
                    }
                    let (path, executable, target, url, cancel) = (
                        path.clone(),
                        executable.clone(),
                        target.clone(),
                        url.clone(),
                        stopping.clone(),
                    );
                    sequence += 1;
                    let exchange = sequence;
                    workers.push(thread::spawn(move || {
                        let mut sent = false;
                        let mut stage = "receive";
                        let result = (|| -> io::Result<()> {
                            let request = receive(&mut socket, &target, &cancel)?;
                            stage = "private-root";
                            let root = BrokerRoot::open(&path)?;
                            stage = "admission-and-forward";
                            Gate::new(&executable)?.forward(
                                &Forwarder::new()?, &root,
                                ForwardRequest { url: &url, headers: &request.headers, body: &request.body },
                                Deadline::after(Duration::from_secs(24 * 60 * 60))?, &cancel,
                                |bytes| { sent = true; socket.write_all(bytes) },
                            )
                        })();
                        if result.is_err() && !sent && !cancel.is_cancelled() {
                            // No provider quota claim, secret-bearing error or automatic replay.
                            let _ = socket.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                        crate::task_runtime::save(
                            &path.join(format!("gateway-exchange-{exchange}.json")),
                            &serde_json::json!({"schema":1,"completed":result.is_ok(),"responseStarted":sent,
                                "errorKind":result.as_ref().err().map(|error| format!("{:?}", error.kind())),
                                "errorStage":result.as_ref().err().map(|_| stage),
                                "errorDetail":result.as_ref().err().map(|error| error.to_string().chars().take(512).collect::<String>())}),
                        )
                    }));
                }
                Ok(())
            })();
            stopping.cancel();
            let mut cleanup = Ok(());
            for worker in workers {
                let joined = worker
                    .join()
                    .map_err(|_| io::Error::other("task ingress worker failed"))
                    .and_then(|result| result);
                if cleanup.is_ok() {
                    cleanup = joined;
                }
            }
            result.and(cleanup)
        });
        Ok(Self {
            base_url,
            cancel,
            worker: Some(worker),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn finish(&mut self) -> io::Result<()> {
        self.cancel.cancel();
        match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| io::Error::other("task ingress failed"))?,
            None => Ok(()),
        }
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

struct Request {
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn receive(socket: &mut TcpStream, target: &str, cancel: &Cancellation) -> io::Result<Request> {
    let deadline = Deadline::after(Duration::from_secs(30))?;
    let mut bytes = Vec::new();
    let (start, length, headers) = loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            if end > 64 * 1024 {
                return Err(invalid());
            }
            let text = std::str::from_utf8(&bytes[..end]).map_err(|_| invalid())?;
            let mut lines = text.split("\r\n");
            if lines.next() != Some(format!("POST {target} HTTP/1.1").as_str()) {
                return Err(invalid());
            }
            let mut length = None;
            let mut headers = Vec::new();
            for line in lines {
                let (name, value) = line.split_once(':').ok_or_else(invalid)?;
                if name.eq_ignore_ascii_case("transfer-encoding") {
                    return Err(invalid());
                }
                if name.eq_ignore_ascii_case("content-length") {
                    if length.is_some() || !value.trim().bytes().all(|b| b.is_ascii_digit()) {
                        return Err(invalid());
                    }
                    length = Some(value.trim().parse::<usize>().map_err(|_| invalid())?);
                }
                headers.push((name.to_owned(), value.trim().to_owned()));
            }
            let length = length.ok_or_else(invalid)?;
            if length > 64 * 1024 * 1024 {
                return Err(invalid());
            }
            break (end + 4, length, headers);
        }
        if bytes.len() > 64 * 1024 {
            return Err(invalid());
        }
        read_more(socket, &mut bytes, deadline, cancel)?;
    };
    while bytes.len() < start + length {
        read_more(socket, &mut bytes, deadline, cancel)?;
    }
    if bytes.len() != start + length {
        return Err(invalid());
    }
    Ok(Request {
        headers,
        body: bytes.split_off(start),
    })
}

fn read_more(
    socket: &mut TcpStream,
    bytes: &mut Vec<u8>,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<()> {
    loop {
        if cancel.is_cancelled() || deadline.expired() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let mut chunk = [0; 4096];
        match socket.read(&mut chunk) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => {
                bytes.extend_from_slice(&chunk[..count]);
                return Ok(());
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "unsupported task ingress request",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_or_unowned_requests_never_reach_upstream_and_shutdown_joins_reads() {
        let owned = BrokerRoot::prepare().unwrap();
        let upstream = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        upstream.set_nonblocking(true).unwrap();
        let mut gateway = Gateway::start(
            owned.root(),
            &std::env::current_exe().unwrap(),
            &format!(
                "http://127.0.0.1:{}/v1",
                upstream.local_addr().unwrap().port()
            ),
        )
        .unwrap();
        let uri: tungstenite::http::Uri = gateway.base_url().parse().unwrap();
        for headers in [
            "Content-Length: 2\r\nContent-Length: 2",
            "Transfer-Encoding: chunked",
            "Content-Length: 2",
        ] {
            let mut socket = TcpStream::connect(uri.authority().unwrap().as_str()).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(
                socket,
                "POST {}/responses HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer owned-secret-not-for-receipts\r\n{headers}\r\n\r\n{{}}",
                uri.path()
            )
            .unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 502"));
        }
        let mut partial = TcpStream::connect(uri.authority().unwrap().as_str()).unwrap();
        partial.write_all(b"POST ").unwrap();
        thread::sleep(Duration::from_millis(100));
        let started = std::time::Instant::now();
        gateway.finish().unwrap();
        for sequence in 1..=3 {
            let receipt = std::fs::read_to_string(
                owned
                    .root()
                    .path()
                    .join(format!("gateway-exchange-{sequence}.json")),
            )
            .unwrap();
            assert!(!receipt.contains("owned-secret-not-for-receipts"));
            assert!(!receipt.contains("Authorization"));
            let receipt: serde_json::Value = serde_json::from_str(&receipt).unwrap();
            assert!(receipt["errorStage"].as_str().is_some());
            assert!(
                receipt["errorDetail"]
                    .as_str()
                    .is_some_and(|detail| !detail.is_empty())
            );
        }
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            matches!(upstream.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
        assert!(TcpStream::connect(uri.authority().unwrap().as_str()).is_err());
    }
}
