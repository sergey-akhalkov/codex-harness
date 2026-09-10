//! Resource-confirmed requests to an authenticated shared broker.
#![cfg(windows)]

use crate::{
    broker_endpoint::Endpoint,
    broker_http,
    process::{Cancellation, Deadline},
};
use serde_json::{Value, json};
use std::{io, thread, time::Duration};

const CLEANUP: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(20);

fn cancelled() -> Value {
    json!({"content":[], "isError":true})
}

fn exchange(
    endpoint: &Endpoint,
    operation: &str,
    payload: &Value,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Value> {
    broker_http::exchange(
        endpoint.port,
        endpoint.token(),
        operation,
        payload,
        deadline,
        cancel,
    )
}

fn release(
    endpoint: &Endpoint,
    key: &Value,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<()> {
    let response = exchange(endpoint, "request/release", key, deadline, cancel)?;
    if response.get("released").and_then(Value::as_bool).is_none() {
        return Err(io::Error::other("invalid broker release response"));
    }
    Ok(())
}

/// A closed transport alone never confirms reclamation. Reserve before invoke
/// makes even a cancel that wins the admission race definitive: no late invoke
/// can start an absent or completed reservation. A cleanup error is fatal to the
/// caller's MCP session and must not advance its queue.
pub fn invoke(
    endpoint: &Endpoint,
    operation: &str,
    payload: &Value,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Value> {
    if cancel.is_cancelled() || deadline.expired() {
        return Ok(cancelled());
    }
    let reservation = match exchange(endpoint, "request/reserve", &json!({}), deadline, cancel) {
        Ok(value) => value,
        // No invoke was sent, so no backend resource can have been admitted.
        Err(_) if cancel.is_cancelled() || deadline.expired() => return Ok(cancelled()),
        Err(error) => return Err(error),
    };
    let key = reservation
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| io::Error::other("invalid broker reservation response"))?;
    let control = json!({"key":key});
    let result = exchange(
        endpoint,
        "request/invoke",
        &json!({"key":key,"operation":operation,"payload":payload}),
        deadline,
        cancel,
    );
    // The caller's expired/cancelled token must not cancel its cleanup RPCs.
    let cleanup = Deadline::after(CLEANUP)?;
    let cleanup_cancel = Cancellation::default();
    match result {
        Ok(result) => {
            release(endpoint, &control, cleanup, &cleanup_cancel)?;
            Ok(result)
        }
        Err(error) => {
            loop {
                let response = exchange(
                    endpoint,
                    "request/cancel",
                    &control,
                    cleanup,
                    &cleanup_cancel,
                )?;
                match response.get("reclaimed").and_then(Value::as_bool) {
                    Some(true) => break,
                    Some(false) if !cleanup.expired() => {
                        thread::sleep(POLL.min(cleanup.remaining()))
                    }
                    Some(false) => {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "broker cleanup was not confirmed",
                        ));
                    }
                    None => return Err(io::Error::other("invalid broker cancellation response")),
                }
            }
            release(endpoint, &control, cleanup, &cleanup_cancel)?;
            if cancel.is_cancelled() || deadline.expired() {
                Ok(cancelled())
            } else {
                Err(error)
            }
        }
    }
}
