//! One owned streaming HTTP exchange for the pending task visibility gate.
//! The caller owns admission and the exact configured upstream. This transport
//! never chooses a model, injects credentials, follows redirects or retries.
use crate::{
    broker_state::BrokerRoot,
    cancellable_pipe::{CancellablePipe, PipeIoError, anonymous_pipe},
    dependency_fetch,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason},
};
use std::{
    io::{self, Write},
    time::Duration,
};

pub struct Forwarder {
    client: dependency_fetch::Client,
}

impl Forwarder {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            client: dependency_fetch::Client::new().map_err(io::Error::other)?,
        })
    }

    /// Forward an already-admitted request. Response bytes include HTTP/1.1
    /// headers and original transfer framing; the consuming listener relays them.
    pub fn forward(
        &self,
        root: &BrokerRoot,
        request: ForwardRequest<'_>,
        deadline: Deadline,
        cancel: &Cancellation,
        mut output: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        let uri: tungstenite::http::Uri = request.url.parse().map_err(|_| invalid())?;
        if uri.authority().is_none_or(|a| a.as_str().contains('@'))
            || !matches!(uri.scheme_str(), Some("https") | Some("http"))
            || (uri.scheme_str() == Some("http") && uri.host() != Some("127.0.0.1"))
            || request.url.contains('#')
            || request.body.len() > 64 * 1024 * 1024
        {
            return Err(invalid());
        }
        if cancel.is_cancelled() || deadline.expired() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "forwarding cancelled before dispatch",
            ));
        }
        let mut body = tempfile::NamedTempFile::new_in(root.path())?;
        body.write_all(request.body)?;
        body.flush()?;
        let mut config = format!(
            "url = {}\ndata-binary = {}\n",
            quoted(request.url)?,
            quoted(&format!("@{}", body.path().to_str().ok_or_else(invalid)?))?
        );
        for (name, value) in request.headers {
            if !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
                || name.is_empty()
            {
                return Err(invalid());
            }
            if [
                "host",
                "content-length",
                "transfer-encoding",
                "connection",
                "expect",
                "proxy-authorization",
                "proxy-connection",
                "keep-alive",
                "te",
                "trailer",
                "upgrade",
            ]
            .iter()
            .any(|excluded| name.eq_ignore_ascii_case(excluded))
            {
                continue;
            }
            config.push_str(&format!(
                "header = {}\n",
                quoted(&format!("{name}: {value}"))?
            ));
        }
        config.push_str("header = \"Connection: close\"\nheader = \"Expect:\"\n");
        for name in ["Content-Type", "Accept", "User-Agent"] {
            if !request
                .headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name))
            {
                config.push_str(&format!("header = \"{name}:\"\n"));
            }
        }
        if config.len() > 64 * 1024 {
            return Err(invalid());
        }
        let (stdin, writer) = anonymous_pipe(65536).map_err(io::Error::other)?;
        let (reader, stdout) = anonymous_pipe(65536).map_err(io::Error::other)?;
        let mut command = CommandSpec::new(self.client.executable());
        command.current_dir = Some(root.path().into());
        // Secrets are sent through the private pipe, never arguments or logs.
        command.args = [
            "--disable",
            "--silent",
            "--globoff",
            "--http1.1",
            "--include",
            "--suppress-connect-headers",
            "--raw",
            "--no-buffer",
            "--request",
            "POST",
            "--proto",
            "=http,https",
            "--max-redirs",
            "0",
            "--retry",
            "0",
            "--config",
            "-",
        ]
        .map(Into::into)
        .to_vec();
        for key in ["SSLKEYLOGFILE", "QLOGDIR"] {
            command.env.insert(key.into(), None);
        }
        command.stdin = Some(stdin);
        command.stdout = Some(stdout);
        let job = Job::new(Limits::default())?;
        let process = job.spawn(&command)?;
        drop(command);
        let transfer = (|| -> io::Result<()> {
            let mut writer =
                CancellablePipe::writer(writer, cancel.clone()).map_err(io::Error::other)?;
            writer
                .write_all(config.as_bytes(), deadline, cancel)
                .map_err(io::Error::other)?;
            writer.close(deadline).map_err(io::Error::other)?;
            let mut reader =
                CancellablePipe::reader(reader, cancel.clone()).map_err(io::Error::other)?;
            loop {
                let bytes = match reader.read(4096, deadline, cancel) {
                    Ok(bytes) => bytes,
                    Err(PipeIoError::EndOfFile) => break,
                    Err(error) => return Err(io::Error::other(error)),
                };
                if bytes.is_empty() {
                    break;
                }
                output(&bytes)?;
            }
            reader.close(deadline).map_err(io::Error::other)?;
            Ok(())
        })();
        if let Err(error) = transfer {
            job.terminate(1, Duration::from_secs(2))
                .map_err(|cleanup| {
                    io::Error::other(format!(
                        "{error}; owned transport cleanup failed: {cleanup}"
                    ))
                })?;
            return Err(error);
        }
        let outcome = job.wait(&process, deadline, cancel, Duration::from_secs(2))?;
        if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
            return Err(io::Error::other(format!(
                "upstream transport ended: {:?}, curl exit {}",
                outcome.reason, outcome.exit_code
            )));
        }
        Ok(())
    }
}

