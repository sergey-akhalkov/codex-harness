//! Bounded loopback HTTP/1 wire for the private shared resource broker.
//!
//! One POST /rpc exchange on a raw 127.0.0.1 TcpStream. Parent owns listen,
//! accept, request semantics, receipts and worker lifetime. Local framing and
//! authentication errors do not include bearer tokens or raw request bytes.
#![cfg(windows)]

use crate::{
    dependency_mcp_probe::strict_json,
    process::{Cancellation, Deadline},
};
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;
const TOKEN_LEN: usize = 64;
const IO_SLICE: Duration = Duration::from_millis(20);
const BEARER: &[u8] = b"Bearer ";

#[derive(Debug)]
pub enum RequestError {
    Unauthorized,
    Invalid,
    Io(io::Error),
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("broker request unauthorized"),
            Self::Invalid => f.write_str("broker request invalid"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for RequestError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Read one POST /rpc request. Wrong or missing bearer is Unauthorized before
/// any body wait. Parent validates operation, payload and admission.
pub fn read_request(
    stream: &mut TcpStream,
    token: &str,
    deadline: Deadline,
    cancel: &Cancellation,
) -> Result<Value, RequestError> {
    let (head, leftover) = read_header_block(stream, deadline, cancel)?;
    let request = parse_request_head(&head, token)?;
    if !request.authorized {
        return Err(RequestError::Unauthorized);
    }
    let length = request.content_length.ok_or(RequestError::Invalid)?;
    if length == 0 || length > MAX_BODY {
        return Err(RequestError::Invalid);
    }
    if leftover.len() > length {
        return Err(RequestError::Invalid);
    }
    let body = read_body(stream, leftover, length, deadline, cancel)?;
    strict_json(&body).map_err(|_| RequestError::Invalid)
}

/// Write one JSON response with Connection: close and a Content-Length of the
/// UTF-8 byte count. Parent chooses the status and envelope.
pub fn write_response(
    stream: &mut TcpStream,
    status: u16,
    body: &Value,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<()> {
    check_stop(deadline, cancel)?;
    if !(100..600).contains(&status) {
        return Err(invalid_input("broker HTTP status is invalid"));
    }
    let body = serde_json::to_vec(body)?;
    if body.len() > MAX_BODY {
        return Err(invalid_data("broker HTTP body exceeds 16 MiB"));
    }
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    write_all_timed(stream, header.as_bytes(), deadline, cancel)?;
    write_all_timed(stream, &body, deadline, cancel)
}

/// Client round-trip to 127.0.0.1:{port}/rpc. Bypasses proxy environment by
/// connecting with TcpStream. HTTP 200 {result} is success, including a
/// tool-level isError inside result; {error} and non-200 stay errors.
pub fn exchange(
    port: u16,
    token: &str,
    operation: &str,
    payload: &Value,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Value> {
    check_stop(deadline, cancel)?;
    if port == 0 {
        return Err(invalid_input("broker HTTP port is invalid"));
    }
    if !valid_token(token) {
        return Err(invalid_input("broker HTTP token is invalid"));
    }
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
        .as_secs_f64()
        + deadline.remaining().as_secs_f64();
    let envelope = serde_json::json!({
        "operation": operation,
        "payload": payload,
        "deadline": unix,
    });
    let body = serde_json::to_vec(&envelope)?;
    if body.len() > MAX_BODY {
        return Err(invalid_data("broker HTTP body exceeds 16 MiB"));
    }
    let mut stream = connect(port, deadline, cancel)?;
    let _ = stream.set_nodelay(true);
    let header = format!(
        "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    write_all_timed(&mut stream, header.as_bytes(), deadline, cancel)?;
    write_all_timed(&mut stream, &body, deadline, cancel)?;
    let (head, leftover) = match read_header_block(&mut stream, deadline, cancel) {
        Ok(block) => block,
        Err(RequestError::Io(error)) => return Err(error),
        Err(_) => return Err(invalid_data("broker HTTP response is invalid")),
    };
    let response =
        parse_response_head(&head).map_err(|_| invalid_data("broker HTTP response is invalid"))?;
    if response.content_length == 0 || response.content_length > MAX_BODY {
        return Err(invalid_data("broker HTTP body exceeds 16 MiB"));
    }
    if leftover.len() > response.content_length {
        return Err(invalid_data("broker HTTP response is invalid"));
    }
    let raw = match read_body(
        &mut stream,
        leftover,
        response.content_length,
        deadline,
        cancel,
    ) {
        Ok(raw) => raw,
        Err(RequestError::Io(error)) => return Err(error),
        Err(_) => return Err(invalid_data("broker HTTP response is invalid")),
    };
    if response.status != 200 {
        return Err(invalid_data("broker HTTP status is not successful"));
    }
    parse_result(&raw)
}

fn connect(port: u16, deadline: Deadline, cancel: &Cancellation) -> io::Result<TcpStream> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    loop {
        check_stop(deadline, cancel)?;
        match TcpStream::connect_timeout(&addr, slice(deadline)) {
            Ok(stream) => return Ok(stream),
            Err(error) if retry_timeout(&error) => {
                std::thread::sleep(Duration::from_millis(1).min(deadline.remaining()));
                continue;
            }
            Err(error) => return Err(error),
        }
    }
}

fn read_header_block(
    stream: &mut TcpStream,
    deadline: Deadline,
    cancel: &Cancellation,
) -> Result<(Vec<u8>, Vec<u8>), RequestError> {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 256];
    loop {
        check_stop(deadline, cancel)?;
        match header_end(&buf)? {
            Some(end) => {
                let leftover = buf[end + 4..].to_vec();
                buf.truncate(end);
                return Ok((buf, leftover));
            }
            None if buf.len() >= MAX_HEADERS => return Err(RequestError::Invalid),
            None => {}
        }
        let want = chunk.len().min(MAX_HEADERS.saturating_sub(buf.len()));
        if want == 0 {
            return Err(RequestError::Invalid);
        }
        set_timeouts(stream, deadline)?;
        match stream.read(&mut chunk[..want]) {
            Ok(0) => return Err(eof()),
            Ok(count) => buf.extend_from_slice(&chunk[..count]),
            Err(error) if retry_timeout(&error) => continue,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

fn read_body(
    stream: &mut TcpStream,
    leftover: Vec<u8>,
    length: usize,
    deadline: Deadline,
    cancel: &Cancellation,
) -> Result<Vec<u8>, RequestError> {
    if leftover.len() > length {
        return Err(RequestError::Invalid);
    }
    let mut body = leftover;
    body.reserve(length - body.len());
    let mut chunk = [0_u8; 1024];
    while body.len() < length {
        check_stop(deadline, cancel)?;
        set_timeouts(stream, deadline)?;
        let want = chunk.len().min(length - body.len());
        match stream.read(&mut chunk[..want]) {
            Ok(0) => return Err(eof()),
            Ok(count) => body.extend_from_slice(&chunk[..count]),
            Err(error) if retry_timeout(&error) => continue,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(body)
}

fn write_all_timed(
    stream: &mut TcpStream,
    mut data: &[u8],
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<()> {
    while !data.is_empty() {
        check_stop(deadline, cancel)?;
        set_timeouts(stream, deadline)?;
        match stream.write(data) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "broker HTTP write closed",
                ));
            }
            Ok(count) => data = &data[count..],
            Err(error) if retry_timeout(&error) => continue,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    check_stop(deadline, cancel)?;
    set_timeouts(stream, deadline)?;
    stream.flush()
}

struct RequestHead {
    authorized: bool,
    content_length: Option<usize>,
}

struct ResponseHead {
    status: u16,
    content_length: usize,
}

fn parse_request_head(block: &[u8], token: &str) -> Result<RequestHead, RequestError> {
    let (line, fields) = parse_header_block(block)?;
    let mut parts = line.split(' ');
    let method = parts.next();
    let target = parts.next();
    let version = parts.next();
    if parts.next().is_some()
        || !matches!(version, Some("HTTP/1.0" | "HTTP/1.1"))
        || method != Some("POST")
        || target != Some("/rpc")
    {
        return Err(RequestError::Invalid);
    }
    let headers = collect_headers(&fields)?;
    if headers.transfer_encoding || headers.duplicate_length || headers.duplicate_authorization {
        return Err(RequestError::Invalid);
    }
    let expected = bearer_value(token);
    let authorized = headers
        .authorization
        .as_deref()
        .is_some_and(|value| const_eq(value, &expected));
    Ok(RequestHead {
        authorized,
        content_length: headers.content_length,
    })
}

fn parse_response_head(block: &[u8]) -> Result<ResponseHead, RequestError> {
    let (line, fields) = parse_header_block(block)?;
    let mut parts = line.split(' ');
    let version = parts.next();
    let status = parts.next();
    if !matches!(version, Some("HTTP/1.0" | "HTTP/1.1")) {
        return Err(RequestError::Invalid);
    }
    let status = status
        .filter(|code| code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|code| code.parse::<u16>().ok())
        .filter(|code| (100..600).contains(code))
        .ok_or(RequestError::Invalid)?;
    let headers = collect_headers(&fields)?;
    if headers.transfer_encoding || headers.duplicate_length {
        return Err(RequestError::Invalid);
    }
    let content_length = headers.content_length.ok_or(RequestError::Invalid)?;
    Ok(ResponseHead {
        status,
        content_length,
    })
}

struct HeaderFlags {
    content_length: Option<usize>,
    authorization: Option<Vec<u8>>,
    transfer_encoding: bool,
    duplicate_length: bool,
    duplicate_authorization: bool,
}

fn parse_header_block(block: &[u8]) -> Result<(&str, Vec<&str>), RequestError> {
    if block.is_empty() || block.contains(&0) {
        return Err(RequestError::Invalid);
    }
    let text = std::str::from_utf8(block).map_err(|_| RequestError::Invalid)?;
    if text
        .bytes()
        .any(|byte| byte.is_ascii_control() && byte != b'\r' && byte != b'\n')
    {
        return Err(RequestError::Invalid);
    }
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(split) = rest.find("\r\n") {
        lines.push(&rest[..split]);
        rest = &rest[split + 2..];
    }
    if !rest.is_empty() {
        lines.push(rest);
    }
    if lines.is_empty() || lines[0].is_empty() {
        return Err(RequestError::Invalid);
    }
    Ok((lines[0], lines[1..].to_vec()))
}

fn collect_headers(fields: &[&str]) -> Result<HeaderFlags, RequestError> {
    let mut headers = HeaderFlags {
        content_length: None,
        authorization: None,
        transfer_encoding: false,
        duplicate_length: false,
        duplicate_authorization: false,
    };
    for line in fields {
        if line.is_empty() {
            return Err(RequestError::Invalid);
        }
        let first = line.as_bytes()[0];
        if first == b' ' || first == b'\t' {
            return Err(RequestError::Invalid);
        }
        let (name, value) = line.split_once(':').ok_or(RequestError::Invalid)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(RequestError::Invalid);
        }
        let value = value.trim_matches([' ', '\t']);
        match name.to_ascii_lowercase().as_str() {
            "content-length" => {
                if headers.content_length.is_some() {
                    headers.duplicate_length = true;
                } else {
                    headers.content_length = Some(parse_length(value)?);
                }
            }
            "authorization" => {
                if headers.authorization.is_some() {
                    headers.duplicate_authorization = true;
                } else {
                    headers.authorization = Some(value.as_bytes().to_vec());
                }
            }
            "transfer-encoding" => headers.transfer_encoding = true,
            _ => {}
        }
    }
    Ok(headers)
}

fn parse_length(value: &str) -> Result<usize, RequestError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(RequestError::Invalid);
    }
    if value.len() > 1 && value.starts_with('0') {
        return Err(RequestError::Invalid);
    }
    value.parse().map_err(|_| RequestError::Invalid)
}

fn parse_result(raw: &[u8]) -> io::Result<Value> {
    let value = strict_json(raw).map_err(|_| invalid_data("broker HTTP JSON is invalid"))?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid_data("broker HTTP JSON is invalid"))?;
    match (object.get("result"), object.get("error")) {
        (Some(result), None) => Ok(result.clone()),
        (None, Some(error)) => Err(bounded_error(error)),
        _ => Err(invalid_data("broker HTTP JSON is invalid")),
    }
}

fn bounded_error(error: &Value) -> io::Error {
    match error.as_str() {
        Some(message)
            if !message.is_empty()
                && message.len() <= 1024
                && message
                    .bytes()
                    .all(|byte| byte >= 0x20 || byte == b'\t' || byte == b'\n') =>
        {
            io::Error::other(format!("broker error: {message}"))
        }
        _ => io::Error::other("broker error"),
    }
}

fn header_end(buf: &[u8]) -> Result<Option<usize>, RequestError> {
    let mut index = 0;
    while index < buf.len() {
        match buf[index] {
            b'\n' if index == 0 || buf[index - 1] != b'\r' => return Err(RequestError::Invalid),
            b'\r' => {
                if index + 1 == buf.len() {
                    return Ok(None);
                }
                if buf[index + 1] != b'\n' {
                    return Err(RequestError::Invalid);
                }
                if index + 3 < buf.len()
                    && buf[index + 1] == b'\n'
                    && buf[index + 2] == b'\r'
                    && buf[index + 3] == b'\n'
                {
                    return Ok(Some(index));
                }
                index += 2;
            }
            byte if byte.is_ascii_control() && byte != b'\t' => return Err(RequestError::Invalid),
            _ => index += 1,
        }
    }
    Ok(None)
}

fn bearer_value(token: &str) -> Vec<u8> {
    let mut value = Vec::with_capacity(BEARER.len() + token.len());
    value.extend_from_slice(BEARER);
    value.extend_from_slice(token.as_bytes());
    value
}

fn valid_token(token: &str) -> bool {
    token.len() == TOKEN_LEN && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn const_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn check_stop(deadline: Deadline, cancel: &Cancellation) -> io::Result<()> {
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "broker HTTP cancelled",
        ));
    }
    if deadline.expired() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "broker HTTP deadline expired",
        ));
    }
    Ok(())
}

