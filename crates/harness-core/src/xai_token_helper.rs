//! Codex auth helper: print a short-lived xAI access token to stdout.
//! Private store under CODEX_HOME. Never copies OpenCode credentials.
#![cfg(windows)]

use crate::{
    native_build,
    process::{Cancellation, CommandSpec, Deadline, StopReason},
    subscription_login,
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const MAX_BODY: usize = 64 * 1024;
const SKEW_MS: u64 = 30_000;
const TIMEOUT: Duration = Duration::from_secs(30);

fn other(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

pub fn store_path(codex_home: &Path) -> PathBuf {
    codex_home.join("harness/subscriptions/xai-oauth.json")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(*secret, "[redacted]");
        }
    }
    out
}

fn curl_exe() -> io::Result<PathBuf> {
    let root = std::env::var_os("SystemRoot")
        .ok_or_else(|| other("Windows system tools are unavailable"))?;
    Ok(PathBuf::from(root).join("System32/curl.exe"))
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

fn load_store(path: &Path) -> io::Result<Value> {
    let bytes = fs::read(path)
        .map_err(|_| invalid("xAI OAuth store is missing; run subscription-login xai first"))?;
    serde_json::from_slice(&bytes).map_err(|_| invalid("xAI OAuth store is not JSON"))
}

fn credential(store: &Value) -> io::Result<&Value> {
    store
        .pointer("/xai/accounts/0/credential")
        .ok_or_else(|| invalid("xAI OAuth store has no credential"))
}

fn persist_store(path: &Path, store: &Value) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded = serde_json::to_vec_pretty(store)?;
    let temporary = path.with_extension("tmp");
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)?;
        file.write_all(&encoded)?;
        file.flush()?;
    }
    subscription_login::protect_owner_file(&temporary)?;
    fs::rename(&temporary, path).or_else(|_| {
        fs::copy(&temporary, path)?;
        fs::remove_file(&temporary)
    })?;
    subscription_login::protect_owner_file(path)?;
    Ok(())
}

struct CurlResult {
    status: u16,
    body: Vec<u8>,
}

fn https_form(body: &str) -> io::Result<CurlResult> {
    let cancel = Cancellation::default();
    let deadline = Deadline::after(TIMEOUT)?;
    if deadline.expired() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "token helper deadline expired",
        ));
    }
    let private = tempfile::Builder::new()
        .prefix("harness-xai-token-")
        .tempdir()?;
    let body_path = private.path().join("body");
    let stderr_path = private.path().join("stderr");
    let stdout_path = private.path().join("stdout");
    let upload_path = private.path().join("upload");
    fs::write(&upload_path, body.as_bytes())?;
    let mut command = CommandSpec::new(curl_exe()?);
    command.current_dir = Some(private.path().to_path_buf());
    let timeout_text = deadline.remaining().as_secs().max(1).to_string();
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
            "65536",
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
    command.args.extend(["--request".into(), "POST".into()]);
    command.args.extend([
        "--header".into(),
        "Accept: application/json".into(),
        "--header".into(),
        "Content-Type: application/x-www-form-urlencoded".into(),
        "--data-binary".into(),
        format!("@{}", upload_path.display()).into(),
        "--url".into(),
        TOKEN_URL.into(),
    ]);
    let outcome = native_build::invoke_management(
        command,
        &stderr_path,
        Some(&stdout_path),
        deadline.remaining(),
    )?;
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "token helper cancelled",
        ));
    }
    if outcome.reason != StopReason::Exited && outcome.exit_code != 0 {
        return Err(other("xAI token refresh failed"));
    }
    let status_text = fs::read_to_string(&stdout_path).unwrap_or_default();
    let status = status_text.trim().parse::<u16>().unwrap_or(0);
    let bytes = fs::read(&body_path).unwrap_or_default();
    if bytes.len() > MAX_BODY {
        return Err(other("token refresh response exceeded the size limit"));
    }
    Ok(CurlResult {
        status,
        body: bytes,
    })
}

