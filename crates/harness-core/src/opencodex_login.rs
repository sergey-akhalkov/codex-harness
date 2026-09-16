//! Browser-only xAI OAuth for pinned OpenCodex 2.44.0.
//!
//! Stock `ocx login xai` opens a browser and installs a closed-stdin waiter.
//! The current kit host writes a local authorization page and omits
//! `onManualCodeInput`. This module owns that contract in Rust.

use crate::{
    broker_endpoint, build_identity, native_build,
    process::{Cancellation, CommandSpec, Deadline, StopReason},
    xai_token_helper,
};
use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const DISCOVERY_URL: &str = "https://auth.x.ai/.well-known/openid-configuration";
const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
const CALLBACK_PORT: u16 = 56121;
const REDIRECT_URI: &str = "http://127.0.0.1:56121/callback";
const CALLBACK_IDLE: Duration = Duration::from_secs(5);
const SUCCESS_HTML: &str = "<!doctype html><html><head><meta charset='utf-8'><title>opencodex</title></head><body style='font-family:system-ui,sans-serif;text-align:center;padding:4rem;color:#111'><h2>Login complete</h2><p>You can close this tab and return to opencodex.</p></body></html>";

#[derive(Clone)]
pub struct Endpoints {
    pub authorization: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginStatus {
    Saved,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug)]
pub struct LoginProbe {
    pub status: LoginStatus,
    pub evidence: PathBuf,
}

pub(crate) trait Transport {
    fn discover(&self, deadline: Deadline, cancel: &Cancellation) -> io::Result<Endpoints>;
    fn exchange(
        &self,
        token_endpoint: &str,
        body: &str,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value>;
}

struct CurlTransport;

impl Transport for CurlTransport {
    fn discover(&self, deadline: Deadline, cancel: &Cancellation) -> io::Result<Endpoints> {
        let payload = https_json(DISCOVERY_URL, None, deadline, cancel)?;
        Ok(Endpoints {
            authorization: https_endpoint(&payload, "authorization_endpoint")?,
            token: https_endpoint(&payload, "token_endpoint")?,
        })
    }

