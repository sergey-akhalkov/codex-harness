//! Bounded MCP stdio framing and JSON-RPC envelopes, independent of tool dispatch.
#![cfg(windows)]

use crate::dependency_mcp_probe::strict_json;
use serde_json::{Map, Value, json};
use std::io;

pub const MAX_FRAME: usize = 16 * 1024 * 1024;
pub const READ_CHUNK: usize = 64 * 1024;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid MCP JSON-RPC message")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Request,
    Notification,
    Result,
    Error,
}

/// Retains the exact JSON values, including numeric/string ID distinction and
/// extension fields. Dispatchers remain responsible for method-specific schemas.
#[derive(Clone, Debug)]
pub struct Message {
    value: Value,
    kind: Kind,
}

fn request_id(value: &Value) -> bool {
    value.is_string() || value.is_i64() || value.is_u64()
}

impl Message {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_FRAME || bytes.contains(&b'\n') {
            return Err(invalid());
        }
        let value = strict_json(bytes).map_err(|_| invalid())?;
        let object = value.as_object().ok_or_else(invalid)?;
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(invalid());
        }
        let id = object.get("id");
        let kind = if let Some(method) = object.get("method") {
            if !method.as_str().is_some_and(|method| !method.is_empty())
                || object.contains_key("result")
                || object.contains_key("error")
                || object
                    .get("params")
                    .is_some_and(|params| !params.is_object())
            {
                return Err(invalid());
            }
            match id {
                Some(id) if request_id(id) => Kind::Request,
                None => Kind::Notification,
                _ => return Err(invalid()),
            }
        } else if object.contains_key("result") && !object.contains_key("error") {
            if !id.is_some_and(request_id) || object.contains_key("params") {
                return Err(invalid());
            }
            Kind::Result
        } else if let Some(error) = object.get("error").and_then(Value::as_object) {
            if object.contains_key("result")
                || object.contains_key("params")
                || id.is_some_and(|id| !id.is_null() && !request_id(id))
                || !error
                    .get("code")
                    .is_some_and(|code| code.is_i64() || code.is_u64())
                || !error.get("message").is_some_and(Value::is_string)
            {
                return Err(invalid());
            }
            Kind::Error
        } else {
            return Err(invalid());
        };
        Ok(Self { value, kind })
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn id(&self) -> Option<&Value> {
        self.value.get("id").filter(|id| !id.is_null())
    }
    pub fn method(&self) -> Option<&str> {
        self.value.get("method").and_then(Value::as_str)
    }
    pub fn params(&self) -> Option<&Map<String, Value>> {
        self.value.get("params").and_then(Value::as_object)
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn into_value(self) -> Value {
        self.value
    }

    pub fn result(id: &Value, result: Value) -> io::Result<Self> {
        Self::parse(&serde_json::to_vec(
            &json!({"jsonrpc":"2.0","id":id,"result":result}),
        )?)
    }

    pub fn error(
        id: Option<&Value>,
        code: i64,
        message: &str,
        data: Option<Value>,
    ) -> io::Result<Self> {
        let mut error = json!({"code":code,"message":message});
        if let Some(data) = data {
            error["data"] = data;
        }
        Self::parse(&serde_json::to_vec(
            &json!({"jsonrpc":"2.0","id":id,"error":error}),
        )?)
    }

    /// Exactly one compact UTF-8 JSON envelope followed by LF; no logging.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(&self.value)?;
        if bytes.len() > MAX_FRAME {
            return Err(invalid());
        }
        bytes.push(b'\n');
        Ok(bytes)
    }
}

