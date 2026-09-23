//! Owned canned app-server endpoint for the control-backed executor checks.
//!
//! The kit has no model-free `codex app-server` double, so the checks that must
//! observe the control protocol own a real loopback WebSocket endpoint instead:
//! it speaks the handshake the kit's `ControlConnection` performs (RFC 6455 with
//! the capability bearer in the `Authorization` header), records every request it
//! receives, answers each method from a canned answer - a sequence when a check
//! must answer the same method differently over time - and pushes notifications
//! to its connected clients on demand.
//!
//! Nothing here touches a model, a subscription or the installed CLI.
// Each test binary that includes this fixture uses a subset of it, so an item
// unused in one binary is not dead code.
#![allow(dead_code)]

use std::{
    collections::{HashMap, VecDeque},
    fs, io,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub fn header(head: &str, name: &str) -> Option<String> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

pub fn accept_key(key: &str) -> String {
    base64(&sha1(
        format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
    ))
}

pub fn sha1(input: &[u8]) -> [u8; 20] {
    let bit_length = (input.len() as u64) * 8;
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    let mut state = [
        0x67452301u32,
        0xEFCDAB89,
        0x98BADCFE,
        0x10325476,
        0xC3D2E1F0,
    ];
    for block in message.as_chunks::<64>().0 {
        let mut words = [0u32; 80];
        for (index, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
            words[index] = u32::from_be_bytes(*chunk);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) =
            (state[0], state[1], state[2], state[3], state[4]);
        for (index, word) in words.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    let mut digest = [0u8; 20];
    for (index, value) in state.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    digest
}

pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let padded = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let value =
            (u32::from(padded[0]) << 16) | (u32::from(padded[1]) << 8) | u32::from(padded[2]);
        out.push(ALPHABET[(value >> 18) as usize & 63] as char);
        out.push(ALPHABET[(value >> 12) as usize & 63] as char);
        out.push(match chunk.len() {
            1 => '=',
            _ => ALPHABET[(value >> 6) as usize & 63] as char,
        });
        out.push(match chunk.len() {
            3 => ALPHABET[value as usize & 63] as char,
            _ => '=',
        });
    }
    out
}