    fn exchange(
        &self,
        token_endpoint: &str,
        body: &str,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        https_json(token_endpoint, Some(body), deadline, cancel)
    }
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}

fn failed(reason: &'static str) -> io::Error {
    io::Error::other(reason)
}

fn contains_secret(haystack: &str, secret: &str) -> bool {
    !secret.is_empty() && haystack.contains(secret)
}

fn random_bytes(length: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(length);
    while bytes.len() < length {
        let hex = broker_endpoint::random_key()?;
        for chunk in hex.as_bytes().chunks(2) {
            if bytes.len() == length {
                break;
            }
            let text = std::str::from_utf8(chunk).unwrap_or("00");
            bytes.push(u8::from_str_radix(text, 16).unwrap_or(0));
        }
    }
    Ok(bytes)
}

fn base64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce() -> io::Result<(String, String, String)> {
    let verifier = base64url(&random_bytes(96)?);
    let challenge = base64url(&Sha256::digest(verifier.as_bytes()));
    let state = random_bytes(16)?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok((verifier, challenge, state))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

struct AuthUrl {
    scheme: String,
    host: String,
    query: Vec<(String, String)>,
}

impl AuthUrl {
    fn parse(url: &str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        let (host_path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let host = host_path.split('/').next()?.split(':').next()?.to_string();
        let mut pairs = Vec::new();
        for part in query.split('&') {
            if part.is_empty() {
                continue;
            }
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            pairs.push((decode(key), decode(value)));
        }
        Some(Self {
            scheme: scheme.to_string(),
            host,
            query: pairs,
        })
    }
}

fn decode(value: &str) -> String {
    let mut out = String::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            )
        {
            out.push(byte as char);
            index += 3;
            continue;
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out.replace('+', " ")
}

pub fn browser_login_page(url: &str) -> io::Result<String> {
    let parsed =
        AuthUrl::parse(url).ok_or_else(|| invalid("Unexpected xAI authorization destination"))?;
    let redirect = parsed
        .query
        .iter()
        .find(|(key, _)| key == "redirect_uri")
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| invalid("Unexpected xAI authorization destination"))?;
    let state = parsed
        .query
        .iter()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| invalid("Unexpected xAI authorization destination"))?;
    if parsed.scheme != "https"
        || parsed.host != "auth.x.ai"
        || redirect != REDIRECT_URI
        || state.is_empty()
    {
        return Err(invalid("Unexpected xAI authorization destination"));
    }
    Ok(format!(
        concat!(
            "<!doctype html><html lang=\"ru\"><meta charset=\"utf-8\"><title>Grok login</title>",
            "<h1>Connect Grok</h1><p>1. Sign in with xAI.</p>",
            "<a href=\"{url}\" target=\"_blank\" rel=\"noreferrer\">Open xAI login</a>",
            "<form action=\"{redirect}\" method=\"get\" autocomplete=\"off\">",
            "<input type=\"hidden\" name=\"state\" value=\"{state}\">",
            "<label for=\"code\">One-time code</label>",
            "<input id=\"code\" name=\"code\" type=\"password\" required autocomplete=\"off\">",
            "<button type=\"submit\">Finish login</button></form></html>"
        ),
        url = html_escape(url),
        redirect = html_escape(redirect),
        state = html_escape(state)
    ))
}

fn https_endpoint(payload: &Value, key: &str) -> io::Result<String> {
    let raw = payload.get(key).and_then(Value::as_str).ok_or_else(|| {
        failed("xAI OAuth discovery response missing authorization/token endpoints")
    })?;
    let parsed = AuthUrl::parse(raw)
        .ok_or_else(|| failed("xAI OAuth discovery returned an unexpected endpoint"))?;
    let host = parsed.host.to_ascii_lowercase();
    if parsed.scheme != "https" || (host != "x.ai" && !host.ends_with(".x.ai")) {
        return Err(failed(
            "xAI OAuth discovery returned an unexpected endpoint",
        ));
    }
    Ok(raw.to_string())
}

fn https_json(
    url: &str,
    form: Option<&str>,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Value> {
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "OpenCodex login was cancelled.",
        ));
    }
    if deadline.expired() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "OpenCodex login deadline expired.",
        ));
    }
    let timeout = deadline.remaining().as_secs().max(1);
    let private = tempfile::Builder::new()
        .prefix("harness-ocx-http-")
        .tempdir()?;
    let body_path = private.path().join("body");
    let stderr_path = private.path().join("stderr");
    let stdout_path = private.path().join("stdout");
    let curl = PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| failed("Windows system tools are unavailable"))?,
    )
    .join("System32/curl.exe");
    let mut command = CommandSpec::new(curl);
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
            "65536",
            "--fail",
            "--silent",
            "--show-error",
            "--disallow-username-in-url",
            "--header",
            "Accept: application/json",
            "--output",
        ]
        .into_iter()
        .map(Into::into),
    );
    command.args.push(body_path.clone().into());
    if let Some(form) = form {
        command.args.extend(["--request".into(), "POST".into()]);
        command.args.extend([
            "--header".into(),
            "Content-Type: application/x-www-form-urlencoded".into(),
        ]);
        command.args.extend(["--data-binary".into(), form.into()]);
    }
    command.args.extend(["--url".into(), url.into()]);
    let outcome = native_build::invoke_management(
        command,
        &stderr_path,
        Some(&stdout_path),
        deadline.remaining(),
    )?;
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(failed("xAI token request failed"));
    }
    let bytes = fs::read(&body_path)?;
    if bytes.len() > 64 * 1024 {
        return Err(failed("xAI token response exceeded the size limit."));
    }
    serde_json::from_slice(&bytes).map_err(|_| failed("xAI token response was not JSON."))
}

fn write_authorization(path: &Path, url: &str) -> io::Result<PathBuf> {
    let page = browser_login_page(url)?;
    let page_path = PathBuf::from(format!("{}.html", path.display()));
    fs::write(
        path,
        serde_json::to_vec(&json!({
            "provider": "xai",
            "authorizationUrl": url,
            "pagePath": page_path
        }))?,
    )?;
    fs::write(&page_path, page)?;
    Ok(page_path)
}