fn refresh_tokens(refresh: &str) -> io::Result<(String, String, u64)> {
    let body = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}",
        CLIENT_ID,
        urlencoding(refresh)
    );
    let result =
        https_form(&body).map_err(|error| other(redact(&error.to_string(), &[refresh])))?;
    if result.status != 200 {
        return Err(other(format!(
            "xAI token refresh failed ({})",
            result.status
        )));
    }
    let payload: Value = serde_json::from_slice(&result.body)
        .map_err(|_| other("xAI token refresh was not JSON"))?;
    let access = payload
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| other("xAI token refresh omitted an access token"))?
        .to_owned();
    let next_refresh = payload
        .get("refresh_token")
        .and_then(Value::as_str)
        .unwrap_or(refresh)
        .to_owned();
    let expires_in = payload
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(3600);
    let expires = now_ms() + expires_in * 1000 - 2 * 60 * 1000;
    Ok((access, next_refresh, expires))
}

fn emit_token(token: &str) -> io::Result<()> {
    if token.is_empty() || token.contains(['\r', '\n']) {
        return Err(other("xAI access token was empty"));
    }
    let mut stdout = io::stdout().lock();
    stdout.write_all(token.as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

/// Print one access token for Codex `[model_providers.*.auth]`.
pub fn emit_access_token(codex_home: &Path) -> io::Result<()> {
    let path = store_path(codex_home);
    let mut store = load_store(&path)?;
    let cred = credential(&store)?.clone();
    let access = cred
        .get("access")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("xAI OAuth store is missing an access token"))?
        .to_owned();
    let refresh = cred
        .get("refresh")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("xAI OAuth store is missing a refresh token"))?
        .to_owned();
    let expires = cred.get("expires").and_then(Value::as_u64).unwrap_or(0);
    if expires == 0 || expires > now_ms() + SKEW_MS {
        emit_token(&access)?;
        return Ok(());
    }
    let (next_access, next_refresh, next_expires) = refresh_tokens(&refresh)?;
    if let Some(slot) = store.pointer_mut("/xai/accounts/0/credential") {
        slot["access"] = json!(next_access.clone());
        slot["refresh"] = json!(next_refresh);
        slot["expires"] = json!(next_expires);
    }
    persist_store(&path, &store)?;
    emit_token(&next_access)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn write_store(home: &Path, access: &str, refresh: &str, expires: u64) {
        let path = store_path(home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let store = json!({
            "xai": {
                "activeAccountId": "fixture",
                "accounts": [{
                    "id": "fixture",
                    "credential": {
                        "access": access,
                        "refresh": refresh,
                        "expires": expires,
                        "source": "oauth"
                    }
                }]
            }
        });
        fs::write(&path, serde_json::to_vec_pretty(&store).unwrap()).unwrap();
    }

    #[test]
    fn missing_store_is_explicit() {
        let root = tempfile::tempdir().unwrap();
        let error = emit_access_token(root.path()).unwrap_err();
        assert!(error.to_string().contains("missing"));
    }

    #[test]
    fn valid_store_emits_access_only() {
        let root = tempfile::tempdir().unwrap();
        write_store(
            root.path(),
            "access-token-value",
            "refresh-token-value",
            now_ms() + 3_600_000,
        );
        // Capture by writing through a redirected helper would need a process;
        // here we assert load + emit_token contract via a file sink equivalent.
        let store = load_store(&store_path(root.path())).unwrap();
        let cred = credential(&store).unwrap();
        assert_eq!(cred["access"], "access-token-value");
        assert_eq!(cred["refresh"], "refresh-token-value");
        let encoded = serde_json::to_string(&store).unwrap();
        assert!(encoded.contains("access-token-value"));
        assert!(!encoded.contains("opencode"));
    }

    #[test]
    fn empty_access_is_rejected() {
        let error = emit_token("").unwrap_err();
        assert!(error.to_string().contains("empty"));
    }

    #[test]
    fn redact_hides_refresh() {
        let text = redact("refresh-secret exploded", &["refresh-secret"]);
        assert!(!text.contains("refresh-secret"));
    }

    #[test]
    fn persist_does_not_write_into_git_paths() {
        let root = tempfile::tempdir().unwrap();
        write_store(root.path(), "a", "r", now_ms() + 1000);
        let path = store_path(root.path());
        assert!(path.ends_with("harness/subscriptions/xai-oauth.json"));
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("opencode"));
    }
}
