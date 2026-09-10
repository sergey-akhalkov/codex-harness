//! Explicit public release metadata, using the installed Windows curl client.
//! Default discovery never constructs this client or contacts a registry.
#![cfg(windows)]
use crate::{
    native_build,
    process::{CommandSpec, StopReason},
};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs::File,
    io::Read,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    time::Duration,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

const MAX_BODY: u64 = 16 * 1024 * 1024;
type Result<T> = std::result::Result<T, &'static str>;

pub(crate) fn identity_endpoint(spec: &Value) -> Result<&'static str> {
    // An exact current catalogue mapping also rejects credentials, alternative
    // ports, query strings, URL globbing, and arbitrary paths on official hosts.
    let (package, manager, url) = match spec["id"].as_str() {
        Some("serena") => (
            "serena-agent",
            "uv",
            "https://pypi.org/pypi/serena-agent/json",
        ),
        Some("graphify") => ("graphifyy", "uv", "https://pypi.org/pypi/graphifyy/json"),
        Some("codebase-memory") => (
            "codebase-memory-mcp",
            "npm",
            "https://registry.npmjs.org/codebase-memory-mcp/latest",
        ),
        Some("nuphus") => (
            "@nuphus/nuphus-mcp",
            "npm",
            "https://registry.npmjs.org/@nuphus%2fnuphus-mcp/latest",
        ),
        Some("python") => (
            "basedpyright",
            "npm",
            "https://registry.npmjs.org/basedpyright/latest",
        ),
        Some("rust") => (
            "rust-analyzer",
            "rustup",
            "https://static.rust-lang.org/dist/channel-rust-stable.toml",
        ),
        _ => return Err("unsupported-metadata-source"),
    };
    if spec["package"] != package || spec["manager"] != manager {
        return Err("unsupported-metadata-source");
    }
    Ok(url)
}

pub(crate) fn endpoint(spec: &Value) -> Result<&'static str> {
    let url = identity_endpoint(spec)?;
    if spec["metadata"] != url {
        return Err("unsupported-metadata-source");
    }
    Ok(url)
}

fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(|_| "metadata-output-unavailable")?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "metadata-output-unavailable")?;
    if bytes.len() as u64 > limit {
        return Err("metadata-output-too-large");
    }
    Ok(bytes)
}

fn supported_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let version = text
        .lines()
        .next()?
        .strip_prefix("curl ")?
        .split_whitespace()
        .next()?;
    let numbers = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    // Since 8.4.0 --max-filesize also bounds responses with unknown length.
    (numbers.len() == 3 && numbers.as_slice() >= [8, 4, 0].as_slice()).then(|| version.to_owned())
}

fn command(executable: &Path, directory: &Path) -> CommandSpec {
    let mut command = CommandSpec::new(executable);
    command.current_dir = Some(directory.to_owned());
    command.args.push("--disable".into()); // Must be the first argument: no .curlrc.
    // Neither inherited trust overrides nor TLS key logging belong to this
    // public metadata operation. Normal configured network proxies still work.
    for key in [
        "CURL_CA_BUNDLE",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "SSLKEYLOGFILE",
        "QLOGDIR",
    ] {
        command.env.insert(key.into(), None);
    }
    command
}

fn fetch_command(executable: &Path, directory: &Path, url: &str) -> CommandSpec {
    let mut command = command(executable, directory);
    command.args.extend(
        [
            "--globoff",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "0",
            "--max-time",
            "45",
            "--connect-timeout",
            "10",
            "--max-filesize",
            "16777216",
            "--fail",
            "--silent",
            "--show-error",
            "--disallow-username-in-url",
            "--user-agent",
            "codex-harness-dependency-lifecycle",
            "--header",
            "Accept: application/json, application/toml, text/plain",
            "--write-out",
            "%{http_code}",
            "--output",
        ]
        .map(OsString::from),
    );
    command.args.push(directory.join("body").into_os_string());
    command
        .args
        .extend([OsString::from("--url"), OsString::from(url)]);
    // No --location, --compressed, auth, netrc, upload or retry options.
    command
}

fn limited_command(executable: &Path, directory: &Path, url: &str, limit: u64) -> CommandSpec {
    let mut cmd = fetch_command(executable, directory, url);
    let position = cmd
        .args
        .iter()
        .position(|arg| arg == "--max-filesize")
        .unwrap();
    cmd.args[position + 1] = limit.to_string().into();
    cmd
}

pub(crate) struct Client {
    executable: PathBuf,
    pub(crate) version: String,
}