fn cleanup(path: &Path, page_path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(page_path);
}

fn encode(value: &str) -> String {
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

fn query_param(target: &str, name: &str) -> Option<String> {
    let query = target.split_once('?')?.1.split(' ').next()?;
    query.split('&').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key == name).then(|| decode(value))
    })
}

fn read_http_request(
    stream: &mut TcpStream,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Option<String>> {
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    let mut idle_until = Instant::now() + CALLBACK_IDLE;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "OpenCodex login was cancelled.",
            ));
        }
        if deadline.expired() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "OpenCodex login deadline expired.",
            ));
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(None),
            Ok(size) => {
                idle_until = Instant::now() + CALLBACK_IDLE;
                buffer.extend_from_slice(&chunk[..size]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") || buffer.len() > 16 * 1024
                {
                    break;
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut =>
            {
                if Instant::now() >= idle_until {
                    return Ok(None);
                }
                thread::sleep(Duration::from_millis(20));
            }
            // A per-connection failure must not consume the shared login
            // deadline; drop this socket and keep accepting the callback.
            Err(_) => return Ok(None),
        }
    }
    Ok(String::from_utf8(buffer).ok())
}

fn handle_callback(
    mut stream: TcpStream,
    expected_state: &str,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Option<String>> {
    let head = match read_http_request(&mut stream, deadline, cancel)? {
        Some(head) => head,
        None => return Ok(None),
    };
    let target = head.lines().next().unwrap_or_default();
    let path = target.split_whitespace().nth(1).unwrap_or_default();
    let code = query_param(path, "code");
    let state = query_param(path, "state").unwrap_or_default();
    let error = query_param(path, "error");
    let ok = error.is_none()
        && code.as_deref().is_some_and(|value| !value.is_empty())
        && state == expected_state;
    let body = if ok {
        SUCCESS_HTML
    } else {
        "<html>failed</html>"
    };
    let status = if ok { "200 OK" } else { "400 Bad Request" };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.set_write_timeout(Some(CALLBACK_IDLE));
    let _ = stream.write_all(response.as_bytes());
    Ok(if ok { code } else { None })
}

fn wait_for_code(
    expected_state: &str,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", CALLBACK_PORT)).map_err(|_| {
        failed("OAuth callback port 56121 unavailable; cannot fall back to a random port when redirectUri is set")
    })?;
    listener.set_nonblocking(true)?;
    loop {
        if cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "OpenCodex login was cancelled.",
            ));
        }
        if deadline.expired() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "OpenCodex login deadline expired.",
            ));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(code) = handle_callback(stream, expected_state, deadline, cancel)? {
                    return Ok(code);
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut =>
            {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

fn decode_jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let mut padded = payload.replace('-', "+").replace('_', "/");
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(padded)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn persist_credentials(ocx_home: &Path, payload: &Value) -> io::Result<()> {
    let access = payload
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| failed("xAI token response did not include an access token"))?;
    let refresh = payload
        .get("refresh_token")
        .and_then(Value::as_str)
        .ok_or_else(|| failed("xAI token response did not include a refresh token"))?;
    let expires_in = payload
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(3600);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let expires = now + expires_in * 1000 - 2 * 60 * 1000;
    let identity = payload
        .get("id_token")
        .and_then(Value::as_str)
        .and_then(decode_jwt_payload)
        .or_else(|| decode_jwt_payload(access));
    let account_id = identity
        .as_ref()
        .and_then(|value| value.get("sub"))
        .and_then(Value::as_str);
    let email = identity
        .as_ref()
        .and_then(|value| value.get("email"))
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase());
    let account_key = account_id.or(email.as_deref()).unwrap_or(refresh);
    let digest = build_identity::hash_bytes(account_key.as_bytes());
    let id = &digest[..32.min(digest.len())];
    let mut credential = json!({
        "access": access,
        "refresh": refresh,
        "expires": expires,
        "source": "oauth"
    });
    if let Some(account_id) = account_id {
        credential["accountId"] = json!(account_id);
    }
    if let Some(email) = email {
        credential["email"] = json!(email);
    }
    let store = json!({
        "xai": {
            "activeAccountId": id,
            "accounts": [{
                "id": id,
                "credential": credential,
                "addedAt": now
            }]
        }
    });
    let path = xai_token_helper::store_path(ocx_home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&store)?)?;
    Ok(())
}

