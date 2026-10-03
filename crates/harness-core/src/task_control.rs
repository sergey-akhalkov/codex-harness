//! Bounded native app-server transport. A wait expiring says nothing about the
//! task's execution state; only correlated protocol events can establish that.
use serde_json::Value;
use std::{
    io,
    net::{Ipv4Addr, SocketAddr, TcpStream},
    time::Duration,
};
use tungstenite::{
    Message, WebSocket, client::IntoClientRequest, client::client_with_config, http::HeaderValue,
    protocol::WebSocketConfig,
};
use windows_sys::Win32::System::Console::{
    COORD, GetConsoleProcessList, GetConsoleScreenBufferInfo, GetConsoleTitleW, GetConsoleWindow,
    GetStdHandle, ReadConsoleOutputCharacterW, STD_OUTPUT_HANDLE, SetConsoleTitleW,
};

/// One authenticated connection to an explicitly owned local server. It does
/// not enumerate sessions, infer task state, retry requests, or select models.
pub struct ControlConnection {
    socket: WebSocket<TcpStream>,
}

impl ControlConnection {
    pub fn connect(port: u16, token: &str, timeout: Duration) -> io::Result<Self> {
        if port == 0 || token.len() < 32 || !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid local control endpoint",
            ));
        }
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let mut request = format!("ws://{address}")
            .into_client_request()
            .map_err(protocol_error)?;
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(protocol_error)?,
        );
        let (socket, _) =
            client_with_config(request, stream, Some(config())).map_err(protocol_error)?;
        Ok(Self { socket })
    }

    pub fn send(&mut self, value: &Value, timeout: Duration) -> io::Result<()> {
        let text = serde_json::to_string(value)?;
        self.socket.get_mut().set_write_timeout(Some(timeout))?;
        self.socket
            .send(Message::Text(text.into()))
            .map_err(protocol_error)
    }

    /// Returns None for an observation deadline, preserving the connection and
    /// any partial WebSocket frame. No request is replayed when called again.
    pub fn receive(&mut self, timeout: Duration) -> io::Result<Option<Value>> {
        self.read_message(timeout)
    }

    fn read_message(&mut self, timeout: Duration) -> io::Result<Option<Value>> {
        let started = std::time::Instant::now();
        loop {
            // SO_RCVTIMEO expiry leaves a Windows connection indeterminate.
            // Probe without starting a blocking receive, then wait for socket
            // readiness. Tungstenite retains incomplete frames on WouldBlock.
            self.socket.get_mut().set_nonblocking(true)?;
            let result = self.socket.read();
            // Reads can queue heartbeat writes. Restore the existing bounded
            // blocking send/flush behavior on every outcome, including errors.
            let restored = self.socket.get_mut().set_nonblocking(false);
            match result {
                Err(tungstenite::Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                    restored?;
                    let remaining = timeout.saturating_sub(started.elapsed());
                    if remaining.is_zero() || !wait_readable(self.socket.get_ref(), remaining)? {
                        return Ok(None);
                    }
                }
                Err(tungstenite::Error::Io(error)) => return Err(error),
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed)
                | Ok(Message::Close(_)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "control connection closed; reconcile task state",
                    ));
                }
                Err(error) => return Err(protocol_error(error)),
                Ok(message) => {
                    restored?;
                    return match message {
                        Message::Text(text) => {
                            serde_json::from_str(&text).map(Some).map_err(Into::into)
                        }
                        Message::Ping(_) | Message::Pong(_) => {
                            self.socket.flush().map_err(protocol_error)?;
                            Ok(None)
                        }
                        _ => Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "expected JSON text control record",
                        )),
                    };
                }
            }
        }
    }
}

/// A readiness timeout starts no receive operation and leaves the socket usable.
/// Error/hangup readiness is consumed by the next read, preserving its cause.
fn wait_readable(stream: &TcpStream, timeout: Duration) -> io::Result<bool> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        POLLRDNORM, SOCKET_ERROR, WSAGetLastError, WSAPOLLFD, WSAPoll,
    };

    let mut descriptor = WSAPOLLFD {
        fd: stream.as_raw_socket() as _,
        events: POLLRDNORM,
        revents: 0,
    };
    // Round sub-millisecond waits up; never convert a large bound to -1
    // (Winsock's infinite wait). The caller accounts for elapsed time.
    let millis = timeout.as_nanos().div_ceil(1_000_000).min(i32::MAX as u128) as i32;
    // SAFETY: the descriptor points to one live socket borrowed for this call.
    let ready = unsafe { WSAPoll(&mut descriptor, 1, millis) };
    if ready == SOCKET_ERROR {
        // Read Winsock's error immediately, not a stale Win32 last error.
        Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }))
    } else {
        Ok(ready > 0)
    }
}