pub struct ForwardRequest<'a> {
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid task upstream request")
}

fn quoted(value: &str) -> io::Result<String> {
    if value.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, net::TcpListener, sync::mpsc, thread};

    #[test]
    fn system_curl_preserves_auth_body_streaming_and_provider_error() {
        let client = Forwarder::new().unwrap();
        let owned = BrokerRoot::prepare().unwrap();
        for (status, chunked) in [
            ("200 OK", false),
            ("429 Too Many Requests", false),
            ("302 Found", false),
            ("200 OK", true),
        ] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let url = format!(
                "http://127.0.0.1:{}/responses",
                listener.local_addr().unwrap().port()
            );
            let (release, wait) = mpsc::channel();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                    assert!(request.len() < 65536);
                }
                let header = String::from_utf8(request).unwrap();
                assert!(header.contains("Authorization: Bearer owned-test-token\r\n"));
                let mut body = [0; 2];
                stream.read_exact(&mut body).unwrap();
                assert_eq!(&body, b"{}");
                let location = if status.starts_with("302") {
                    format!(
                        "Location: http://127.0.0.1:{}/must-not-follow\r\n",
                        listener.local_addr().unwrap().port()
                    )
                } else {
                    String::new()
                };
                let framing = if chunked {
                    "Transfer-Encoding: chunked\r\nTrailer: X-Result\r\n"
                } else {
                    "Content-Length: 12\r\n"
                };
                let first = if chunked { "6\r\nfirst-\r\n" } else { "first-" };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\n{location}{framing}Connection: close\r\n\r\n{first}"
                )
                .unwrap();
                stream.flush().unwrap();
                wait.recv_timeout(Duration::from_secs(10))
                    .expect("response must stream before completion");
                stream
                    .write_all(if chunked {
                        b"6\r\nsecond\r\n0\r\nX-Result: consumed\r\n\r\n"
                    } else {
                        b"second"
                    })
                    .unwrap();
            });
            let mut received = Vec::new();
            let mut released = false;
            client
                .forward(
                    owned.root(),
                    ForwardRequest {
                        url: &url,
                        headers: &[("Authorization".into(), "Bearer owned-test-token".into())],
                        body: b"{}",
                    },
                    Deadline::after(Duration::from_secs(15)).unwrap(),
                    &Cancellation::default(),
                    |bytes| {
                        received.extend_from_slice(bytes);
                        let first = if chunked {
                            b"6\r\nfirst-\r\n".as_slice()
                        } else {
                            b"first-".as_slice()
                        };
                        if !released && received.ends_with(first) {
                            release.send(()).unwrap();
                            released = true;
                        }
                        Ok(())
                    },
                )
                .unwrap();
            server.join().unwrap();
            assert!(received.starts_with(format!("HTTP/1.1 {status}\r\n").as_bytes()));
            let expected = if chunked {
                b"6\r\nfirst-\r\n6\r\nsecond\r\n0\r\nX-Result: consumed\r\n\r\n".as_slice()
            } else {
                b"first-second".as_slice()
            };
            assert!(received.ends_with(expected));
        }
    }

    #[test]
    fn cancellation_closes_the_owned_upstream_stream() {
        let client = Forwarder::new().unwrap();
        let owned = BrokerRoot::prepare().unwrap();
        let files = || {
            std::fs::read_dir(owned.root().path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<std::collections::BTreeSet<_>>()
        };
        let before = files();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!(
            "http://127.0.0.1:{}/responses",
            listener.local_addr().unwrap().port()
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 65536);
            }
            let mut body = [0; 2];
            stream.read_exact(&mut body).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npending")
                .unwrap();
            let mut byte = [0];
            match stream.read(&mut byte) {
                Ok(0) => (),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                    ) => {}
                outcome => panic!("cancelled transport must close its upstream: {outcome:?}"),
            }
        });
        let cancel = Cancellation::default();
        let result = client.forward(
            owned.root(),
            ForwardRequest {
                url: &url,
                headers: &[],
                body: b"{}",
            },
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &cancel,
            |_| {
                cancel.cancel();
                Ok(())
            },
        );
        assert!(result.is_err());
        server.join().unwrap();
        assert_eq!(
            files(),
            before,
            "temporary body must be removed and broker ownership files preserved"
        );
    }

    #[test]
    fn config_cannot_inject_options_or_expose_credentials_in_errors() {
        assert_eq!(quoted("a\\b\"c").unwrap(), "\"a\\\\b\\\"c\"");
        let error = quoted("owned-secret\nurl = another").unwrap_err();
        assert!(!error.to_string().contains("owned-secret"));
    }
}