fn login_with<T: Transport>(
    transport: &T,
    ocx_home: &Path,
    authorization_path: &Path,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<LoginStatus> {
    if !authorization_path.is_absolute() {
        return Err(invalid("Authorization evidence path must be absolute."));
    }
    let (verifier, challenge, state) = pkce()?;
    let endpoints = transport.discover(deadline, cancel)?;
    let nonce = broker_endpoint::random_key()?;
    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}&nonce={}",
        endpoints.authorization,
        CLIENT_ID,
        encode(REDIRECT_URI),
        encode(SCOPE),
        challenge,
        state,
        &nonce[..32.min(nonce.len())]
    );
    let page_path = write_authorization(authorization_path, &url)?;
    let result: io::Result<LoginStatus> = (|| {
        let code = wait_for_code(&state, deadline, cancel)?;
        let body = format!(
            "grant_type=authorization_code&client_id={}&code={}&redirect_uri={}&code_verifier={}",
            CLIENT_ID,
            encode(&code),
            encode(REDIRECT_URI),
            encode(&verifier)
        );
        let payload = transport.exchange(&endpoints.token, &body, deadline, cancel)?;
        persist_credentials(ocx_home, &payload)?;
        Ok(LoginStatus::Saved)
    })();
    cleanup(authorization_path, &page_path);
    match result {
        Ok(status) => Ok(status),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(LoginStatus::Cancelled),
        Err(error) if error.kind() == io::ErrorKind::TimedOut => Ok(LoginStatus::TimedOut),
        Err(error) => {
            if contains_secret(&error.to_string(), "access_token")
                || contains_secret(&error.to_string(), "refresh_token")
            {
                return Err(failed(
                    "OpenCodex login probe emitted private credential material.",
                ));
            }
            Ok(LoginStatus::Failed)
        }
    }
}