fn config() -> WebSocketConfig {
    // No harness-side message, frame or write-buffer size cap: a managed
    // conversation's real records - full-thread reads and exact-session
    // resume state - legitimately exceed any small bound, and both peers are
    // owned localhost processes of this host.
    WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(usize::MAX)
        .max_message_size(None)
        .max_frame_size(None)
}

fn protocol_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

/// The current console caption, when this process has a console. `None` means
/// there is no console to give a frontend; an empty caption is a live console
/// that has not loaded a thread yet.
pub fn console_caption() -> io::Result<Option<String>> {
    let mut buffer = [0u16; 1024];
    let count = unsafe { GetConsoleTitleW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if count == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(6) || unsafe { GetConsoleWindow() }.is_null() {
            return Ok(None);
        }
        return Ok(Some(String::new()));
    }
    let caption = String::from_utf16_lossy(&buffer[..count as usize]);
    Ok(Some(caption))
}

/// Whether `pid` is attached to this process's console. A created process that
/// is not on this console does not own the host's terminal surface.
pub fn console_contains(pid: u32) -> io::Result<bool> {
    if pid == 0 {
        return Ok(false);
    }
    let mut list = vec![0u32; 64];
    let mut count = unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) };
    if count == 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(6) {
            Ok(false)
        } else {
            Err(error)
        };
    }
    if count as usize > list.len() {
        list.resize(count as usize, 0);
        count = unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) };
        if count == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(list[..count as usize].contains(&pid))
}

/// The native TUI replaces the console caption with `{title} | ` after it has
/// loaded that named thread. A running process or an untitled console is not
/// attachment.
pub fn frontend_loaded(pid: u32, title: &str) -> io::Result<bool> {
    let Some(caption) = console_caption()? else {
        return Ok(false);
    };
    if !console_contains(pid)? {
        return Ok(false);
    }
    Ok(caption_loaded(&caption, title))
}

