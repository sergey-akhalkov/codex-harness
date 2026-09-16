//! Bounded live probe: subscription Grok over Codex-shaped Responses.
//! Uses existing harness xAI OAuth. Does not copy OpenCode tokens or
//! require a pay-as-you-go key. Access tokens stay out of reports.
#![cfg(windows)]

use crate::{
    native_build,
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const RESPONSES_URL: &str = "https://api.x.ai/v1/responses";
const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const MODEL: &str = "grok-4.6";
const MAX_BODY: usize = 256 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub ok: bool,
    pub endpoint: String,
    pub requested_model: String,
    pub http_status: Option<u16>,
    pub reported_model: Option<String>,
    pub tools: bool,
    pub streaming: bool,
    pub grant: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProbeRequest {
    pub user_home: PathBuf,
    pub evidence: PathBuf,
    pub transport: ProbeTransport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeTransport {
    Live,
}

fn other(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(secret, "[redacted]");
        }
    }
    out
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn curl_exe() -> io::Result<PathBuf> {
    let root = std::env::var_os("SystemRoot")
        .ok_or_else(|| other("Windows system tools are unavailable"))?;
    Ok(PathBuf::from(root).join("System32/curl.exe"))
}

fn write_limited(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_BODY {
        return Err(other("probe payload exceeded the size limit"));
    }
    fs::write(path, bytes)
}

fn read_limited(path: &Path) -> io::Result<Vec<u8>> {
    let bytes = fs::read(path)?;
    if bytes.len() > MAX_BODY {
        return Err(other("probe response exceeded the size limit"));
    }
    Ok(bytes)
}

struct CurlResult {
    status: u16,
    body: Vec<u8>,
    stderr: String,
    exit_code: i32,
}

fn https_request(
    url: &str,
    method: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<CurlResult> {
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "probe cancelled",
        ));
    }
    if deadline.expired() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "probe deadline expired",
        ));
    }
    let timeout = deadline.remaining().as_secs().max(1);
    let private = tempfile::Builder::new()
        .prefix("harness-xai-probe-")
        .tempdir()?;
    let body_path = private.path().join("body");
    let stderr_path = private.path().join("stderr");
    let stdout_path = private.path().join("stdout");
    let upload_path = private.path().join("upload");
    if let Some(bytes) = body {
        write_limited(&upload_path, bytes)?;
    }
    let mut command = CommandSpec::new(curl_exe()?);
    command.current_dir = Some(private.path().to_path_buf());
    let timeout_text = timeout.to_string();
    command.args.extend(
        [
            "--disable",
            "--globoff",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "0",
            "--max-time",
            timeout_text.as_str(),
            "--connect-timeout",
            "10",
            "--max-filesize",
            "262144",
            "--silent",
            "--show-error",
            "--disallow-username-in-url",
            "--output",
        ]
        .into_iter()
        .map(Into::into),
    );
    command.args.push(body_path.clone().into());
    command
        .args
        .extend(["--write-out".into(), "%{http_code}".into()]);
    command.args.extend(["--request".into(), method.into()]);
    for (name, value) in headers {
        command
            .args
            .extend(["--header".into(), format!("{name}: {value}").into()]);
    }
    if body.is_some() {
        command.args.extend([
            "--data-binary".into(),
            format!("@{}", upload_path.display()).into(),
        ]);
    }
    command.args.extend(["--url".into(), url.into()]);
    let outcome = native_build::invoke_management(
        command,
        &stderr_path,
        Some(&stdout_path),
        deadline.remaining(),
    )?;
    let status_text = fs::read_to_string(&stdout_path).unwrap_or_default();
    let status = status_text.trim().parse::<u16>().unwrap_or(0);
    let stderr =
        String::from_utf8_lossy(&read_limited(&stderr_path).unwrap_or_default()).into_owned();
    Ok(CurlResult {
        status,
        body: read_limited(&body_path).unwrap_or_default(),
        stderr,
        exit_code: if outcome.reason == StopReason::Exited {
            outcome.exit_code as i32
        } else {
            1
        },
    })
}

fn auth_store(user_home: &Path) -> PathBuf {
    let harness = user_home
        .join(".codex")
        .join("harness/subscriptions/xai-oauth.json");
    if harness.is_file() {
        harness
    } else {
        user_home.join(".opencodex").join("auth.json")
    }
}

fn load_credential(user_home: &Path) -> io::Result<(String, String, u64, String)> {
    let path = auth_store(user_home);
    let value: Value = serde_json::from_slice(&fs::read(&path).map_err(|_| {
        invalid("harness xAI OAuth store is missing; run subscription-login xai first")
    })?)?;
    let account = value
        .pointer("/xai/accounts/0/credential")
        .ok_or_else(|| invalid("harness xAI OAuth store has no xAI credential"))?;
    let access = account
        .get("access")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("harness xAI OAuth store is missing an access token"))?
        .to_owned();
    let refresh = account
        .get("refresh")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("harness xAI OAuth store is missing a refresh token"))?
        .to_owned();
    let expires = account.get("expires").and_then(Value::as_u64).unwrap_or(0);
    let source = account
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("oauth")
        .to_owned();
    let grant = match source.as_str() {
        "local-cli" => "imported-grok-cli",
        _ => "browser-oauth",
    };
    Ok((access, refresh, expires, grant.to_owned()))
}