pub fn login_xai_browser_only(
    package_root: &Path,
    ocx_home: &Path,
    authorization_path: &Path,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<LoginProbe> {
    let _ = package_root;
    let evidence = authorization_path
        .parent()
        .ok_or_else(|| invalid("Authorization evidence path must be absolute."))?
        .to_path_buf();
    let status = login_with(
        &CurlTransport,
        ocx_home,
        authorization_path,
        deadline,
        cancel,
    )?;
    Ok(LoginProbe { status, evidence })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    struct FixtureTransport {
        authorization: String,
        token: String,
        payload: Value,
    }

    impl Transport for FixtureTransport {
        fn discover(&self, _deadline: Deadline, _cancel: &Cancellation) -> io::Result<Endpoints> {
            Ok(Endpoints {
                authorization: self.authorization.clone(),
                token: self.token.clone(),
            })
        }

        fn exchange(
            &self,
            _token_endpoint: &str,
            _body: &str,
            _deadline: Deadline,
            _cancel: &Cancellation,
        ) -> io::Result<Value> {
            Ok(self.payload.clone())
        }
    }

    fn owned() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::Builder::new()
            .prefix("harness-ocx-browser-")
            .tempdir()
            .unwrap();
        let ocx = root.path().join("opencodex");
        fs::create_dir(&ocx).unwrap();
        let authorization = root.path().join("authorization.json");
        (root, ocx, authorization)
    }

    #[test]
    fn browser_login_page_rejects_foreign_hosts() {
        let err = browser_login_page(
            "https://example.invalid/authorize?state=fake&redirect_uri=http%3A%2F%2F127.0.0.1%3A56121%2Fcallback",
        )
        .unwrap_err();
        assert!(err.to_string().contains("Unexpected xAI"));
    }

    #[test]
    fn browser_only_success_persists_auth_and_removes_authorization_files() {
        let (root, ocx, authorization) = owned();
        let cancel = Cancellation::default();
        let deadline = Deadline::after(Duration::from_secs(5)).unwrap();
        let transport = FixtureTransport {
            authorization: "https://auth.x.ai/oauth2/auth".into(),
            token: "https://auth.x.ai/oauth2/token".into(),
            payload: json!({
                "access_token": "fixture-access",
                "refresh_token": "fixture-refresh",
                "expires_in": 3600
            }),
        };
        let worker = {
            let ocx = ocx.clone();
            let authorization = authorization.clone();
            thread::spawn(move || login_with(&transport, &ocx, &authorization, deadline, &cancel))
        };
        let started = Instant::now();
        let url = loop {
            if authorization.is_file() {
                let value: Value =
                    serde_json::from_slice(&fs::read(&authorization).unwrap()).unwrap();
                break value["authorizationUrl"].as_str().unwrap().to_string();
            }
            assert!(started.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(20));
        };
        assert!(url.contains("auth.x.ai"));
        let page_path = PathBuf::from(format!("{}.html", authorization.display()));
        assert!(page_path.exists());
        let parsed = AuthUrl::parse(&url).unwrap();
        let state = parsed
            .query
            .iter()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .clone();
        let mut stream = TcpStream::connect(("127.0.0.1", CALLBACK_PORT)).unwrap();
        stream
            .write_all(
                format!(
                    "GET /callback?code=fake-code&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let status = worker.join().unwrap().unwrap();
        assert_eq!(status, LoginStatus::Saved);
        assert!(!authorization.exists());
        let auth: Value =
            serde_json::from_slice(&fs::read(xai_token_helper::store_path(&ocx)).unwrap()).unwrap();
        assert_eq!(
            auth["xai"]["accounts"][0]["credential"]["access"],
            "fixture-access"
        );
        assert!(!ocx.join("auth.json").exists());
        drop(root);
    }

    #[test]
    fn browser_only_closed_input_times_out_without_secret_echo() {
        let (root, ocx, authorization) = owned();
        let cancel = Cancellation::default();
        let deadline = Deadline::after(Duration::from_millis(200)).unwrap();
        let transport = FixtureTransport {
            authorization: "https://auth.x.ai/oauth2/auth".into(),
            token: "https://auth.x.ai/oauth2/token".into(),
            payload: json!({
                "access_token": "secret-must-not-leak",
                "refresh_token": "secret-refresh"
            }),
        };
        let status = login_with(&transport, &ocx, &authorization, deadline, &cancel).unwrap();
        assert_eq!(status, LoginStatus::TimedOut);
        assert!(!authorization.exists());
        assert!(!xai_token_helper::store_path(&ocx).exists());
        drop(root);
    }

    #[test]
    fn browser_only_cancel_does_not_wait_for_manual_input() {
        let (root, ocx, authorization) = owned();
        let cancel = Cancellation::default();
        cancel.cancel();
        let deadline = Deadline::after(Duration::from_secs(5)).unwrap();
        let transport = FixtureTransport {
            authorization: "https://auth.x.ai/oauth2/auth".into(),
            token: "https://auth.x.ai/oauth2/token".into(),
            payload: json!({"access_token":"secret","refresh_token":"secret"}),
        };
        let status = login_with(&transport, &ocx, &authorization, deadline, &cancel).unwrap();
        assert_eq!(status, LoginStatus::Cancelled);
        drop(root);
    }

    #[test]
    fn read_http_request_drops_idle_connection_before_login_deadline() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let cancel = Cancellation::default();
        let deadline = Deadline::after(Duration::from_secs(60)).unwrap();
        let started = Instant::now();
        let head = read_http_request(&mut server, deadline, &cancel).unwrap();
        assert_eq!(head, None);
        assert!(started.elapsed() < Duration::from_secs(15));
        drop(client);
    }

    #[test]
    fn read_http_request_keeps_waiting_across_split_callback_head() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let cancel = Cancellation::default();
        let deadline = Deadline::after(Duration::from_secs(5)).unwrap();
        let reader =
            thread::spawn(move || read_http_request(&mut server, deadline, &cancel).unwrap());
        client.write_all(b"GET /callback?code=split").unwrap();
        thread::sleep(Duration::from_millis(300));
        client
            .write_all(b"&state=s HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .unwrap();
        let head = reader.join().unwrap().unwrap();
        assert!(head.starts_with("GET /callback?code=split&state=s "));
        drop(client);
    }
}