/// Feed at most READ_CHUNK bytes, then drain complete frames before feeding more.
/// Buffering is bounded even if a peer never emits a delimiter. A rejected frame
/// poisons this decoder; a protocol failure cannot silently resynchronize later.
#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
    consumed: usize,
    scanned: usize,
    failed: bool,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.failed
            || bytes.len() > READ_CHUNK
            || self
                .buffer
                .len()
                .saturating_sub(self.consumed)
                .saturating_add(bytes.len())
                > MAX_FRAME + READ_CHUNK + 1
        {
            self.failed = true;
            return Err(invalid());
        }
        if self.consumed > 0 {
            self.buffer.drain(..self.consumed);
            self.scanned -= self.consumed;
            self.consumed = 0;
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    pub fn next_message(&mut self) -> io::Result<Option<Message>> {
        if self.failed {
            return Err(invalid());
        }
        let Some(offset) = self.buffer[self.scanned..]
            .iter()
            .position(|byte| *byte == b'\n')
        else {
            self.scanned = self.buffer.len();
            if self.buffer.len() - self.consumed > MAX_FRAME + 1 {
                self.failed = true;
                return Err(invalid());
            }
            return Ok(None);
        };
        let end = self.scanned + offset;
        let frame = &self.buffer[self.consumed..end];
        let frame = frame.strip_suffix(b"\r").unwrap_or(frame);
        let message = match Message::parse(frame) {
            Ok(message) => message,
            Err(error) => {
                self.failed = true;
                return Err(error);
            }
        };
        self.consumed = end + 1;
        self.scanned = self.consumed;
        Ok(Some(message))
    }

    pub fn finish(self) -> io::Result<()> {
        if self.failed || self.consumed != self.buffer.len() {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "MCP stream ended before its final frame",
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_unicode_and_adjacent_frames_preserve_exact_ids() {
        let frames = "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"tools/call\",\"params\":{\"text\":\"日本 😀\\n\"}}\r\n{\"jsonrpc\":\"2.0\",\"id\":18446744073709551615,\"result\":{}}\n";
        let mut decoder = Decoder::default();
        let mut messages = Vec::new();
        for byte in frames.as_bytes() {
            decoder.push(std::slice::from_ref(byte)).unwrap();
            while let Some(message) = decoder.next_message().unwrap() {
                messages.push(message);
            }
        }
        decoder.finish().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].kind(), Kind::Request);
        assert_eq!(messages[0].id(), Some(&json!("0")));
        assert_eq!(messages[0].params().unwrap()["text"], "日本 😀\n");
        assert_eq!(messages[1].id(), Some(&json!(u64::MAX)));
    }

    #[test]
    fn malformed_envelopes_and_duplicate_keys_do_not_resynchronize() {
        for input in [
            "[]",
            "{}",
            "{\"jsonrpc\":\"1.0\",\"method\":\"ping\"}",
            "{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"ping\"}",
            "{\"jsonrpc\":\"2.0\",\"id\":1.5,\"method\":\"ping\"}",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"id\":2,\"method\":\"ping\"}",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{},\"error\":{\"code\":-1,\"message\":\"bad\"}}",
        ] {
            let mut decoder = Decoder::default();
            decoder
                .push(format!("{input}\n{{\"jsonrpc\":\"2.0\",\"method\":\"ping\"}}\n").as_bytes())
                .unwrap();
            assert!(decoder.next_message().is_err(), "{input}");
            assert!(decoder.next_message().is_err());
            assert!(decoder.finish().is_err());
        }
    }

    #[test]
    fn eof_and_unterminated_flood_are_bounded() {
        assert!(Decoder::default().finish().is_ok());
        let mut decoder = Decoder::default();
        decoder.push(b"{\"jsonrpc\":\"2.0\"").unwrap();
        assert!(decoder.finish().is_err());
        let mut flood = Decoder::default();
        let block = vec![b' '; READ_CHUNK];
        for _ in 0..MAX_FRAME / READ_CHUNK {
            flood.push(&block).unwrap();
            assert!(flood.next_message().unwrap().is_none());
        }
        flood.push(b"xx").unwrap();
        assert!(flood.next_message().is_err());
    }

    #[test]
    fn output_is_one_json_frame_and_error_ids_remain_optional() {
        let output = Message::result(&json!("opaque"), json!({"text":"line\n日本"}))
            .unwrap()
            .encode()
            .unwrap();
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        let error = Message::error(None, -32700, "Parse error", None).unwrap();
        assert_eq!(error.kind(), Kind::Error);
        assert_eq!(error.id(), None);
        assert!(Message::result(&Value::Null, json!({})).is_err());
        let notification = Message::parse(br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"opaque"}}"#).unwrap();
        assert_eq!(notification.kind(), Kind::Notification);
    }
}
