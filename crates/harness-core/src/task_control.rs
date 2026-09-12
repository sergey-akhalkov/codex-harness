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
