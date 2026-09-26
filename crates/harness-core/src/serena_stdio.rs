//! Native stdio proxy for the shared Serena broker.
//!
//! One lazily connected client per stdio connection forwards JSON-RPC
//! requests through the authenticated shared broker and keeps its own cached
//! route. The managed tool selection lives in the generated Serena home
//! (`serena_configuration`), so the worker's own catalogue, guidance and error
//! paths are forwarded unchanged. The broker can serve a live client from a
//! different worker after a project activation or a replaced failed worker, so
//! the proxy still advertises catalogue changes.
//! Notifications are not forwarded, matching the seam; client EOF disconnects
//! from the shared worker pool.
#![cfg(windows)]

use crate::{
    cancellable_pipe::{CancellablePipe, PipeIoError},
    mcp_protocol::{Decoder, Message, READ_CHUNK},
    process::{Cancellation, Deadline},
    serena_broker::{self, Configuration},
};
use serde_json::{Value, json};
use std::{ffi::OsString, fs::File, io, path::Path, time::Duration};

const REQUEST: Duration = Duration::from_secs(240);
const CLEANUP: Duration = Duration::from_secs(5);

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Advertise catalogue changes when the worker exposes a tools capability: the
/// shared broker can move a live client to another worker for its route, and
/// native Serena fixes a connection's catalogue without advertising changes
/// (the worker's own `tools.listChanged` capability is false).
pub fn with_list_changed(message: Value) -> Value {
    let mut message = message;
    if message["result"]["capabilities"]["tools"].is_object() {
        message["result"]["capabilities"]["tools"]["listChanged"] = json!(true);
    }
    message
}

/// The broker answers with the shared worker-session envelope, whose id
/// belongs to the broker's exchange with Serena. The stdio client owns
/// per-connection ids, so every forwarded response must carry the id of the
/// request that opened it; otherwise the MCP client reports a conflicting
/// initialize response id and closes the server.
fn client_response(mut response: Value, id: &Value) -> Value {
    if let Some(object) = response.as_object_mut() {
        object.insert("id".into(), id.clone());
        object.insert("jsonrpc".into(), json!("2.0"));
    }
    response
}

struct Proxy<'a> {
    client: serena_broker::Client,
    arguments: &'a [OsString],
    cwd: &'a Path,
    initialize: Option<Value>,
    route: Option<Value>,
    pending_change: bool,
}

impl<'a> Proxy<'a> {
    fn new(
        configuration: Configuration,
        arguments: &'a [OsString],
        cwd: &'a Path,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<Self> {
        Ok(Self {
            client: serena_broker::Client::new(configuration, deadline, cancellation)?,
            arguments,
            cwd,
            initialize: None,
            route: None,
            pending_change: false,
        })
    }

    fn pending_take(&mut self) -> bool {
        std::mem::take(&mut self.pending_change)
    }

    fn disconnect(&self) -> io::Result<()> {
        self.client.disconnect()
    }

    fn deadline(connection: Deadline) -> io::Result<Deadline> {
        Deadline::after(connection.remaining().min(REQUEST))
    }

    fn handle(
        &mut self,
        message: &Message,
        cancel: &Cancellation,
        connection: Deadline,
    ) -> io::Result<Value> {
        let id = message
            .id()
            .cloned()
            .ok_or_else(|| invalid("request without identity"))?;
        let method = message.method().unwrap_or_default().to_owned();
        let params = message
            .params()
            .cloned()
            .map(Value::Object)
            .unwrap_or_else(|| json!({}));
        let result = (|| -> io::Result<Value> {
            if method == "initialize" {
                self.initialize = Some(params);
                let response = self.client.connect(
                    self.arguments,
                    self.cwd,
                    self.initialize.as_ref().expect("just stored"),
                    Self::deadline(connection)?,
                    cancel,
                )?;
                self.route = Some(response["route"].clone());
                return Ok(with_list_changed(response["message"].clone()));
            }
            let Some(initialize) = self.initialize.clone() else {
                return Err(io::Error::other("Serena client must initialize first"));
            };
            let route = self.route.clone().unwrap_or_else(|| json!({}));
            let response = self.client.rpc(
                &method,
                &params,
                &route,
                &initialize,
                Self::deadline(connection)?,
                cancel,
            )?;
            self.route = Some(response["route"].clone());
            if response["tools_changed"] == true {
                self.pending_change = true;
            }
            Ok(response["message"].clone())
        })();
        match result {
            Ok(response) => Ok(client_response(response, &id)),
            Err(error) => Ok(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32603, "message": error.to_string()},
            })),
        }
    }
}

