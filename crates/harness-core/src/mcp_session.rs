//! Serial tool scheduling for a single MCP stdio connection. Transport and owned
//! process cleanup remain the caller's responsibility; cancellation never frees
//! the active slot until `complete` confirms that cleanup has finished.
#![cfg(windows)]

use crate::{
    mcp_protocol::{Kind, Message},
    process::{Cancellation, Deadline},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, VecDeque},
    io,
    time::Duration,
};

const QUEUE_LIMIT: usize = 64;
const ARGUMENT_LIMIT: usize = 16 * 1024;
const PROTOCOL: &str = "2024-11-05";

#[derive(PartialEq, Eq)]
enum Phase {
    New,
    Initializing,
    Ready,
    Closed,
}

/// A started request owns a separate cancellation token. Its deadline starts
/// when the request enters the session, including time spent waiting in queue.
pub struct Operation {
    pub id: Value,
    pub name: String,
    pub arguments: Value,
    pub deadline: Deadline,
    pub cancellation: Cancellation,
}

struct Active {
    id: Value,
    deadline: Deadline,
    cancellation: Cancellation,
    answered: bool,
}

pub struct Session {
    name: String,
    instructions: Option<String>,
    definitions: Vec<Value>,
    names: BTreeSet<String>,
    phase: Phase,
    queue: VecDeque<Operation>,
    active: Option<Active>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl Session {
    pub fn new(name: &str, definitions: Vec<Value>) -> io::Result<Self> {
        if name.is_empty() || name.len() > 128 || definitions.is_empty() || definitions.len() > 256
        {
            return Err(invalid("invalid MCP server catalogue"));
        }
        let mut names = BTreeSet::new();
        for definition in &definitions {
            let name = definition["name"]
                .as_str()
                .filter(|name| !name.is_empty() && name.len() <= 128)
                .ok_or_else(|| invalid("invalid MCP tool name"))?;
            if !definition["inputSchema"].is_object() || !names.insert(name.to_owned()) {
                return Err(invalid("invalid or duplicate MCP tool definition"));
            }
        }
        // Refuse an unencodable catalogue before accepting any client requests.
        Message::result(&json!(0), json!({"tools":definitions}))?.encode()?;
        Ok(Self {
            name: name.into(),
            instructions: None,
            definitions,
            names,
            phase: Phase::New,
            queue: VecDeque::new(),
            active: None,
        })
    }

    /// Provider-specific, compact instructions replace upstream guidance only
    /// for the explicitly selected managed endpoint.
    pub fn with_instructions(mut self, instructions: &str) -> io::Result<Self> {
        if instructions.len() > 4096 {
            return Err(invalid("MCP instructions exceed the byte limit"));
        }
        self.instructions = Some(instructions.into());
        Ok(self)
    }

    /// Accept one validated envelope. Errors here terminate the connection;
    /// recoverable method/parameter failures are returned as JSON-RPC replies.
    pub fn receive(&mut self, message: Message) -> io::Result<Option<Message>> {
        if self.phase == Phase::Closed {
            return Err(invalid("MCP session already closed"));
        }
        if message.kind() == Kind::Notification {
            match message.method() {
                Some("notifications/initialized") if self.phase == Phase::Initializing => {
                    self.phase = Phase::Ready
                }
                Some("notifications/cancelled") => {
                    if let Some(id) = message.params().and_then(|params| params.get("requestId")) {
                        // Malformed/unknown/completed cancellation is ignored.
                        if (id.is_string() || id.is_i64() || id.is_u64())
                            && message
                                .params()
                                .and_then(|params| params.get("reason"))
                                .is_none_or(Value::is_string)
                        {
                            self.queue.retain(|operation| operation.id != *id);
                            if let Some(active) = &mut self.active
                                && active.id == *id
                            {
                                active.cancellation.cancel();
                                active.answered = true;
                            }
                        }
                    }
                }
                _ => {}
            }
            return Ok(None);
        }
        if message.kind() != Kind::Request {
            return Err(invalid("unexpected MCP client response"));
        }
        if serde_json::to_vec(message.value())?.len() > ARGUMENT_LIMIT {
            return Err(invalid("MCP request exceeds the byte limit"));
        }
        let id = message
            .id()
            .ok_or_else(|| invalid("MCP request missing identity"))?;
        if self.active.as_ref().is_some_and(|active| active.id == *id)
            || self.queue.iter().any(|operation| operation.id == *id)
        {
            return Err(invalid("duplicate in-flight MCP request identity"));
        }
        let error = |code, reason| Message::error(Some(id), code, reason, None).map(Some);
        let method = message.method().unwrap_or_default();
        let params = message
            .value()
            .get("params")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if method == "ping" {
            return Message::result(id, json!({})).map(Some);
        }
        if method == "initialize" {
            if self.phase != Phase::New {
                return error(-32600, "MCP session is already initialized");
            }
            if !params["protocolVersion"]
                .as_str()
                .is_some_and(|version| !version.is_empty())
                || !params["capabilities"].is_object()
                || !params["clientInfo"]["name"]
                    .as_str()
                    .is_some_and(|name| !name.is_empty())
                || !params["clientInfo"]["version"].is_string()
            {
                return error(-32602, "Invalid initialize parameters");
            }
            self.phase = Phase::Initializing;
            let mut result = json!({
                "protocolVersion":PROTOCOL,"capabilities":{"tools":{}},
                "serverInfo":{"name":self.name,"version":env!("CARGO_PKG_VERSION")}
            });
            if let Some(instructions) = &self.instructions {
                result["instructions"] = json!(instructions);
            }
            return Message::result(id, result).map(Some);
        }
        if self.phase != Phase::Ready {
            return error(-32000, "MCP initialization is incomplete");
        }
        match method {
            "tools/list" => {
                if params.get("cursor").is_some() {
                    return error(-32602, "This catalogue has no next page");
                }
                Message::result(id, json!({"tools":self.definitions})).map(Some)
            }
            "tools/call" => {
                let Some(name) = params["name"]
                    .as_str()
                    .filter(|name| self.names.contains(*name))
                else {
                    return error(-32602, "Unknown tool name");
                };
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !arguments.is_object()
                    || serde_json::to_vec(&arguments)?.len() > ARGUMENT_LIMIT
                    || params.get("task").is_some()
                {
                    return error(
                        -32602,
                        "Tool arguments are invalid or exceed the request limit",
                    );
                }
                if self.queue.len() >= QUEUE_LIMIT {
                    return error(-32000, "MCP request queue is full");
                }
                self.queue.push_back(Operation {
                    id: id.clone(),
                    name: name.into(),
                    arguments,
                    deadline: Deadline::after(Duration::from_secs(
                        if matches!(
                            name,
                            "index_repository" | "codegraph_index" | "codegraph_sync"
                        ) {
                            600
                        } else {
                            60
                        },
                    ))?,
                    cancellation: Cancellation::default(),
                });
                Ok(None)
            }
            _ => error(-32601, "Method not found"),
        }
    }

    /// Deliver timeout replies and signal active cancellation. The active slot
    /// stays occupied while the caller reclaims its worker and descendants.
    pub fn tick(&mut self) -> io::Result<Vec<Message>> {
        let mut replies = Vec::new();
        if let Some(active) = &mut self.active
            && active.deadline.expired()
            && !active.answered
        {
            active.cancellation.cancel();
            active.answered = true;
            replies.push(Message::error(
                Some(&active.id),
                -32000,
                "Tool request deadline exceeded",
                None,
            )?);
        }
        let mut remaining = VecDeque::new();
        while let Some(operation) = self.queue.pop_front() {
            if operation.deadline.expired() {
                replies.push(Message::error(
                    Some(&operation.id),
                    -32000,
                    "Tool request expired in queue",
                    None,
                )?);
            } else {
                remaining.push_back(operation);
            }
        }
        self.queue = remaining;
        Ok(replies)
    }

    pub fn next_operation(&mut self) -> Option<Operation> {
        if self.phase != Phase::Ready
            || self.active.is_some()
            || self
                .queue
                .front()
                .is_some_and(|operation| operation.deadline.expired())
        {
            return None;
        }
        let operation = self.queue.pop_front()?;
        self.active = Some(Active {
            id: operation.id.clone(),
            deadline: operation.deadline,
            cancellation: operation.cancellation.clone(),
            answered: false,
        });
        Some(operation)
    }

    /// Call only once the owned worker has completed cleanup. Foreign errors
    /// must already have been reduced to a private diagnostic locator by caller.
    pub fn complete(&mut self, id: &Value, result: Value) -> io::Result<Option<Message>> {
        if !self.active.as_ref().is_some_and(|active| active.id == *id) {
            return Err(invalid("MCP completion does not match the active request"));
        }
        let active = self.active.take().unwrap();
        if active.answered || self.phase == Phase::Closed {
            return Ok(None);
        }
        if active.deadline.expired() {
            active.cancellation.cancel();
            return Message::error(Some(id), -32000, "Tool request deadline exceeded", None)
                .map(Some);
        }
        if !result["content"].is_array()
            || result
                .get("isError")
                .is_some_and(|value| !value.is_boolean())
        {
            return Err(invalid("invalid MCP tool result"));
        }
        Message::result(id, result).map(Some)
    }

    pub fn close(&mut self) {
        self.phase = Phase::Closed;
        self.queue.clear();
        if let Some(active) = &mut self.active {
            active.cancellation.cancel();
            active.answered = true;
        }
    }

    pub fn is_idle(&self) -> bool {
        self.active.is_none() && self.queue.is_empty()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(value: Value) -> Message {
        Message::parse(&serde_json::to_vec(&value).unwrap()).unwrap()
    }
    fn call(id: Value) -> Message {
        message(
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"echo","arguments":{"text":"кириллица 日本"}}}),
        )
    }
    fn fresh() -> Session {
        Session::new("owned-test", vec![json!({"name":"echo","description":"unchanged","inputSchema":{"type":"object","properties":{}}})]).unwrap()
    }
    fn ready() -> Session {
        let mut session = fresh();
        let init = session.receive(message(json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}}))).unwrap().unwrap();
        assert_eq!(init.value()["result"]["protocolVersion"], PROTOCOL);
        session
            .receive(message(
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            ))
            .unwrap();
        session
    }
    fn cancel(session: &mut Session, id: Value) {
        assert!(session.receive(message(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}))).unwrap().is_none());
    }

    #[test]
    fn initialization_and_capability_boundaries() {
        let mut session = fresh();
        assert_eq!(
            session.receive(call(json!(1))).unwrap().unwrap().value()["error"]["code"],
            -32000
        );
        assert!(session.is_idle());
        let bad = message(json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}));
        assert_eq!(
            session.receive(bad).unwrap().unwrap().value()["error"]["code"],
            -32602
        );
        let mut session = ready();
        let list = session
            .receive(message(
                json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}),
            ))
            .unwrap()
            .unwrap();
        assert_eq!(
            list.value()["result"]["tools"][0]["description"],
            "unchanged"
        );
        assert_eq!(
            session
                .receive(message(
                    json!({"jsonrpc":"2.0","id":3,"method":"resources/list"})
                ))
                .unwrap()
                .unwrap()
                .value()["error"]["code"],
            -32601
        );
        assert!(session.is_idle());
    }

    #[test]
    fn exact_ids_queue_cancellation_and_cleanup_gate() {
        let mut session = ready();
        session.receive(call(json!(u64::MAX))).unwrap();
        let first = session.next_operation().unwrap();
        session
            .receive(call(json!("18446744073709551615")))
            .unwrap();
        session.receive(call(json!(3))).unwrap();
        cancel(&mut session, json!(3));
        cancel(&mut session, json!(u64::MAX));
        assert!(first.cancellation.is_cancelled());
        assert!(session.next_operation().is_none());
        assert!(!session.is_idle());
        assert!(
            session
                .complete(&first.id, json!({"content":[]}))
                .unwrap()
                .is_none()
        );
        let second = session.next_operation().unwrap();
        assert_eq!(second.id, json!("18446744073709551615"));
        assert_eq!(second.arguments["text"], "кириллица 日本");
        assert!(!second.cancellation.is_cancelled());
        let reply = session
            .complete(
                &second.id,
                json!({"content":[],"isError":true,"structuredContent":{"kept":true}}),
            )
            .unwrap()
            .unwrap();
        assert_eq!(reply.id(), Some(&second.id));
        assert_eq!(reply.value()["result"]["isError"], true);
        assert_eq!(reply.value()["result"]["structuredContent"]["kept"], true);
        assert!(session.is_idle());
    }

    #[test]
    fn bounded_queue_duplicates_and_invalid_cancellation() {
        let mut session = ready();
        session.receive(call(json!(0))).unwrap();
        let active = session.next_operation().unwrap();
        assert!(session.receive(call(json!(0))).is_err());
        for id in 1..=QUEUE_LIMIT {
            assert!(session.receive(call(json!(id))).unwrap().is_none());
        }
        assert_eq!(
            session.receive(call(json!(99))).unwrap().unwrap().value()["error"]["code"],
            -32000
        );
        session.receive(message(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":0,"reason":7}}))).unwrap();
        cancel(&mut session, json!(null));
        cancel(&mut session, json!("0"));
        assert!(!active.cancellation.is_cancelled());
        session.close();
        assert!(active.cancellation.is_cancelled());
        assert!(session.next_operation().is_none());
        assert!(
            session
                .complete(&active.id, json!({"content":[]}))
                .unwrap()
                .is_none()
        );
        assert!(session.is_idle());
    }

    #[test]
    fn expired_work_does_not_start_or_release_an_active_worker() {
        let mut session = ready();
        session.receive(call(json!(1))).unwrap();
        session.queue.front_mut().unwrap().deadline = Deadline::after(Duration::ZERO).unwrap();
        assert!(session.next_operation().is_none());
        assert_eq!(session.tick().unwrap()[0].id(), Some(&json!(1)));
        session.receive(call(json!(2))).unwrap();
        let active = session.next_operation().unwrap();
        session.active.as_mut().unwrap().deadline = Deadline::after(Duration::ZERO).unwrap();
        assert_eq!(session.tick().unwrap()[0].id(), Some(&json!(2)));
        assert!(session.tick().unwrap().is_empty());
        assert!(active.cancellation.is_cancelled());
        session.receive(call(json!(3))).unwrap();
        assert!(session.next_operation().is_none());
        assert!(
            session
                .complete(&active.id, json!({"content":[]}))
                .unwrap()
                .is_none()
        );
        assert_eq!(session.next_operation().unwrap().id, json!(3));
    }
}
