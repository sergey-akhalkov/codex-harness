//! Native stdio proxy for the shared Serena broker.
//!
//! Ports the seam's proxy: one lazily connected client per stdio connection
//! forwards JSON-RPC requests through the authenticated shared broker, keeps
//! its own cached route, filters memory/onboarding/configuration tools from
//! tools/list (unless explicitly unfiltered) and reports catalogue changes.
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

/// Native Git records own project memory; onboarding and configuration
/// introspection are not part of the managed model-facing surface.
pub const HIDDEN_TOOLS: [&str; 9] = [
    "onboarding",
    "initial_instructions",
    "get_current_config",
    "list_memories",
    "read_memory",
    "write_memory",
    "edit_memory",
    "delete_memory",
    "rename_memory",
];

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub fn unfiltered() -> bool {
    std::env::var_os("HARNESS_SERENA_UNFILTERED").is_some_and(|value| value == "1")
}

/// Filter the managed tool catalogue for a tools/list result.
pub fn exposed_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter(|tool| {
            tool["name"]
                .as_str()
                .is_none_or(|name| !HIDDEN_TOOLS.contains(&name))
        })
        .cloned()
        .collect()
}

/// Advertise catalogue changes when the worker exposes a tools capability, so
/// filtered additions and removals surface without a reconnect.
pub fn with_list_changed(message: Value) -> Value {
    let mut message = message;
    if message["result"]["capabilities"]["tools"].is_object() {
        message["result"]["capabilities"]["tools"]["listChanged"] = json!(true);
    }
    message
}

fn filter_catalogue(message: Value) -> Value {
    let mut message = message;
    if let Some(tools) = message["result"]["tools"].as_array() {
        message["result"]["tools"] = Value::Array(exposed_tools(tools));
    }
    message
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
            let mut message = response["message"].clone();
            if method == "tools/list" && !unfiltered() {
                message = filter_catalogue(message);
            }
            Ok(message)
        })();
        match result {
            Ok(response) => Ok(response),
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
    fn memory_and_onboarding_tools_stay_hidden() {
        let tools = vec![
            json!({"name": "find_symbol"}),
            json!({"name": "onboarding"}),
            json!({"name": "list_memories"}),
            json!({"name": "read_memory"}),
            json!({"name": "get_symbols_overview"}),
        ];
        let exposed = exposed_tools(&tools);
        let names: Vec<_> = exposed
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(names, ["find_symbol", "get_symbols_overview"]);
    }

    #[test]
    fn tools_capability_advertises_catalogue_changes() {
        let message = with_list_changed(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"serverInfo": {"name": "Serena"}, "capabilities": {"tools": {}}},
        }));
        assert_eq!(
            message["result"]["capabilities"]["tools"]["listChanged"],
            true
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
    fn catalogue_filtering_keeps_other_results_intact() {
        let message = filter_catalogue(json!({
            "jsonrpc": "2.0",
            "id": 4,
            "result": {"tools": [
                {"name": "find_symbol"},
                {"name": "initial_instructions"},
            ]},
        }));
        assert_eq!(message["result"]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(message["result"]["tools"][0]["name"], "find_symbol");
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