/// Serve one stdio connection as a shared-broker proxy. The connection
/// deadline is an explicit maximum chosen by its owner.
pub fn serve(
    configuration: Configuration,
    arguments: Vec<OsString>,
    input: File,
    output: File,
    cancellation: &Cancellation,
    deadline: Deadline,
) -> io::Result<()> {
    let cwd = std::env::current_dir()?;
    let stop = Cancellation::default();
    let mut reader = CancellablePipe::reader(input, stop.clone())?;
    let mut writer = CancellablePipe::writer(output, stop.clone())?;
    let mut proxy = Proxy::new(configuration, &arguments, &cwd, deadline, cancellation)?;
    let mut decoder = Decoder::default();
    let result = (|| -> io::Result<()> {
        loop {
            if cancellation.is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Serena proxy cancelled",
                ));
            }
            if deadline.expired() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Serena proxy deadline expired",
                ));
            }
            if proxy.pending_take() {
                let notice = Message::parse(&serde_json::to_vec(&json!({
                    "jsonrpc":"2.0",
                    "method":"notifications/tools/list_changed"
                }))?)?;
                emit(&mut writer, &notice, &stop, deadline)?;
            }
            while let Some(message) = decoder.next_message()? {
                if message.id().is_none() {
                    // Notifications are not forwarded, matching the seam.
                    continue;
                }
                let response = proxy.handle(&message, cancellation, deadline)?;
                let encoded = serde_json::to_vec(&response)?;
                let parsed = Message::parse(&encoded)?;
                emit(&mut writer, &parsed, &stop, deadline)?;
            }
            match reader.read(READ_CHUNK, deadline, cancellation) {
                Ok(bytes) => {
                    if bytes.is_empty() {
                        return Ok(());
                    }
                    decoder.push(&bytes)?;
                }
                Err(PipeIoError::EndOfFile) => return Ok(()),
                Err(PipeIoError::Cancelled {
                    worker_joined: true,
                }) if cancellation.is_cancelled() => {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Serena proxy cancelled",
                    ));
                }
                Err(PipeIoError::DeadlineExpired { .. }) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Serena proxy deadline expired",
                    ));
                }
                Err(error) => return Err(error.into()),
            }
        }
    })();
    let _ = proxy.disconnect();
    stop.cancel();
    let cleanup = Deadline::after(CLEANUP)?;
    let output_cleanup = writer.close(cleanup).map_err(io::Error::from);
    let input_cleanup = reader.close(cleanup).map_err(io::Error::from);
    let mut errors = Vec::new();
    if let Err(error) = output_cleanup {
        errors.push(format!("output: {error}"));
    }
    if let Err(error) = input_cleanup {
        errors.push(format!("input: {error}"));
    }
    if errors.is_empty() {
        return result;
    }
    let combined = errors.join("; ");
    match result {
        Ok(()) => Err(io::Error::other(combined)),
        Err(primary) => Err(io::Error::new(
            primary.kind(),
            format!("{primary}; {combined}"),
        )),
    }
}

fn emit(
    pipe: &mut CancellablePipe,
    message: &Message,
    stop: &Cancellation,
    connection: Deadline,
) -> io::Result<()> {
    let deadline = Deadline::after(CLEANUP.min(connection.remaining()))?;
    let bytes = message.encode()?;
    for chunk in bytes.chunks(READ_CHUNK) {
        pipe.write_all(chunk, deadline, stop)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::serena_route::Route;
    use serde_json::json;

    #[test]
    fn initialize_envelope_passes_through_with_only_the_capability_change() {
        let message = with_list_changed(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "serverInfo": {"name": "Serena"},
                "capabilities": {"tools": {}},
                "instructions": "Worker-authored guidance stays untouched.",
            },
        }));
        assert_eq!(
            message["result"]["capabilities"]["tools"]["listChanged"],
            true
        );
        assert_eq!(
            message["result"]["instructions"],
            "Worker-authored guidance stays untouched."
        );
        let without = with_list_changed(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {"capabilities": {"tools": {}}},
        }));
        assert_eq!(
            without["result"]["capabilities"]["tools"]["listChanged"],
            true
        );
        let bare = with_list_changed(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {"capabilities": {}},
        }));
        assert!(bare["result"]["capabilities"]["tools"].is_null());
    }

    #[test]
    fn forwarded_responses_carry_the_client_request_id() {
        let worker_envelope = json!({
            "id": 1,
            "jsonrpc": "2.0",
            "result": {"serverInfo": {"name": "Serena"}},
        });
        let rewritten = client_response(worker_envelope, &json!(0));
        assert_eq!(rewritten["id"], 0);
        assert_eq!(rewritten["jsonrpc"], "2.0");
        assert_eq!(rewritten["result"]["serverInfo"]["name"], "Serena");
        let named = client_response(
            json!({"id": 7, "jsonrpc": "2.0", "result": {}}),
            &json!("opaque"),
        );
        assert_eq!(named["id"], "opaque");
    }

    #[test]
    fn route_echo_round_trips_through_the_broker_shape() {
        let route = Route {
            project: Some(Path::new("D:/proj/repo").to_path_buf()),
            cwd: Path::new("D:/elsewhere").to_path_buf(),
            arguments: vec![
                "start-mcp-server".into(),
                "--context".into(),
                "codex".into(),
            ],
            removed_projects: vec!["legacy".into()],
            mutation_owner: Some("0123456789abcdef0123456789abcdef".into()),
        };
        let echo = route.to_json();
        assert_eq!(Route::from_json(&echo).unwrap(), route);
        assert!(Route::from_json(&json!({"project": null})).is_err());
        assert!(
            Route::from_json(&json!({
                "project": null,
                "cwd": "D:/x",
                "arguments": [],
                "removed_projects": [],
                "mutation_owner": null,
                "extra": true,
            }))
            .is_ok()
        );
    }
}