impl Client {
    pub(crate) fn new() -> Result<Self> {
        let mut buffer = vec![0u16; 32768];
        let size =
            unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
        if size == 0 || size >= buffer.len() {
            return Err("system-curl-unavailable");
        }
        let executable = PathBuf::from(OsString::from_wide(&buffer[..size])).join("curl.exe");
        let private = tempfile::Builder::new()
            .prefix("harness-metadata-client-")
            .tempdir()
            .map_err(|_| "metadata-private-directory-unavailable")?;
        let stdout = private.path().join("stdout");
        let mut cmd = command(&executable, private.path());
        cmd.args.push("--version".into());
        let outcome = native_build::invoke_management(
            cmd,
            &private.path().join("stderr"),
            Some(&stdout),
            Duration::from_secs(5),
        )
        .map_err(|_| "system-curl-unavailable")?;
        if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
            return Err("system-curl-unavailable");
        }
        let version = supported_version(&read(&stdout, 16 * 1024)?)
            .ok_or("system-curl-version-unsupported")?;
        private
            .close()
            .map_err(|_| "metadata-private-cleanup-failed")?;
        Ok(Self {
            executable,
            version,
        })
    }

    pub(crate) fn fetch(&self, spec: &Value) -> Result<Vec<u8>> {
        let url = endpoint(spec)?;
        self.fetch_validated(url, MAX_BODY)
    }

    // Only lifecycle modules may supply an already identity-validated official
    // URL. This is deliberately not a public arbitrary-URL CLI/API.
    pub(crate) fn fetch_validated(&self, url: &str, limit: u64) -> Result<Vec<u8>> {
        if limit > 128 * 1024 * 1024 {
            return Err("metadata-output-too-large");
        }
        let private = tempfile::Builder::new()
            .prefix("harness-release-metadata-")
            .tempdir()
            .map_err(|_| "metadata-private-directory-unavailable")?;
        let mut command = limited_command(&self.executable, private.path(), url, limit);
        if url.starts_with("https://api.github.com/") {
            command
                .args
                .extend(["--header".into(), "X-GitHub-Api-Version: 2026-03-10".into()]);
        }
        let bytes = transfer_limited(command, private.path(), limit);
        private
            .close()
            .map_err(|_| "metadata-private-cleanup-failed")?;
        bytes
    }

    pub(crate) fn github_asset(&self, asset: &crate::dependency_assets::Asset) -> Result<Vec<u8>> {
        let private = tempfile::Builder::new()
            .prefix("harness-release-asset-")
            .tempdir()
            .map_err(|_| "metadata-private-directory-unavailable")?;
        let result = (|| {
            let mut command =
                limited_command(&self.executable, private.path(), &asset.url, asset.size);
            let output = command
                .args
                .iter()
                .position(|arg| arg == "--write-out")
                .unwrap();
            command.args[output + 1] = "%{http_code} %{redirect_url}".into();
            completed(command, private.path())?;
            let status = read(&private.path().join("stdout"), 16 * 1024)?;
            let status =
                std::str::from_utf8(&status).map_err(|_| "metadata-http-status-rejected")?;
            if status == "200 " {
                return read(&private.path().join("body"), asset.size);
            }
            let location = status
                .strip_prefix("302 ")
                .ok_or("metadata-http-status-rejected")?;
            if !crate::dependency_assets::admitted_redirect(location) {
                return Err("metadata-redirect-rejected");
            }
            // Exactly one admitted CDN hop. The ordinary transfer rejects any
            // second redirect. Neither signed URL nor response headers are output.
            self.fetch_validated(location, asset.size)
        })();
        private
            .close()
            .map_err(|_| "metadata-private-cleanup-failed")?;
        result
    }
}

#[cfg(test)]
fn transfer(command: CommandSpec, directory: &Path) -> Result<Vec<u8>> {
    transfer_limited(command, directory, MAX_BODY)
}

fn transfer_limited(command: CommandSpec, directory: &Path, limit: u64) -> Result<Vec<u8>> {
    completed(command, directory)?;
    // Metadata never follows redirects, even to another official host.
    if read(&directory.join("stdout"), 16)?.as_slice() != b"200" {
        return Err("metadata-http-status-rejected");
    }
    read(&directory.join("body"), limit)
}