fn refresh_access(refresh: &str, deadline: Deadline, cancel: &Cancellation) -> io::Result<String> {
    let body = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}",
        CLIENT_ID,
        urlencoding(refresh)
    );
    let result = https_request(
        TOKEN_URL,
        "POST",
        &[
            ("Accept", "application/json"),
            ("Content-Type", "application/x-www-form-urlencoded"),
        ],
        Some(body.as_bytes()),
        deadline,
        cancel,
    )?;
    if result.status != 200 {
        return Err(other(format!(
            "xAI token refresh failed ({})",
            result.status
        )));
    }
    let payload: Value = serde_json::from_slice(&result.body)
        .map_err(|_| other("xAI token refresh was not JSON"))?;
    payload
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| other("xAI token refresh omitted an access token"))
}

fn urlencoding(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn responses_payload() -> Value {
    json!({
        "model": MODEL,
        "input": [
            {
                "role": "user",
                "content": [{"type": "input_text", "text": "Call the echo tool with text ping, then stop."}]
            }
        ],
        "tools": [{
            "type": "function",
            "name": "echo",
            "description": "Return the provided text.",
            "parameters": {
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "required": ["text"],
                "additionalProperties": false
            }
        }],
        "tool_choice": "required",
        "stream": false,
        "max_output_tokens": 64
    })
}

fn report_from_response(status: u16, body: &[u8], grant: &str) -> ProbeReport {
    let parsed = serde_json::from_slice::<Value>(body).ok();
    let reported_model = parsed.as_ref().and_then(|value| {
        value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let tools = parsed
        .as_ref()
        .and_then(|value| value.get("output"))
        .and_then(Value::as_array)
        .map(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("function_call")
                    || item.get("type").and_then(Value::as_str) == Some("tool_call")
                    || item.get("name").and_then(Value::as_str) == Some("echo")
            })
        })
        .unwrap_or(false);
    let error = if status == 200 {
        None
    } else {
        parsed
            .as_ref()
            .and_then(|value| {
                value
                    .pointer("/error/code")
                    .or_else(|| value.pointer("/error/type"))
                    .or_else(|| value.get("error"))
            })
            .and_then(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| Some(value.to_string()))
            })
    };
    ProbeReport {
        ok: status == 200 && tools,
        endpoint: RESPONSES_URL.to_owned(),
        requested_model: MODEL.to_owned(),
        http_status: Some(status),
        reported_model,
        tools,
        streaming: false,
        grant: grant.to_owned(),
        error,
    }
}

pub fn probe(request: &ProbeRequest) -> io::Result<ProbeReport> {
    fs::create_dir_all(&request.evidence)?;
    let cancel = Cancellation::default();
    let deadline = Deadline::after(PROBE_TIMEOUT)?;
    let (mut access, refresh, expires, grant) = load_credential(&request.user_home)?;
    if expires > 0 && expires <= now_ms() + 30_000 {
        let refreshed = refresh_access(&refresh, deadline, &cancel).map_err(|error| {
            other(redact(
                &error.to_string(),
                &[access.as_str(), refresh.as_str()],
            ))
        })?;
        access = refreshed;
    }
    let secrets = [access.clone(), refresh.clone()];
    let secret_refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
    let payload = serde_json::to_vec(&responses_payload())?;
    let auth = format!("Bearer {access}");
    let result = https_request(
        RESPONSES_URL,
        "POST",
        &[
            ("Accept", "application/json"),
            ("Authorization", auth.as_str()),
            ("Content-Type", "application/json"),
        ],
        Some(&payload),
        deadline,
        &cancel,
    )
    .map_err(|error| other(redact(&error.to_string(), &secret_refs)))?;
    drop(auth);
    drop(access);
    let report = report_from_response(result.status, &result.body, &grant);
    let receipt = json!({
        "ok": report.ok,
        "endpoint": report.endpoint,
        "requestedModel": report.requested_model,
        "httpStatus": report.http_status,
        "reportedModel": report.reported_model,
        "tools": report.tools,
        "streaming": report.streaming,
        "grant": report.grant,
        "error": report.error,
        "curlExit": result.exit_code,
        "stderrPresent": !result.stderr.trim().is_empty()
    });
    let encoded = serde_json::to_vec_pretty(&receipt)?;
    let text = String::from_utf8_lossy(&encoded);
    if secret_refs.iter().any(|secret| text.contains(secret)) {
        return Err(other("probe receipt contained credential material"));
    }
    let mut file = fs::File::create(request.evidence.join("receipt.json"))?;
    file.write_all(&encoded)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_store_is_explicit() {
        let root = tempfile::tempdir().unwrap();
        let error = load_credential(root.path()).unwrap_err();
        assert!(error.to_string().contains("missing"));
    }

    #[test]
    fn report_detects_tool_call_and_omits_tokens() {
        let body = serde_json::to_vec(&json!({
            "model": "grok-4.6",
            "output": [{"type": "function_call", "name": "echo", "arguments": "{\"text\":\"ping\"}"}]
        })).unwrap();
        let report = report_from_response(200, &body, "browser-oauth");
        assert!(report.ok);
        assert_eq!(report.reported_model.as_deref(), Some("grok-4.6"));
        assert!(report.tools);
        assert!(!report.streaming);
        let encoded = serde_json::to_string(&report).unwrap();
        assert!(!encoded.contains("Bearer"));
        assert!(!encoded.contains("eyJ"));
    }

    #[test]
    fn unsuccessful_status_is_not_ok() {
        let body = serde_json::to_vec(&json!({"error": {"code": "invalid_request"}})).unwrap();
        let report = report_from_response(400, &body, "browser-oauth");
        assert!(!report.ok);
        assert_eq!(report.http_status, Some(400));
        assert_eq!(report.error.as_deref(), Some("invalid_request"));
    }

    #[test]
    fn redact_strips_secrets_from_errors() {
        let text = redact("token abcdef leaked", &["abcdef"]);
        assert!(!text.contains("abcdef"));
        assert!(text.contains("[redacted]"));
    }
}
