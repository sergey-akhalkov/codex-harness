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
    GetConsoleProcessList, GetConsoleTitleW, GetConsoleWindow, SetConsoleTitleW,
};

const RECORD_LIMIT: usize = 1024 * 1024;

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
        if text.len() > RECORD_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "control record limit exceeded",
            ));
        }
        self.socket.get_mut().set_write_timeout(Some(timeout))?;
        self.socket
            .send(Message::Text(text.into()))
            .map_err(protocol_error)
    }

    /// Returns None for an observation timeout, preserving the connection and
    /// any partial WebSocket frame. No request is replayed when called again.
    pub fn receive(&mut self, timeout: Duration) -> io::Result<Option<Value>> {
        self.socket.get_mut().set_read_timeout(Some(timeout))?;
        self.read_message()
    }

    fn read_message(&mut self) -> io::Result<Option<Value>> {
        match self.socket.read() {
            Ok(Message::Text(text)) => serde_json::from_str(&text).map(Some).map_err(Into::into),
            Ok(Message::Ping(_) | Message::Pong(_)) => {
                self.socket.flush().map_err(protocol_error)?;
                Ok(None)
            }
            Ok(Message::Close(_))
            | Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "control connection closed; reconcile task state",
                ))
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(protocol_error(error)),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected JSON text control record",
            )),
        }
    }
}

fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(RECORD_LIMIT + 4096)
        .max_message_size(Some(RECORD_LIMIT))
        .max_frame_size(Some(RECORD_LIMIT))
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