fn completed(command: CommandSpec, directory: &Path) -> Result<()> {
    let stdout = directory.join("stdout");
    let outcome = native_build::invoke_management(
        command,
        &directory.join("stderr"),
        Some(&stdout),
        Duration::from_secs(50),
    )
    .map_err(|_| "metadata-process-failed")?;
    if outcome.reason != StopReason::Exited {
        return Err("metadata-deadline-or-resource-limit");
    }
    if outcome.exit_code != 0 {
        return Err(match outcome.exit_code {
            22 => "metadata-http-error",
            28 => "metadata-deadline",
            63 => "metadata-output-too-large",
            35 | 51 | 58 | 60 | 77 | 83 | 90 | 91 => "metadata-tls-error",
            _ => "metadata-transfer-failed",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_official_endpoints_reject_credentials_and_source_substitution() {
        let mut spec = json!({"id":"python","package":"basedpyright","manager":"npm","metadata":"https://registry.npmjs.org/basedpyright/latest"});
        assert!(endpoint(&spec).is_ok());
        for bad in [
            "http://registry.npmjs.org/basedpyright/latest",
            "https://secret@registry.npmjs.org/basedpyright/latest",
            "https://registry.npmjs.org:443/basedpyright/latest",
            "https://registry.npmjs.org/basedpyright/latest?key=PRIVATE",
            "https://registry.npmjs.org/{basedpyright,other}/latest",
        ] {
            spec["metadata"] = json!(bad);
            assert_eq!(endpoint(&spec), Err("unsupported-metadata-source"));
        }
        spec["metadata"] = json!("https://registry.npmjs.org/basedpyright/latest");
        spec["package"] = json!("other");
        assert!(endpoint(&spec).is_err());
    }

    #[test]
    fn curl_unknown_length_bound_requires_supported_version() {
        for good in ["8.4.0", "8.21.0", "9.0.0"] {
            assert_eq!(
                supported_version(format!("curl {good} (Windows)\nFeatures: anything").as_bytes()),
                Some(good.into())
            );
        }
        for bad in [
            "curl 8.3.9",
            "curl 7.99.0",
            "curl 8.4.0-private",
            "curl 8.4",
            "fake 9.1.0",
            "curl 9999999999999.1.0",
        ] {
            assert_eq!(supported_version(bad.as_bytes()), None);
        }
    }

    #[test]
    fn metadata_command_suppresses_config_redirects_and_tls_log_outputs() {
        let command = fetch_command(
            Path::new("C:/Windows/System32/curl.exe"),
            Path::new("C:/owned metadata"),
            "https://pypi.org/pypi/serena-agent/json",
        );
        assert_eq!(command.args.first().unwrap(), "--disable");
        assert!(command.args.windows(2).any(|v| v == ["--proto", "=https"]));
        assert!(
            command
                .args
                .windows(2)
                .any(|v| v == ["--max-filesize", "16777216"])
        );
        for absent in [
            "--location",
            "--compressed",
            "--insecure",
            "--netrc",
            "--user",
            "--retry",
            "--config",
        ] {
            assert!(!command.args.iter().any(|arg| arg == absent));
        }
        assert_eq!(
            command.env.get(&OsString::from("SSLKEYLOGFILE")),
            Some(&None)
        );
        assert_eq!(command.args[command.args.len() - 2], "--url");
    }

    #[test]
    fn output_read_keeps_body_bound_on_unknown_or_changed_length() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("body");
        std::fs::write(&path, b"12345").unwrap();
        assert_eq!(read(&path, 4), Err("metadata-output-too-large"));
        assert_eq!(read(&path, 5).unwrap(), b"12345");
    }

    #[test]
    #[ignore = "explicit installed curl against owned loopback HTTP failures; production remains exact HTTPS only"]
    fn real_client_preserves_failure_privacy_limits_and_redirect_boundary() {
        use std::{io::Write, net::TcpListener, thread};
        let client = Client::new().unwrap();
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let cases = [
            ("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_owned(), None),
            ("HTTP/1.1 403 Forbidden\r\nContent-Length: 7\r\nConnection: close\r\n\r\nPRIVATE".to_owned(), Some("metadata-http-error")),
            (format!("HTTP/1.1 302 Found\r\nLocation: http://{}/PRIVATE\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", destination.local_addr().unwrap()), Some("metadata-http-status-rejected")),
            ("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n20\r\n0123456789abcdef0123456789abcdef\r\n0\r\n\r\n".to_owned(), Some("metadata-output-too-large")),
            ("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_owned(), Some("metadata-tls-error")),
            (String::new(), Some("metadata-deadline")),
        ];
        for (response, expected) in cases {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let tls = expected == Some("metadata-tls-error");
            let timeout = expected == Some("metadata-deadline");
            let url = format!(
                "{}://{}",
                if tls { "https" } else { "http" },
                listener.local_addr().unwrap()
            );
            let server = thread::spawn(move || {
                let until = std::time::Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < until =>
                        {
                            thread::sleep(Duration::from_millis(10))
                        }
                        Err(error) => panic!("owned server accept: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 4096];
                let count = stream.read(&mut request).unwrap();
                if !tls {
                    assert!(request[..count].starts_with(b"GET / HTTP/"));
                }
                if timeout {
                    thread::sleep(Duration::from_millis(500));
                    return;
                }
                stream.write_all(response.as_bytes()).unwrap();
            });
            let private = tempfile::tempdir().unwrap();
            let mut command = fetch_command(&client.executable, private.path(), &url);
            // Test-only HTTP scope. The production endpoint validator above has
            // no override, environment switch, or configurable client path.
            command.args.extend(
                [
                    "--proto",
                    if tls { "=https" } else { "=http" },
                    "--proxy",
                    "",
                    "--max-filesize",
                    "16",
                ]
                .map(OsString::from),
            );
            if timeout {
                command
                    .args
                    .extend(["--max-time", "0.2"].map(OsString::from));
            }
            let result = transfer(command, private.path());
            server.join().unwrap();
            match expected {
                Some(reason) => assert_eq!(result, Err(reason)),
                None => assert_eq!(result.unwrap(), b"{}"),
            }
        }
        assert_eq!(
            destination.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    #[ignore = "explicit installed Windows curl version check, no network"]
    fn actual_system_curl_supports_the_bounded_transport() {
        let client = Client::new().unwrap();
        assert!(supported_version(format!("curl {}", client.version).as_bytes()).is_some());
    }
}