fn set_timeouts(stream: &mut TcpStream, deadline: Deadline) -> io::Result<()> {
    let timeout = Some(slice(deadline));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)
}

fn slice(deadline: Deadline) -> Duration {
    let remaining = deadline.remaining();
    if remaining.is_zero() {
        Duration::from_millis(1)
    } else if remaining < IO_SLICE {
        remaining
    } else {
        IO_SLICE
    }
}

fn retry_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn eof() -> RequestError {
    RequestError::Io(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "broker HTTP connection closed",
    ))
}

fn invalid_data(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

fn invalid_input(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{self, Read, Write},
        net::{Ipv4Addr, TcpListener, TcpStream},
        thread::{self, JoinHandle},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    struct JoinOnDrop(Option<JoinHandle<()>>);

    impl JoinOnDrop {
        fn spawn(handle: JoinHandle<()>) -> Self {
            Self(Some(handle))
        }

        fn join(mut self) {
            self.0
                .take()
                .expect("peer still owned")
                .join()
                .expect("peer thread");
        }
    }

    impl Drop for JoinOnDrop {
        fn drop(&mut self) {
            if let Some(handle) = self.0.take() {
                let _ = handle.join();
            }
        }
    }

    fn listen() -> (TcpListener, u16) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    fn accept(listener: &TcpListener) -> TcpStream {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    let _ = stream.set_nodelay(true);
                    return stream;
                }
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < until =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    }

    fn connect_port(port: u16) -> TcpStream {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            match TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    return stream;
                }
                Err(_) if Instant::now() < until => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("connect: {error}"),
            }
        }
    }

    fn deadline_ms(ms: u64) -> Deadline {
        Deadline::after(Duration::from_millis(ms)).unwrap()
    }

    fn write_chunks(stream: &mut TcpStream, bytes: &[u8]) {
        for byte in bytes {
            stream.write_all(std::slice::from_ref(byte)).unwrap();
            stream.flush().unwrap();
        }
    }

    fn request(token: &str, body: &[u8]) -> Vec<u8> {
        request_with("POST", "/rpc", "HTTP/1.1", token, &[], body)
    }

    fn request_with(
        method: &str,
        path: &str,
        version: &str,
        token: &str,
        extra: &[&str],
        body: &[u8],
    ) -> Vec<u8> {
        let mut head = format!("{method} {path} {version}\r\n");
        if !token.is_empty() {
            head.push_str("Authorization: Bearer ");
            head.push_str(token);
            head.push_str("\r\n");
        }
        for line in extra {
            head.push_str(line);
            head.push_str("\r\n");
        }
        if !extra
            .iter()
            .any(|line| line.to_ascii_lowercase().starts_with("content-length:"))
        {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    fn http_ok(body: &Value) -> Vec<u8> {
        let raw = serde_json::to_vec(body).unwrap();
        let mut bytes = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            raw.len()
        )
        .into_bytes();
        bytes.extend_from_slice(&raw);
        bytes
    }

    fn drain(stream: &mut TcpStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
        let mut buf = [0_u8; 1024];
        let _ = stream.read(&mut buf);
    }

    #[test]
    fn outbound_rejects_invalid_port_and_token() {
        let cancel = Cancellation::default();
        let deadline = deadline_ms(500);
        let payload = json!({});
        let port = exchange(0, TOKEN, "status", &payload, deadline, &cancel).unwrap_err();
        assert_eq!(port.kind(), io::ErrorKind::InvalidInput);
        let token = exchange(1, "short", "status", &payload, deadline, &cancel).unwrap_err();
        assert_eq!(token.kind(), io::ErrorKind::InvalidInput);
        assert!(!format!("{token}").contains("short"));
    }

    #[test]
    fn subsecond_caller_budget_serializes_a_future_unix_deadline() {
        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = accept(&listener);
            let request = read_request(
                &mut stream,
                TOKEN,
                deadline_ms(2_000),
                &Cancellation::default(),
            )
            .expect("subsecond request");
            let cutoff = request["deadline"]
                .as_f64()
                .expect("serialized unix deadline");
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            assert!(
                cutoff > now,
                "serialized deadline {cutoff} was not still in the future at {now}"
            );
            write_response(
                &mut stream,
                200,
                &json!({"result": {"ok": true, "deadline": cutoff}}),
                deadline_ms(2_000),
                &Cancellation::default(),
            )
            .unwrap();
        }));
        let budget = Duration::from_millis(250);
        let send_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let result = exchange(
            port,
            TOKEN,
            "status",
            &json!({"probe": "subsecond"}),
            Deadline::after(budget).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(result["ok"], true);
        peer.join();
        let cutoff = result["deadline"].as_f64().expect("unix deadline");
        assert!(
            cutoff > send_now,
            "deadline {cutoff} was not in the future at send {send_now}"
        );
        assert!(
            (cutoff - send_now - budget.as_secs_f64()).abs() < 0.1,
            "deadline {cutoff} not close to {} from {send_now}",
            send_now + budget.as_secs_f64()
        );
    }

    #[test]
    fn fragmented_unicode_success_returns_full_result() {
        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = accept(&listener);
            let request = read_request(
                &mut stream,
                TOKEN,
                deadline_ms(2_000),
                &Cancellation::default(),
            )
            .expect("unicode request");
            assert_eq!(request["payload"]["text"], "日本 😀\n");
            write_response(
                &mut stream,
                200,
                &json!({"result": {"echo": "日本 😀\n", "ok": true}}),
                deadline_ms(2_000),
                &Cancellation::default(),
            )
            .unwrap();
        }));
        let mut client = connect_port(port);
        let body = serde_json::to_vec(&json!({
            "operation": "echo",
            "payload": {"text": "日本 😀\n"},
            "deadline": 1,
        }))
        .unwrap();
        write_chunks(
            &mut client,
            &request_with("POST", "/rpc", "HTTP/1.0", TOKEN, &[], &body),
        );
        let mut incoming = Vec::new();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        loop {
            let mut byte = [0_u8; 1];
            match client.read(&mut byte) {
                Ok(0) => break,
                Ok(1) => incoming.push(byte[0]),
                Ok(_) => unreachable!(),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("{error}"),
            }
        }
        let split = incoming
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let body = &incoming[split + 4..];
        assert_eq!(strict_json(body).unwrap()["result"]["echo"], "日本 😀\n");
        peer.join();

        let cancel = Cancellation::default();
        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = accept(&listener);
            let mut buf = [0_u8; 4096];
            let _ = stream.read(&mut buf);
            write_chunks(
                &mut stream,
                &http_ok(&json!({"result": {"echo": "日本 😀\n"}})),
            );
        }));
        let result = exchange(
            port,
            TOKEN,
            "echo",
            &json!({"text": "日本 😀\n"}),
            deadline_ms(2_000),
            &cancel,
        )
        .unwrap();
        assert_eq!(result["echo"], "日本 😀\n");
        peer.join();
    }

    #[test]
    fn wrong_or_missing_token_refuses_without_waiting_for_body() {
        for auth in [
            Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            None,
        ] {
            let (listener, port) = listen();
            let peer = JoinOnDrop::spawn(thread::spawn(move || {
                let mut stream = connect_port(port);
                let token = auth.unwrap_or("");
                let extra = ["Content-Length: 8000000"];
                let headers = request_with("POST", "/rpc", "HTTP/1.1", token, &extra, &[]);
                stream.write_all(&headers).unwrap();
                stream.flush().unwrap();
                thread::sleep(Duration::from_millis(400));
            }));
            let mut stream = accept(&listener);
            let started = Instant::now();
            let error = read_request(
                &mut stream,
                TOKEN,
                deadline_ms(2_000),
                &Cancellation::default(),
            )
            .unwrap_err();
            assert!(
                started.elapsed() < Duration::from_millis(800),
                "unauthorized waited for body: {:?}",
                started.elapsed()
            );
            assert!(matches!(error, RequestError::Unauthorized), "{error}");
            drop(stream);
            peer.join();
        }
    }

    #[test]
    fn duplicate_content_length_and_transfer_encoding_are_rejected() {
        let cases: [&[&str]; 2] = [
            &["Content-Length: 2", "Content-Length: 2"],
            &["Transfer-Encoding: chunked", "Content-Length: 2"],
        ];
        for extra in cases {
            let (listener, port) = listen();
            let extra = extra
                .iter()
                .map(|line| (*line).to_owned())
                .collect::<Vec<_>>();
            let peer = JoinOnDrop::spawn(thread::spawn(move || {
                let mut stream = connect_port(port);
                let lines: Vec<&str> = extra.iter().map(String::as_str).collect();
                let bytes = request_with("POST", "/rpc", "HTTP/1.1", TOKEN, &lines, b"{}");
                stream.write_all(&bytes).unwrap();
                stream.flush().unwrap();
            }));
            let mut stream = accept(&listener);
            let error = read_request(
                &mut stream,
                TOKEN,
                deadline_ms(2_000),
                &Cancellation::default(),
            )
            .unwrap_err();
            assert!(matches!(error, RequestError::Invalid), "{error}");
            drop(stream);
            peer.join();
        }
    }

    #[test]
    fn truncated_oversized_malformed_and_duplicate_json_are_rejected() {
        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = connect_port(port);
            let bytes = request(TOKEN, b"{\"x\":1}");
            stream.write_all(&bytes[..bytes.len() - 3]).unwrap();
            stream.flush().unwrap();
        }));
        let mut stream = accept(&listener);
        let error = read_request(
            &mut stream,
            TOKEN,
            deadline_ms(2_000),
            &Cancellation::default(),
        )
        .unwrap_err();
        let RequestError::Io(error) = error else {
            panic!("truncated body must stay EOF, got {error}");
        };
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        drop(stream);
        peer.join();

        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = connect_port(port);
            let bytes = request_with(
                "POST",
                "/rpc",
                "HTTP/1.1",
                TOKEN,
                &["Content-Length: 16777217"],
                &[],
            );
            stream.write_all(&bytes).unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_millis(400));
        }));
        let mut stream = accept(&listener);
        let started = Instant::now();
        let error = read_request(
            &mut stream,
            TOKEN,
            deadline_ms(2_000),
            &Cancellation::default(),
        )
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_millis(800));
        assert!(matches!(error, RequestError::Invalid), "{error}");
        drop(stream);
        peer.join();

        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = connect_port(port);
            stream.write_all(&request(TOKEN, b"{")).unwrap();
            stream.flush().unwrap();
        }));
        let mut stream = accept(&listener);
        let error = read_request(
            &mut stream,
            TOKEN,
            deadline_ms(2_000),
            &Cancellation::default(),
        )
        .unwrap_err();
        assert!(matches!(error, RequestError::Invalid), "{error}");
        drop(stream);
        peer.join();

        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = connect_port(port);
            stream
                .write_all(&request(
                    TOKEN,
                    b"{\"operation\":\"a\",\"operation\":\"b\",\"payload\":{},\"deadline\":1}",
                ))
                .unwrap();
            stream.flush().unwrap();
        }));
        let mut stream = accept(&listener);
        let error = read_request(
            &mut stream,
            TOKEN,
            deadline_ms(2_000),
            &Cancellation::default(),
        )
        .unwrap_err();
        assert!(matches!(error, RequestError::Invalid), "{error}");
        drop(stream);
        peer.join();
    }

    #[test]
    fn slow_trickle_does_not_reset_hard_deadline() {
        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = connect_port(port);
            let body = vec![b'x'; 40];
            let headers = request_with(
                "POST",
                "/rpc",
                "HTTP/1.1",
                TOKEN,
                &[&format!("Content-Length: {}", body.len())],
                &[],
            );
            stream.write_all(&headers).unwrap();
            stream.flush().unwrap();
            for byte in body {
                thread::sleep(Duration::from_millis(80));
                if stream.write_all(&[byte]).is_err() {
                    break;
                }
                let _ = stream.flush();
            }
        }));
        let mut stream = accept(&listener);
        let started = Instant::now();
        let error = read_request(
            &mut stream,
            TOKEN,
            deadline_ms(400),
            &Cancellation::default(),
        )
        .unwrap_err();
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(200), "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        let RequestError::Io(error) = error else {
            panic!("trickle must time out, got {error}");
        };
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        drop(stream);
        peer.join();
    }

    #[test]
    fn cancellation_unblocks_while_peer_holds_connection() {
        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = accept(&listener);
            let until = Instant::now() + Duration::from_secs(3);
            stream
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            let mut buf = [0_u8; 1];
            while Instant::now() < until {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                        ) =>
                    {
                        continue;
                    }
                    Err(_) => break,
                }
            }
        }));
        let cancel = Cancellation::default();
        let stopper = cancel.clone();
        let stop = JoinOnDrop::spawn(thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            stopper.cancel();
        }));
        let started = Instant::now();
        let error = exchange(
            port,
            TOKEN,
            "status",
            &json!({}),
            deadline_ms(2_000),
            &cancel,
        )
        .unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        stop.join();
        peer.join();
    }

    #[test]
    fn non_success_http_and_tool_level_iserror_are_forwarded() {
        let cancel = Cancellation::default();
        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = accept(&listener);
            drain(&mut stream);
            let body = b"{}";
            let response = format!(
                "HTTP/1.1 500 Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        }));
        let error = exchange(
            port,
            TOKEN,
            "status",
            &json!({}),
            deadline_ms(2_000),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!format!("{error}").contains("{}"));
        peer.join();

        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = accept(&listener);
            drain(&mut stream);
            write_chunks(
                &mut stream,
                &http_ok(&json!({
                    "result": {
                        "isError": true,
                        "content": [{"type": "text", "text": "tool"}]
                    }
                })),
            );
        }));
        let result =
            exchange(port, TOKEN, "call", &json!({}), deadline_ms(2_000), &cancel).unwrap();
        assert_eq!(result["isError"], true);
        peer.join();

        let (listener, port) = listen();
        let peer = JoinOnDrop::spawn(thread::spawn(move || {
            let mut stream = accept(&listener);
            drain(&mut stream);
            stream
                .write_all(&http_ok(&json!({"error": "no such operation"})))
                .unwrap();
        }));
        let error = exchange(
            port,
            TOKEN,
            "missing",
            &json!({}),
            deadline_ms(2_000),
            &cancel,
        )
        .unwrap_err();
        assert!(format!("{error}").contains("no such operation"));
        peer.join();
    }
}