/// RFC 6455's own example: it pins both the digest and the base64 encoder of the
/// fixture's handshake.
#[test]
fn accept_key_matches_the_rfc_6455_example() {
    assert_eq!(
        accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
    assert_eq!(
        sha1(b"abc"),
        [
            0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
            0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d
        ]
    );
}

pub enum Poll {
    Message(String),
    Closed,
    Idle,
}

enum Frame {
    Text(String),
    Closed,
    Ping,
}

pub struct Wire {
    stream: TcpStream,
    buffer: Vec<u8>,
}

/// What the canned endpoint accepts as its bearer.
#[derive(Clone)]
pub enum Bearer {
    /// The exact value, for the refusal checks.
    Value(String),
    /// The token file the module writes, read at handshake time exactly as the
    /// installed app-server reads it through `--ws-token-file`.
    File(PathBuf),
}

impl Bearer {
    fn resolve(&self) -> Option<String> {
        match self {
            Self::Value(value) => Some(value.clone()),
            Self::File(path) => fs::read_to_string(path)
                .ok()
                .map(|text| text.trim().to_owned()),
        }
    }
}

impl Wire {
    pub fn accept(mut stream: TcpStream, bearer: &Bearer) -> io::Result<Self> {
        // A stream accepted from the nonblocking listener inherits that mode
        // on Windows: without an explicit switch back, a read whose bytes
        // have not arrived yet fails with WSAEWOULDBLOCK instead of waiting,
        // the handshake is dropped and the client sees an aborted connection.
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut received = Vec::new();
        let mut chunk = [0u8; 512];
        let head_end = loop {
            if let Some(index) = find(&received, b"\r\n\r\n") {
                break index;
            }
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                return Err(io::Error::other("handshake closed early"));
            }
            received.extend_from_slice(&chunk[..read]);
            if received.len() > 8192 {
                return Err(io::Error::other("handshake exceeds its bound"));
            }
        };
        let head = String::from_utf8_lossy(&received[..head_end]).into_owned();
        let key = header(&head, "sec-websocket-key").unwrap_or_default();
        let expected = bearer.resolve().map(|token| format!("Bearer {token}"));
        if header(&head, "authorization") != expected {
            let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
            let _ = stream.flush();
            return Err(io::Error::other("capability token refused"));
        }
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
            accept_key(&key)
        );
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        let mut wire = Self {
            stream,
            buffer: Vec::new(),
        };
        wire.buffer.extend_from_slice(&received[head_end + 4..]);
        Ok(wire)
    }

    pub fn poll_text(&mut self, timeout: Duration) -> io::Result<Poll> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self.take_frame()? {
                Some(Frame::Text(text)) => return Ok(Poll::Message(text)),
                Some(Frame::Closed) => return Ok(Poll::Closed),
                Some(Frame::Ping) => {
                    self.write_frame(0xA, b"")?;
                }
                None => {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        return Ok(Poll::Idle);
                    }
                    self.stream.set_read_timeout(Some(remaining))?;
                    let mut chunk = [0u8; 4096];
                    match self.stream.read(&mut chunk) {
                        Ok(0) => return Ok(Poll::Closed),
                        Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                            ) =>
                        {
                            return Ok(Poll::Idle);
                        }
                        Err(error) => return Err(error),
                    }
                    if self.buffer.len() > 1024 * 1024 + 16 {
                        return Err(io::Error::other("canned frame exceeds its bound"));
                    }
                }
            }
        }
    }

    fn take_frame(&mut self) -> io::Result<Option<Frame>> {
        if self.buffer.len() < 2 {
            return Ok(None);
        }
        let opcode = self.buffer[0] & 0x0f;
        let masked = self.buffer[1] & 0x80 != 0;
        let (header, length) = match self.buffer[1] & 0x7f {
            126 => {
                if self.buffer.len() < 4 {
                    return Ok(None);
                }
                (
                    4,
                    u16::from_be_bytes([self.buffer[2], self.buffer[3]]) as usize,
                )
            }
            127 => {
                if self.buffer.len() < 10 {
                    return Ok(None);
                }
                let mut size = [0u8; 8];
                size.copy_from_slice(&self.buffer[2..10]);
                let size = u64::from_be_bytes(size);
                if size > 1024 * 1024 {
                    return Err(io::Error::other("canned frame exceeds its bound"));
                }
                (10, size as usize)
            }
            small => (2, small as usize),
        };
        let header = if masked { header + 4 } else { header };
        if self.buffer.len() < header + length {
            return Ok(None);
        }
        let mask = masked.then(|| {
            let mut key = [0u8; 4];
            key.copy_from_slice(&self.buffer[header - 4..header]);
            key
        });
        let mut payload = self.buffer[header..header + length].to_vec();
        if let Some(key) = mask {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= key[index % 4];
            }
        }
        self.buffer.drain(..header + length);
        Ok(Some(match opcode {
            0x1 => Frame::Text(String::from_utf8_lossy(&payload).into_owned()),
            0x8 => Frame::Closed,
            0x9 => Frame::Ping,
            other => {
                return Err(io::Error::other(format!(
                    "canned frame opcode {other} is unsupported"
                )));
            }
        }))
    }

    pub fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.write_frame(0x1, text.as_bytes())
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> io::Result<()> {
        let mut frame = vec![0x80 | opcode];
        let length = payload.len();
        if length < 126 {
            frame.push(length as u8);
        } else if length <= usize::from(u16::MAX) {
            frame.push(126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        } else {
            frame.push(127);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }
        frame.extend_from_slice(payload);
        self.stream.write_all(&frame)?;
        self.stream.flush()
    }
}

// ------------------------------------------------------------- canned server

/// One canned answer to one request method.
#[derive(Clone)]
pub enum Answer {
    Result(serde_json::Value),
    Error(serde_json::Value),
    /// A complete record sent verbatim; `"@request"` as its id becomes the
    /// request's own id, so protocol deviations can be exercised.
    Record(serde_json::Value),
    /// No answer at all: the client's own bound decides what that means.
    Silence,
}

#[derive(Default)]
struct Canned {
    requests: Vec<serde_json::Value>,
    answers: HashMap<String, VecDeque<Answer>>,
    notifications: VecDeque<serde_json::Value>,
    connections: usize,
    refusals: usize,
}

impl Canned {
    /// Takes the next answer of one method; the last answer of a sequence
    /// stands for every later request of that method.
    fn take_answer(&mut self, method: &str) -> Option<Answer> {
        let queue = self.answers.get_mut(method)?;
        let answer = queue.pop_front()?;
        if queue.is_empty() {
            queue.push_back(answer.clone());
        }
        Some(answer)
    }
}

