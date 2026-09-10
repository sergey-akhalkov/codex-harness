//! Explicit native CBM MCP connections with optional owned shared broker routing.
#![cfg(windows)]

use crate::{
    cbm_catalogue, cbm_index,
    mcp_session::{Operation, Session},
    mcp_stdio,
    process::{Cancellation, Deadline},
};
use serde_json::{Value, json};
use std::{fs::File, io, path::PathBuf};

/// Every path is selected by the connection owner. The catalogue is explicit
/// local configuration; its claimed artifact digest does not certify origin.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub executable: PathBuf,
    pub cache: PathBuf,
    pub runtime: PathBuf,
    pub account: PathBuf,
    pub catalogue: PathBuf,
}

impl Configuration {
    pub(crate) fn call(&self, operation: Operation) -> io::Result<Value> {
        if operation.cancellation.is_cancelled() || operation.deadline.expired() {
            // Nothing started. The session suppresses a cancelled response or
            // emits its deadline error when completing this slot.
            return Ok(json!({"content":[],"isError":true}));
        }
        let report = if operation.name == "index_repository" {
            cbm_index::index(
                &self.executable,
                &self.cache,
                &self.runtime,
                &self.account,
                &operation.arguments,
                &operation.cancellation,
            )
        } else {
            cbm_index::call(
                &self.executable,
                &self.cache,
                &operation.name,
                &operation.arguments,
                &operation.cancellation,
            )
        };
        let report = match report {
            Ok(report) => report,
            Err(error)
                if operation.cancellation.is_cancelled()
                    && cbm_index::failure_reclaimed(&error) =>
            {
                return Ok(json!({"content":[],"isError":true}));
            }
            Err(error) if cbm_index::resource_failure_message(&error).is_some() => {
                return Ok(json!({
                    "content":[{"type":"text","text":cbm_index::resource_failure_message(&error)}],
                    "isError":true
                }));
            }
            Err(error) => return Err(error),
        };
        if report["owned_tree_stopped"] != true || report["temporary_state_removed"] != true {
            return Err(io::Error::other("CBM operation cleanup was not confirmed"));
        }
        report
            .get("result")
            .cloned()
            .ok_or_else(|| io::Error::other("CBM tool result unavailable"))
    }
}

/// Handshake/catalogue reads never launch CBM. A tool operation separately
/// verifies the audited executable and policy, then uses owned native execution.
/// Infrastructure errors terminate this connection; upstream tool errors remain
/// normal CallToolResult values. There is no retry or daemon adoption here.
pub fn serve(
    configuration: Configuration,
    input: File,
    output: File,
    cancellation: &Cancellation,
    deadline: Deadline,
) -> io::Result<()> {
    let definitions = cbm_catalogue::read(&configuration.catalogue)?;
    let session = Session::new("codex-harness-codebase-memory", definitions)?;
    mcp_stdio::serve_fallible(
        session,
        input,
        output,
        cancellation,
        deadline,
        move |operation| configuration.call(operation),
    )
}

/// Same local handshake and queue semantics, with resource-confirmed shared
/// requests. The owner supplies a newly prepared private broker root explicitly.
pub fn serve_shared(
    configuration: Configuration,
    broker_root: PathBuf,
    input: File,
    output: File,
    cancellation: &Cancellation,
    deadline: Deadline,
) -> io::Result<()> {
    let definitions = cbm_catalogue::read(&configuration.catalogue)?;
    let session = Session::new("codex-harness-codebase-memory", definitions)?;
    let client = crate::cbm_broker::Client::new(configuration, broker_root)?;
    mcp_stdio::serve_fallible(
        session,
        input,
        output,
        cancellation,
        deadline,
        move |operation| client.call(operation),
    )
}