/// Bounded text from this console, only while the named child belongs to it.
/// Used after a failed frontend attachment, never as readiness evidence. This
/// reads characters, not pixels, and neither sends input nor changes the UI.
pub fn frontend_console_text(pid: u32) -> io::Result<Option<String>> {
    if !console_contains(pid)? {
        return Ok(None);
    }
    let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    let mut info = unsafe { std::mem::zeroed() };
    if unsafe { GetConsoleScreenBufferInfo(output, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let left = info.srWindow.Left;
    let bottom = info.srWindow.Bottom;
    let top = info.srWindow.Top.max(bottom.saturating_sub(63));
    let width = (i32::from(info.srWindow.Right) - i32::from(left) + 1).clamp(1, 1024);
    let mut line = vec![0_u16; width as usize];
    let mut text = String::new();
    for y in top..=bottom {
        let mut read = 0;
        if unsafe {
            ReadConsoleOutputCharacterW(
                output,
                line.as_mut_ptr(),
                line.len() as u32,
                COORD { X: left, Y: y },
                &mut read,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let decoded = String::from_utf16_lossy(&line[..read as usize]);
        let trimmed = decoded.trim_end();
        if !trimmed.is_empty() {
            text.push_str(trimmed);
            text.push('\n');
        }
        if text.len() > 4096 {
            let mut start = text.len() - 4096;
            while !text.is_char_boundary(start) {
                start += 1;
            }
            text.drain(..start);
        }
    }
    Ok(Some(text.trim_end().to_owned()))
}

/// Sets this process's console caption. Used by an owned frontend double; the
/// production TUI sets its own caption.
pub fn set_console_caption(title: &str) -> io::Result<()> {
    let mut wide: Vec<u16> = title.encode_utf16().collect();
    wide.push(0);
    if unsafe { SetConsoleTitleW(wide.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn caption_loaded(caption: &str, title: &str) -> bool {
    let expected = format!("{title} | ");
    if caption.starts_with(&expected) {
        return true;
    }
    // The native TUI prefixes a braille spinner while the first frame is active.
    let mut chars = caption.chars();
    matches!(chars.next(), Some('\u{2800}'..='\u{28ff}'))
        && chars.next() == Some(' ')
        && chars.as_str().starts_with(&expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{io::Write, net::TcpListener, thread, time::Instant};

    const WAIT: Duration = Duration::from_secs(2);

    fn connected() -> (ControlConnection, WebSocket<TcpStream>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(WAIT)).unwrap();
            stream.set_write_timeout(Some(WAIT)).unwrap();
            tungstenite::accept(stream).unwrap()
        });
        let client = ControlConnection::connect(port, &"a".repeat(32), WAIT).unwrap();
        (client, server.join().unwrap())
    }

    #[test]
    fn repeated_idle_and_partial_frames_keep_the_connection_and_order() {
        let (mut client, mut server) = connected();
        let started = Instant::now();
        for sequence in 0..128 {
            for _ in 0..4 {
                assert_eq!(
                    client.receive(Duration::from_millis(1)).unwrap(),
                    None,
                    "idle before event {sequence}"
                );
            }
            let expected = json!({"event": sequence});
            let text = expected.to_string();
            let frame = [vec![0x81, text.len() as u8], text.into_bytes()].concat();
            server.get_mut().write_all(&frame[..3]).unwrap();
            assert_eq!(client.receive(Duration::from_millis(1)).unwrap(), None);
            server.get_mut().write_all(&frame[3..]).unwrap();
            let next = json!({"next": sequence});
            server.send(Message::Text(next.to_string().into())).unwrap();
            assert_eq!(client.receive(WAIT).unwrap(), Some(expected));
            assert_eq!(client.receive(WAIT).unwrap(), Some(next));
            let reply = json!({"ack": sequence});
            client.send(&reply, WAIT).unwrap();
            assert_eq!(
                server.read().unwrap().into_text().unwrap(),
                reply.to_string()
            );
            assert!(started.elapsed() < Duration::from_secs(30));
        }
    }

    #[test]
    fn a_trickling_frame_cannot_extend_the_observation_deadline() {
        let (mut client, mut server) = connected();
        let expected = json!({"message": "a deliberately fragmented control record"});
        let text = expected.to_string();
        let frame = [vec![0x81, text.len() as u8], text.into_bytes()].concat();
        server.get_mut().write_all(&frame[..3]).unwrap();
        let writer = thread::spawn(move || {
            for byte in &frame[3..] {
                thread::sleep(Duration::from_millis(10));
                server.get_mut().write_all(&[*byte]).unwrap();
            }
            server
        });
        let started = Instant::now();
        let observed = client.receive(Duration::from_millis(80));
        let elapsed = started.elapsed();
        let server = writer.join().unwrap();
        assert_eq!(observed.unwrap(), None, "elapsed: {elapsed:?}");
        assert!(elapsed < Duration::from_millis(300), "elapsed: {elapsed:?}");
        assert_eq!(client.receive(WAIT).unwrap(), Some(expected));
        drop(server);
    }

    #[test]
    fn heartbeat_and_buffered_events_survive_observation_deadlines() {
        let (mut client, mut server) = connected();
        assert_eq!(client.receive(Duration::from_millis(1)).unwrap(), None);
        server.send(Message::Ping(vec![1, 2, 3].into())).unwrap();
        server
            .send(Message::Text(json!({"event": 1}).to_string().into()))
            .unwrap();
        server
            .send(Message::Text(json!({"event": 2}).to_string().into()))
            .unwrap();
        assert_eq!(client.receive(WAIT).unwrap(), None);
        assert_eq!(server.read().unwrap(), Message::Pong(vec![1, 2, 3].into()));
        assert_eq!(client.receive(WAIT).unwrap(), Some(json!({"event": 1})));
        assert_eq!(client.receive(WAIT).unwrap(), Some(json!({"event": 2})));
    }

    #[test]
    fn invalid_messages_and_disconnects_are_not_idle_observations() {
        let (mut client, mut server) = connected();
        assert_eq!(client.receive(Duration::from_millis(1)).unwrap(), None);
        server.send(Message::Text("{invalid JSON".into())).unwrap();
        assert_eq!(
            client.receive(WAIT).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        server.close(None).unwrap();
        assert_eq!(
            client.receive(WAIT).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );

        let (mut client, mut server) = connected();
        // A peer disappearing halfway through a frame must not look idle.
        server.get_mut().write_all(&[0x81, 20, b'{']).unwrap();
        assert_eq!(client.receive(Duration::from_millis(1)).unwrap(), None);
        drop(server);
        assert!(client.receive(WAIT).is_err());
    }

    #[test]
    fn a_send_after_an_idle_read_waits_for_backpressure_without_replay() {
        let (mut client, mut server) = connected();
        assert_eq!(client.receive(Duration::from_millis(1)).unwrap(), None);
        let message = json!({"payload": "x".repeat(4 * 1024 * 1024)});
        let expected = message.to_string();
        let reader = thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            let text = server.read().unwrap().into_text().unwrap();
            assert_eq!(text, expected);
            server
                .send(Message::Text(json!({"accepted": 1}).to_string().into()))
                .unwrap();
            server
        });
        client.send(&message, WAIT).unwrap();
        assert_eq!(client.receive(WAIT).unwrap(), Some(json!({"accepted": 1})));
        let mut server = reader.join().unwrap();
        server.get_mut().set_nonblocking(true).unwrap();
        assert!(matches!(server.read(), Err(tungstenite::Error::Io(error))
            if error.kind() == io::ErrorKind::WouldBlock));
    }
}