/// A canned app-server endpoint on a real loopback port.
pub struct Server {
    pub port: u16,
    state: Arc<Mutex<Canned>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(bearer: Bearer) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(Canned::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            thread::spawn(move || serve(listener, &bearer, &state, &stop))
        };
        Self {
            port,
            state,
            stop,
            worker: Some(worker),
        }
    }

    /// Answers one method with one answer, replacing every answer queued for it.
    pub fn answer(&self, method: &str, answer: Answer) {
        let mut canned = self.state.lock().unwrap();
        let queue = canned.answers.entry(method.to_owned()).or_default();
        queue.clear();
        queue.push_back(answer);
    }

    /// Answers one method with a sequence of answers, consumed one request at a
    /// time; the last answer stands for every later request of that method.
    pub fn answer_sequence(&self, method: &str, answers: Vec<Answer>) {
        let mut canned = self.state.lock().unwrap();
        canned
            .answers
            .insert(method.to_owned(), answers.into_iter().collect());
    }

    pub fn push(&self, notification: serde_json::Value) {
        self.state
            .lock()
            .unwrap()
            .notifications
            .push_back(notification);
    }

    pub fn requests(&self) -> Vec<serde_json::Value> {
        self.state.lock().unwrap().requests.clone()
    }

    pub fn requests_for(&self, method: &str) -> Vec<serde_json::Value> {
        self.requests()
            .into_iter()
            .filter(|request| request["method"] == method)
            .collect()
    }

    pub fn connections(&self) -> usize {
        self.state.lock().unwrap().connections
    }

    pub fn refusals(&self) -> usize {
        self.state.lock().unwrap().refusals
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(
    listener: TcpListener,
    bearer: &Bearer,
    state: &Arc<Mutex<Canned>>,
    stop: &Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                // A real app-server serves several clients, so every accepted
                // connection is served on its own worker.
                let bearer = bearer.clone();
                let state = Arc::clone(state);
                let stop = Arc::clone(stop);
                thread::spawn(move || match Wire::accept(stream, &bearer) {
                    Ok(mut wire) => {
                        state.lock().unwrap().connections += 1;
                        let _ = converse(&mut wire, &state, &stop);
                    }
                    Err(_) => state.lock().unwrap().refusals += 1,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            // A transient accept failure - for example an incoming
            // connection that was reset before accept on loopback - must not
            // kill the canned endpoint: the next client would then see a
            // refused/aborted connect and the check would flake. Poll again
            // until the owning test stops the server.
            Err(_) => thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// Serves one connection: queued notifications first, then the answers of the
/// requests that arrive. Returns false when the connection is unusable.
fn converse(wire: &mut Wire, state: &Arc<Mutex<Canned>>, stop: &Arc<AtomicBool>) -> bool {
    while !stop.load(Ordering::SeqCst) {
        let queued: Vec<serde_json::Value> =
            state.lock().unwrap().notifications.drain(..).collect();
        for value in queued {
            if wire.write_text(&value.to_string()).is_err() {
                return false;
            }
        }
        match wire.poll_text(Duration::from_millis(5)) {
            Ok(Poll::Message(text)) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    return false;
                };
                let id = value["id"].clone();
                let method = value["method"].as_str().unwrap_or_default().to_owned();
                // The app-server answers requests and stays silent for
                // notifications such as `initialized`.
                let answers = !id.is_null();
                let answer = {
                    let mut canned = state.lock().unwrap();
                    canned.requests.push(value);
                    canned.take_answer(&method)
                };
                if !answers {
                    continue;
                }
                let record = match answer.unwrap_or(Answer::Result(serde_json::json!({}))) {
                    Answer::Result(result) => serde_json::json!({"id": id, "result": result}),
                    Answer::Error(error) => serde_json::json!({"id": id, "error": error}),
                    Answer::Record(mut record) => {
                        if record["id"] == "@request" {
                            record["id"] = id.clone();
                        }
                        record
                    }
                    Answer::Silence => continue,
                };
                if wire.write_text(&record.to_string()).is_err() {
                    return false;
                }
            }
            Ok(Poll::Idle) => {}
            Ok(Poll::Closed) | Err(_) => return true,
        }
    }
    true
}
