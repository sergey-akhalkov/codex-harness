//! Explicit stage/update compatibility checks, never an ordinary MCP launcher.
//!
//! Only a digest-pinned, local native executable is admitted. The caller owns
//! package/DLL provenance and version selection; a Windows Job is containment,
//! not a filesystem, network, or hostile same-account security sandbox.
//! Nuphus is never asked to operate the desktop/browser. Codebase Memory indexes
//! only an inert source sample in this invocation's disposable state.
use crate::process::Cancellation;
use serde_json::Value;
use std::{io, path::Path};

#[cfg(windows)]
pub(crate) use windows::{artifact, environment, pipe, private_directory, strict_json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeKind {
    CodebaseMemory,
    Nuphus,
}

/// Validate the complete bounded exchange and EOF, then remove the owned tree.
/// Errors contain fixed harness reasons, never foreign JSON, stderr or paths.
pub fn probe(executable: &Path, kind: ProbeKind, expected_sha256: &str) -> io::Result<Value> {
    probe_with_cancellation(executable, kind, expected_sha256, &Cancellation::default())
}

/// Same fixed protocol with caller cancellation; cancellation never targets an
/// installed/shared server. No configurable commands, tool calls or environment.
pub fn probe_with_cancellation(
    executable: &Path,
    kind: ProbeKind,
    expected_sha256: &str,
    cancellation: &Cancellation,
) -> io::Result<Value> {
    #[cfg(windows)]
    return windows::run(executable, kind, expected_sha256, cancellation, false);
    #[cfg(not(windows))]
    {
        let _ = (executable, kind, expected_sha256, cancellation);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "MCP probe requires Windows",
        ))
    }
}

/// Obtain complete tool definitions from an isolated native process. This only
/// negotiates, lists all bounded catalogue pages and shuts down by EOF; it does
/// not call tools or certify their runtime compatibility.
pub fn catalogue(
    executable: &Path,
    kind: ProbeKind,
    expected_sha256: &str,
    cancellation: &Cancellation,
) -> io::Result<Value> {
    #[cfg(windows)]
    return windows::run(executable, kind, expected_sha256, cancellation, true);
    #[cfg(not(windows))]
    {
        let _ = (executable, kind, expected_sha256, cancellation);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "MCP catalogue requires Windows",
        ))
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::process::{CommandSpec, Deadline, Job, Limits, StopReason};
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs::{self, File, OpenOptions},
        io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
        os::windows::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        path::{Component, Prefix},
        time::Duration,
    };
    use windows_sys::Win32::{
        Security::*,
        Storage::FileSystem::*,
        System::{
            Pipes::CreatePipe,
            Threading::{GetCurrentProcess, OpenProcessToken},
            WindowsProgramming::DRIVE_FIXED,
        },
    };

    const TIMEOUT: Duration = Duration::from_secs(25);
    const CLEANUP: Duration = Duration::from_secs(5);
    const POLL: Duration = Duration::from_millis(20);
    const LINE_LIMIT: usize = 1024 * 1024;
    const STDOUT_LIMIT: usize = 4 * 1024 * 1024;
    const STDERR_LIMIT: usize = 256 * 1024;
    const REQUEST_LIMIT: usize = 16 * 1024;
    const TOOL_LIMIT: usize = 256;
    const NOTIFICATION_LIMIT: usize = 128;
    const ARTIFACT_LIMIT: u64 = 1024 * 1024 * 1024;
    const PROTOCOL: &str = "2024-11-05";

    // Pinned upstream registry, v0.2.2/crates/nuphus-mcp/src/tools/schemas.rs:
    // https://github.com/mrpulor-gh/nuphus-mcp/blob/v0.2.2/crates/nuphus-mcp/src/tools/schemas.rs
    const NUPHUS_TOOLS: &[&str] = &[
        "desktop_screen_size",
        "desktop_screenshot",
        "desktop_windows_list",
        "desktop_window_activate",
        "desktop_window_screenshot",
        "desktop_window_move",
        "desktop_window_resize",
        "desktop_window_info",
        "desktop_vision",
        "desktop_perceive",
        "desktop_mouse",
        "desktop_mouse_drag",
        "desktop_input",
        "desktop_clipboard_clean",
        "desktop_clipboard_write",
        "browser_navigate",
        "browser_snapshot",
        "browser_exec",
        "browser_click",
        "browser_type",
        "browser_press",
        "browser_scroll",
        "browser_extract",
        "browser_screenshot",
        "browser_close",
        "browser_evaluate",
        "browser_back",
        "browser_forward",
        "browser_wait_for",
        "browser_cookies_get",
        "browser_cookies_set",
        "browser_import_cookies",
        "browser_upload",
        "browser_drag_files",
        "browser_list_downloads",
        "browser_new_tab",
        "browser_list_tabs",
        "browser_switch_tab",
    ];

    fn invalid(reason: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, reason)
    }
    fn checked(ok: i32, reason: &'static str) -> io::Result<()> {
        if ok == 0 {
            Err(io::Error::other(reason))
        } else {
            Ok(())
        }
    }
    fn check_stop(deadline: Deadline, cancellation: &Cancellation) -> io::Result<()> {
        if cancellation.is_cancelled() {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "MCP probe cancelled",
            ))
        } else if deadline.expired() {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "MCP probe deadline exceeded",
            ))
        } else {
            Ok(())
        }
    }

    /// Pin each lexical ancestor against rename/delete, rejecting reparse points
    /// and UNC/device/ADS inputs. A final file lock also denies concurrent writes.
    /// Retained handles close only after the owned process tree has stopped.
    fn pin_path(path: &Path) -> io::Result<Vec<File>> {
        let mut components = path.components();
        if !matches!(components.next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
            || !matches!(components.next(), Some(Component::RootDir))
            || components.any(|c| !matches!(c, Component::Normal(s) if !s.to_string_lossy().contains([':', '\0'])))
        {
            return Err(invalid("MCP executable requires an absolute local disk path"));
        }
        let root = path
            .ancestors()
            .last()
            .ok_or_else(|| invalid("MCP path has no root"))?;
        let wide: Vec<_> = root.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { GetDriveTypeW(wide.as_ptr()) } != DRIVE_FIXED {
            return Err(invalid("MCP executable requires a fixed local disk"));
        }
        let mut pins = Vec::new();
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            let directory = ancestor != path;
            let handle = OpenOptions::new()
                .read(true)
                .share_mode(if directory {
                    FILE_SHARE_READ | FILE_SHARE_WRITE
                } else {
                    FILE_SHARE_READ
                })
                .custom_flags(
                    FILE_FLAG_OPEN_REPARSE_POINT
                        | if directory {
                            FILE_FLAG_BACKUP_SEMANTICS
                        } else {
                            0
                        },
                )
                .open(ancestor)
                .map_err(|_| invalid("MCP artifact cannot be pinned"))?;
            let metadata = handle
                .metadata()
                .map_err(|_| invalid("MCP artifact metadata unavailable"))?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || metadata.is_dir() != directory
            {
                return Err(invalid("MCP artifact has a linked or invalid ancestor"));
            }
            pins.push(handle);
        }
        Ok(pins)
    }

    pub(crate) fn artifact(
        path: &Path,
        digest: &str,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Vec<File>> {
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("MCP artifact requires an expected SHA-256"));
        }
        if !path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
        {
            return Err(invalid("MCP artifact must be a native executable"));
        }
        let mut pins = pin_path(path)?;
        let file = pins
            .last_mut()
            .ok_or_else(|| invalid("MCP artifact unavailable"))?;
        let length = file
            .metadata()
            .map_err(|_| invalid("MCP artifact metadata unavailable"))?
            .len();
        if !(256..=ARTIFACT_LIMIT).contains(&length) {
            return Err(invalid("MCP artifact size limit"));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            check_stop(deadline, cancel)?;
            let count = file
                .read(&mut buffer)
                .map_err(|_| invalid("MCP artifact read failed"))?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(digest) {
            return Err(invalid("MCP artifact digest mismatch"));
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| invalid("MCP artifact read failed"))?;
        let mut dos = [0; 64];
        file.read_exact(&mut dos)
            .map_err(|_| invalid("MCP artifact is not native PE"))?;
        let offset = u32::from_le_bytes(dos[60..64].try_into().unwrap()) as u64;
        if &dos[..2] != b"MZ" || offset > length.saturating_sub(26) {
            return Err(invalid("MCP artifact is not native PE"));
        }
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| invalid("MCP artifact read failed"))?;
        let mut pe = [0; 26];
        file.read_exact(&mut pe)
            .map_err(|_| invalid("MCP artifact is not native PE"))?;
        if &pe[..4] != b"PE\0\0"
            || u16::from_le_bytes([pe[4], pe[5]]) != 0x8664
            || u16::from_le_bytes([pe[22], pe[23]]) & 0x2002 != 0x0002
            || u16::from_le_bytes([pe[24], pe[25]]) != 0x20b
        {
            return Err(invalid("MCP artifact requires a Windows x64 PE executable"));
        }
        Ok(pins)
    }

    /// The empty random directory receives an owner-only protected DACL before
    /// any state or child is created. No inherited grants reach probe contents.
    pub(crate) fn private_directory() -> io::Result<tempfile::TempDir> {
        let parent = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| invalid("MCP private state parent unavailable"))?;
        let dir = tempfile::Builder::new()
            .prefix("hmp-")
            .tempdir_in(parent)
            .map_err(|_| invalid("MCP private state creation failed"))?;
        let mut token = std::ptr::null_mut();
        checked(
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
            "MCP account identity unavailable",
        )?;
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut needed = 0;
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                std::ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        if needed == 0 || needed > 65536 {
            return Err(invalid("MCP account identity size limit"));
        }
        let mut user = vec![0u64; (needed as usize).div_ceil(8)];
        checked(
            unsafe {
                GetTokenInformation(
                    token.as_raw_handle(),
                    TokenUser,
                    user.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                )
            },
            "MCP account identity unavailable",
        )?;
        let sid = unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        let mut acl = [0u32; 256];
        let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
        checked(
            unsafe { InitializeAcl(acl_ptr, size_of_val(&acl) as u32, ACL_REVISION) },
            "MCP private ACL failed",
        )?;
        checked(
            unsafe {
                AddAccessAllowedAceEx(
                    acl_ptr,
                    ACL_REVISION,
                    OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
                    FILE_ALL_ACCESS,
                    sid,
                )
            },
            "MCP private ACL failed",
        )?;
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        let sd = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
        checked(
            unsafe { InitializeSecurityDescriptor(sd, 1) },
            "MCP private ACL failed",
        )?;
        checked(
            unsafe { SetSecurityDescriptorDacl(sd, 1, acl_ptr, 0) },
            "MCP private ACL failed",
        )?;
        checked(
            unsafe { SetSecurityDescriptorControl(sd, SE_DACL_PROTECTED, SE_DACL_PROTECTED) },
            "MCP private ACL protection failed",
        )?;
        let wide: Vec<_> = dir
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        checked(
            unsafe {
                SetFileSecurityW(
                    wide.as_ptr(),
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    sd,
                )
            },
            "MCP private ACL failed",
        )?;
        // Inspect the stored descriptor, rather than treating a successful API
        // return as proof that inheritance was disabled on this filesystem.
        let mut stored = [0u64; 1024];
        let stored_sd = stored.as_mut_ptr().cast();
        checked(
            unsafe {
                GetFileSecurityW(
                    wide.as_ptr(),
                    DACL_SECURITY_INFORMATION,
                    stored_sd,
                    size_of_val(&stored) as u32,
                    &mut needed,
                )
            },
            "MCP private ACL readback failed",
        )?;
        let (mut actual, mut present, mut defaulted) = (std::ptr::null_mut(), 0, 0);
        let (mut control, mut revision) = (0, 0);
        checked(
            unsafe {
                GetSecurityDescriptorDacl(stored_sd, &mut present, &mut actual, &mut defaulted)
            },
            "MCP private ACL readback failed",
        )?;
        checked(
            unsafe { GetSecurityDescriptorControl(stored_sd, &mut control, &mut revision) },
            "MCP private ACL readback failed",
        )?;
        if present == 0
            || actual.is_null()
            || control & SE_DACL_PROTECTED == 0
            || unsafe { (*actual).AceCount } != 1
        {
            return Err(invalid("MCP private ACL is not owner-only and protected"));
        }
        let mut entry = std::ptr::null_mut();
        checked(
            unsafe { GetAce(actual, 0, &mut entry) },
            "MCP private ACL entry unavailable",
        )?;
        let ace = unsafe { &*entry.cast::<ACCESS_ALLOWED_ACE>() };
        if ace.Header.AceType != 0
            || ace.Mask != FILE_ALL_ACCESS
            || unsafe { EqualSid((&ace.SidStart as *const u32).cast_mut().cast(), sid) } == 0
        {
            return Err(invalid("MCP private ACL does not match current account"));
        }
        Ok(dir)
    }

    pub(crate) fn environment(command: &mut CommandSpec, root: &Path) -> io::Result<()> {
        // CommandSpec removes names case-insensitively. Reject non-ASCII names
        // rather than accidentally retaining an environment it cannot clear.
        for (key, _) in std::env::vars_os() {
            let name = key
                .to_str()
                .filter(|s| s.is_ascii() && !s.contains(['=', '\0']))
                .ok_or_else(|| invalid("MCP environment cannot be isolated"))?;
            command.env.insert(name.into(), None);
        }
        // Windows loader/OS discovery only: no PATH, credentials, proxy, model,
        // browser endpoint, package manager or tool profile from the caller.
        for name in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(name) {
                command.env.insert(name.into(), Some(value));
            }
        }
        for (variable, folder) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("APPDATA", "roaming"),
            ("LOCALAPPDATA", "local"),
            ("TEMP", "temp"),
            ("TMP", "temp"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("CODEX_HOME", "codex"),
            ("CBM_CACHE_DIR", "cbm"),
            ("CBM_RUNTIME_DIR", "ipc"),
            ("NUPHUS_MODELS_DIR", "models"),
        ] {
            let path = root.join(folder);
            fs::create_dir_all(&path).map_err(|_| invalid("MCP private state setup failed"))?;
            command
                .env
                .insert(variable.into(), Some(path.into_os_string()));
        }
        // Verified v0.2.2 models.rs: skips both OCR and YOLO downloads.
        command
            .env
            .insert("NUPHUS_MCP_NO_MODEL_DOWNLOAD".into(), Some("1".into()));
        Ok(())
    }

    pub(crate) fn pipe() -> io::Result<(File, File)> {
        let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
        checked(
            unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 64 * 1024) },
            "MCP pipe creation failed",
        )?;
        // Non-inheritable originals; Job duplicates only child endpoints into
        // its explicit handle list. Blocking IO is confined to joined workers;
        // Job.wait kills all pipe owners on deadline, cancellation or root exit.
        Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
    }

    struct Channel {
        input: Option<File>,
        output: BufReader<File>,
        bytes: usize,
        sent: usize,
        notifications: usize,
        id: u64,
        deadline: Deadline,
        cancel: Cancellation,
    }
    impl Channel {
        fn send(&mut self, value: Value) -> io::Result<()> {
            check_stop(self.deadline, &self.cancel)?;
            let bytes =
                serde_json::to_vec(&value).map_err(|_| invalid("MCP request encoding failed"))?;
            let bytes = crate::mcp_protocol::Message::parse(&bytes)?.encode()?;
            self.sent += bytes.len();
            if self.sent > REQUEST_LIMIT {
                return Err(invalid("MCP request byte limit"));
            }
            self.input
                .as_mut()
                .ok_or_else(|| invalid("MCP input already closed"))?
                .write_all(&bytes)
                .map_err(|_| invalid("MCP input pipe closed"))
        }
        fn next(&mut self) -> io::Result<Option<Value>> {
            check_stop(self.deadline, &self.cancel)?;
            let mut bytes = Vec::new();
            let count = Read::by_ref(&mut self.output)
                .take((LINE_LIMIT + 1) as u64)
                .read_until(b'\n', &mut bytes)
                .map_err(|_| invalid("MCP output pipe read failed"))?;
            if count == 0 {
                return Ok(None);
            }
            self.bytes += count;
            if count > LINE_LIMIT || self.bytes > STDOUT_LIMIT {
                return Err(invalid("MCP stdout byte limit"));
            }
            if !bytes.ends_with(b"\n") {
                return Err(invalid("MCP incomplete output frame"));
            }
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            let value = crate::mcp_protocol::Message::parse(&bytes)
                .map_err(|_| invalid("MCP malformed JSON-RPC envelope"))?
                .into_value();
            Ok(Some(value))
        }
        fn notification(&mut self, value: &Value) -> io::Result<bool> {
            if value.get("id").is_some() {
                return Ok(false);
            }
            if !value["method"]
                .as_str()
                .is_some_and(|s| s.starts_with("notifications/") && s.len() <= 128)
                || value.get("result").is_some()
                || value.get("error").is_some()
                || value.get("params").is_some_and(|v| !v.is_object())
            {
                return Err(invalid("MCP invalid notification"));
            }
            self.notifications += 1;
            if self.notifications > NOTIFICATION_LIMIT {
                return Err(invalid("MCP notification limit"));
            }
            Ok(true)
        }
        fn request(&mut self, method: &str, params: Value) -> io::Result<Value> {
            self.id += 1;
            self.send(json!({"jsonrpc":"2.0", "id":self.id, "method":method,"params":params}))?;
            loop {
                let value = self
                    .next()?
                    .ok_or_else(|| invalid("MCP EOF before response"))?;
                if self.notification(&value)? {
                    continue;
                }
                if value["id"].as_u64() != Some(self.id) || value.get("method").is_some() {
                    return Err(invalid("MCP unexpected response identity"));
                }
                if value.get("error").is_some() {
                    return Err(invalid("MCP request returned an error"));
                }
                return value
                    .get("result")
                    .filter(|v| v.is_object())
                    .cloned()
                    .ok_or_else(|| invalid("MCP response lacks object result"));
            }
        }
        fn call(&mut self, name: &str, arguments: Value) -> io::Result<Value> {
            let result = self.request("tools/call", json!({"name":name,"arguments":arguments}))?;
            if result.get("isError").is_some_and(|v| v != false)
                || result.get("error").is_some()
                || !result["content"].as_array().is_some_and(|a| {
                    a.iter().any(|v| {
                        v["type"] == "text" && v["text"].as_str().is_some_and(|s| !s.is_empty())
                    })
                })
            {
                return Err(invalid("MCP tool operation failed"));
            }
            Ok(result)
        }
        fn exchange(
            &mut self,
            kind: ProbeKind,
            root: &Path,
            catalogue_only: bool,
        ) -> io::Result<Value> {
            let info = self.request("initialize", json!({"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"harness-update-probe","version":"1"}}))?;
            let server = match kind {
                ProbeKind::CodebaseMemory => "codebase-memory-mcp",
                ProbeKind::Nuphus => "nuphus-mcp",
            };
            if info["protocolVersion"] != PROTOCOL
                || info["serverInfo"]["name"] != server
                || !info["serverInfo"]["version"]
                    .as_str()
                    .is_some_and(|v| !v.is_empty() && v.len() <= 64)
                || !info["capabilities"]["tools"].is_object()
            {
                return Err(invalid("MCP initialize contract incompatible"));
            }
            self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
            let mut tools = BTreeMap::new();
            let mut definitions = Vec::new();
            let mut cursor = None;
            let mut cursors = BTreeSet::new();
            for page in 0..8 {
                let result = self.request(
                    "tools/list",
                    cursor
                        .take()
                        .map_or_else(|| json!({}), |c: String| json!({"cursor":c})),
                )?;
                let items = result["tools"]
                    .as_array()
                    .ok_or_else(|| invalid("MCP tools catalogue missing"))?;
                for tool in items {
                    let name = tool["name"]
                        .as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 128)
                        .ok_or_else(|| invalid("MCP invalid tool name"))?;
                    validate_schema(&tool["inputSchema"])?;
                    if tool
                        .get("description")
                        .is_some_and(|value| !value.is_string())
                        || tool.get("title").is_some_and(|value| !value.is_string())
                        || tool
                            .get("annotations")
                            .is_some_and(|value| !value.is_object())
                    {
                        return Err(invalid("MCP invalid tool definition"));
                    }
                    if let Some(schema) = tool.get("outputSchema") {
                        validate_schema(schema)?;
                    }
                    if tools.len() >= TOOL_LIMIT
                        || tools
                            .insert(name.to_owned(), tool["inputSchema"].clone())
                            .is_some()
                    {
                        return Err(invalid("MCP duplicate or excessive tools"));
                    }
                    definitions.push(tool.clone());
                }
                if let Some(next) = result.get("nextCursor") {
                    let next = next
                        .as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 1024)
                        .ok_or_else(|| invalid("MCP invalid catalogue cursor"))?;
                    if page == 7 || !cursors.insert(next.to_owned()) {
                        return Err(invalid("MCP catalogue pagination limit"));
                    }
                    cursor = Some(next.to_owned());
                } else {
                    break;
                }
            }
            let schema_hash = format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&tools)
                        .map_err(|_| invalid("MCP schema encoding failed"))?
                )
            );
            let mut summary = json!({"state":"protocol-ready","server":server,"protocol_version":PROTOCOL,"tool_count":tools.len(),"schema_sha256":schema_hash,
                "checked_operations":["initialize","notifications/initialized","tools/list"]});
            if catalogue_only {
                if definitions.is_empty() {
                    return Err(invalid("MCP empty tool catalogue"));
                }
                summary["state"] = json!("catalogue-read");
                summary["tools"] = json!(definitions);
                summary["tool_calls_executed"] = json!(false);
            } else {
                match kind {
                    ProbeKind::Nuphus => {
                        if tools.keys().map(String::as_str).collect::<BTreeSet<_>>()
                            != NUPHUS_TOOLS.iter().copied().collect()
                        {
                            return Err(invalid("Nuphus 38-tool contract incompatible"));
                        }
                        let mut missing_types = 0;
                        for (name, other) in [
                            ("browser_click", None),
                            ("browser_type", Some("text")),
                            ("browser_drag_files", Some("file_paths")),
                        ] {
                            let schema = &tools[name];
                            if name == "browser_type"
                                && schema["properties"]["text"]["type"] != "string"
                                || name == "browser_drag_files"
                                    && (schema["properties"]["file_paths"]["type"] != "array"
                                        || schema["properties"]["file_paths"]["items"]["type"]
                                            != "string"
                                        || schema["properties"]["file_paths"]["minItems"] != 1)
                            {
                                return Err(invalid(
                                    "Nuphus selector/ref payload schema incompatible",
                                ));
                            }
                            for key in ["selector", "ref"] {
                                if schema["properties"][key]["type"] != "string"
                                    || schema["properties"][key]["minLength"] != 1
                                {
                                    return Err(invalid("Nuphus selector/ref schema incompatible"));
                                }
                            }
                            let required = schema["required"]
                                .as_array()
                                .ok_or_else(|| invalid("Nuphus required schema missing"))?;
                            if required != &other.into_iter().map(|s| json!(s)).collect::<Vec<_>>()
                            {
                                return Err(invalid("Nuphus required schema incompatible"));
                            }
                            let branches = schema["anyOf"]
                                .as_array()
                                .filter(|a| a.len() == 2)
                                .ok_or_else(|| {
                                    invalid("Nuphus selector/ref alternatives missing")
                                })?;
                            for (branch, key) in branches.iter().zip(["selector", "ref"]) {
                                if branch["required"] != json!([key])
                                    || branch.get("type").is_some_and(|v| v != "object")
                                {
                                    return Err(invalid(
                                        "Nuphus selector/ref alternatives incompatible",
                                    ));
                                }
                                if branch.get("type").is_none() {
                                    missing_types += 1;
                                }
                            }
                        }
                        summary["selector_ref_branch_types_missing"] = json!(missing_types);
                    }
                    ProbeKind::CodebaseMemory => {
                        for name in [
                            "index_repository",
                            "search_graph",
                            "query_graph",
                            "get_graph_schema",
                            "list_projects",
                        ] {
                            if !tools.contains_key(name) {
                                return Err(invalid("Codebase Memory required operation missing"));
                            }
                        }
                        let index = &tools["index_repository"]["properties"];
                        let path_key = ["repo_path", "path", "repository_path"]
                            .into_iter()
                            .find(|k| index[*k]["type"] == "string")
                            .ok_or_else(|| {
                                invalid("Codebase Memory index path contract incompatible")
                            })?;
                        for (name, keys) in [
                            ("query_graph", &["project", "query"][..]),
                            ("get_graph_schema", &["project"][..]),
                        ] {
                            if keys
                                .iter()
                                .any(|k| tools[name]["properties"][*k]["type"] != "string")
                            {
                                return Err(invalid(
                                    "Codebase Memory schema contract incompatible",
                                ));
                            }
                        }
                        let fixture = root.join("fixture");
                        fs::create_dir(&fixture)
                            .map_err(|_| invalid("MCP indexing fixture creation failed"))?;
                        fs::write(
                            fixture.join("probe.rs"),
                            "pub fn inventory_probe(value: i32) -> i32 { value + 1 }\n",
                        )
                        .map_err(|_| invalid("MCP indexing fixture creation failed"))?;
                        let mode = &index["mode"];
                        let admits_fast = mode["type"] == "string"
                            && mode["enum"]
                                .as_array()
                                .is_some_and(|values| values.iter().any(|v| v == "fast"));
                        if !admits_fast {
                            return Err(invalid(
                                "Codebase Memory index mode contract incompatible",
                            ));
                        }
                        self.call("index_repository", json!({path_key:fixture,"mode":"fast"}))?;
                        let projects = payload(&self.call("list_projects", json!({}))?)?;
                        let projects = if projects.is_array() {
                            projects.as_array()
                        } else {
                            if projects.get("total").is_some_and(|v| v != 1)
                                || projects.get("has_more").is_some_and(|v| v != false)
                            {
                                return Err(invalid(
                                    "Codebase Memory disposable project isolation failed",
                                ));
                            }
                            projects["projects"].as_array()
                        }
                        .filter(|a| a.len() == 1)
                        .ok_or_else(|| {
                            invalid("Codebase Memory disposable project isolation failed")
                        })?;
                        let project = &projects[0];
                        let name = project["name"]
                            .as_str()
                            .or_else(|| project["project"].as_str())
                            .filter(|s| !s.is_empty() && s.len() <= 256)
                            .ok_or_else(|| invalid("Codebase Memory project identity missing"))?;
                        let observed = project["root_path"]
                            .as_str()
                            .ok_or_else(|| invalid("Codebase Memory project root missing"))?;
                        if Path::new(observed)
                            .canonicalize()
                            .map_err(|_| invalid("Codebase Memory project root unavailable"))?
                            != fixture
                                .canonicalize()
                                .map_err(|_| invalid("Codebase Memory fixture root unavailable"))?
                        {
                            return Err(invalid("Codebase Memory project root mismatch"));
                        }
                        self.call("get_graph_schema", json!({"project":name}))?;
                        let query = payload(&self.call("query_graph", json!({"project":name,"query":"MATCH (n:Function) WHERE n.name = 'inventory_probe' RETURN n.name","format":"json"}))?)?;
                        if query["rows"] != json!([["inventory_probe"]])
                            || query["columns"] != json!(["n.name"])
                            || query["total"] != 1
                        {
                            return Err(invalid("Codebase Memory disposable graph query failed"));
                        }
                        summary["state"] = json!("passed");
                        summary["checked_operations"] = json!([
                            "initialize",
                            "notifications/initialized",
                            "tools/list",
                            "index_repository",
                            "list_projects",
                            "get_graph_schema",
                            "query_graph"
                        ]);
                        summary["isolated_project_count"] = json!(1);
                    }
                }
            }
            // EOF is the stdio shutdown contract. Inspect all remaining output,
            // including data buffered after the final wanted reply.
            self.input.take();
            while let Some(value) = self.next()? {
                if !self.notification(&value)? {
                    return Err(invalid("MCP unexpected response after completion"));
                }
            }
            Ok(summary)
        }
    }

    fn validate_schema(schema: &Value) -> io::Result<()> {
        if schema["type"] != "object" || !schema["properties"].is_object() {
            return Err(invalid("MCP tool input schema incompatible"));
        }
        if let Some(required) = schema.get("required") {
            let required = required
                .as_array()
                .ok_or_else(|| invalid("MCP required schema incompatible"))?;
            let mut seen = BTreeSet::new();
            for key in required {
                let key = key
                    .as_str()
                    .ok_or_else(|| invalid("MCP required schema incompatible"))?;
                if schema["properties"].get(key).is_none() || !seen.insert(key) {
                    return Err(invalid("MCP required schema incompatible"));
                }
            }
        }
        Ok(())
    }
    fn payload(result: &Value) -> io::Result<Value> {
        if let Some(value) = result.get("structuredContent") {
            if value.is_object() || value.is_array() {
                return Ok(value.clone());
            }
            return Err(invalid("MCP structured tool result incompatible"));
        }
        let content = result["content"]
            .as_array()
            .ok_or_else(|| invalid("MCP tool content missing"))?;
        let texts: Vec<_> = content.iter().filter(|v| v["type"] == "text").collect();
        if texts.len() != 1 {
            return Err(invalid("MCP tool text result ambiguous"));
        }
        strict_json(
            texts[0]["text"]
                .as_str()
                .ok_or_else(|| invalid("MCP tool text missing"))?
                .as_bytes(),
        )
    }

    // serde_json::Value normally accepts duplicate object keys, including two
    // conflicting IDs. Reject them at every depth instead of trusting the last.
    pub(crate) fn strict_json(bytes: &[u8]) -> io::Result<Value> {
        use serde::{
            Deserialize, Deserializer,
            de::{self, MapAccess, SeqAccess, Visitor},
        };
        struct Strict(Value);
        impl<'de> Deserialize<'de> for Strict {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct V;
                impl<'de> Visitor<'de> for V {
                    type Value = Strict;
                    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                        f.write_str("JSON with unique keys")
                    }
                    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
                        Ok(Strict(v.into()))
                    }
                    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
                        Ok(Strict(v.into()))
                    }
                    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
                        Ok(Strict(v.into()))
                    }
                    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Strict, E> {
                        Ok(Strict(json!(v)))
                    }
                    fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
                        Ok(Strict(v.into()))
                    }
                    fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
                        Ok(Strict(Value::Null))
                    }
                    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                        let mut values = Vec::new();
                        while let Some(Strict(v)) = a.next_element()? {
                            values.push(v);
                        }
                        Ok(Strict(values.into()))
                    }
                    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                        let mut values = serde_json::Map::new();
                        while let Some(k) = a.next_key::<String>()? {
                            if values.contains_key(&k) {
                                return Err(de::Error::custom("duplicate JSON key"));
                            }
                            values.insert(k, a.next_value::<Strict>()?.0);
                        }
                        Ok(Strict(values.into()))
                    }
                }
                d.deserialize_any(V)
            }
        }
        serde_json::from_slice::<Strict>(bytes)
            .map(|s| s.0)
            .map_err(|_| invalid("MCP malformed JSON output"))
    }

    fn remove_private_state(path: &Path) -> io::Result<()> {
        let deadline = Deadline::after(CLEANUP)?;
        loop {
            match fs::remove_dir_all(path) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) if deadline.expired() => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "MCP private state cleanup failed (os error {}); retained for recovery",
                            error.raw_os_error().unwrap_or(-1)
                        ),
                    ));
                }
                Err(_) => std::thread::sleep(POLL.min(deadline.remaining())),
            }
        }
    }

    pub(super) fn run(
        executable: &Path,
        kind: ProbeKind,
        expected: &str,
        caller: &Cancellation,
        catalogue_only: bool,
    ) -> io::Result<Value> {
        let deadline = Deadline::after(TIMEOUT)?;
        check_stop(deadline, caller)?;
        let _pins = artifact(executable, expected, deadline, caller)?;
        let state = private_directory()?;
        let lease = state.path().join("owner");
        fs::write(&lease, b"dependency-mcp-probe")
            .map_err(|_| invalid("MCP state ownership setup failed"))?;
        let state_pins = pin_path(&lease)?;
        let mut command = CommandSpec::new(executable);
        command.current_dir = Some(state.path().into());
        environment(&mut command, state.path())?;
        if kind == ProbeKind::CodebaseMemory {
            crate::cbm_configuration::initialize_private(&state.path().join("cbm"))?;
        }
        let (input, writer) = pipe()?;
        let (reader, output) = pipe()?;
        let (mut errors, stderr) = pipe()?;
        command.stdin = Some(input);
        command.stdout = Some(output);
        command.stderr = Some(stderr);
        let memory = match kind {
            ProbeKind::CodebaseMemory => 2 * 1024 * 1024 * 1024usize,
            ProbeKind::Nuphus => 512 * 1024 * 1024,
        };
        let job = Job::new(Limits {
            memory_bytes: Some(memory),
            cpu_percent: Some(25.0),
        })
        .map_err(|_| invalid("MCP Job creation failed"))?;
        let suspended = job
            .spawn_suspended(&command)
            .map_err(|_| invalid("MCP contained process creation failed"))?;
        let child = suspended
            .resume()
            .map_err(|_| invalid("MCP contained process resume failed"))?;
        drop(command);
        // This token is internal; a protocol failure must not cancel its caller.
        let stop = Cancellation::default();
        let root = state.path().to_owned();
        let cancelled = stop.clone();
        let worker = std::thread::Builder::new()
            .name("dependency-mcp-stdio".into())
            .spawn(move || {
                let mut channel = Channel {
                    input: Some(writer),
                    output: BufReader::new(reader),
                    bytes: 0,
                    sent: 0,
                    notifications: 0,
                    id: 0,
                    deadline,
                    cancel: cancelled.clone(),
                };
                let result = channel.exchange(kind, &root, catalogue_only);
                if result.is_err() {
                    cancelled.cancel();
                }
                result
            })
            .map_err(|_| invalid("MCP protocol worker unavailable"))?;
        let cancelled = stop.clone();
        let stderr_worker = std::thread::Builder::new()
            .name("dependency-mcp-stderr".into())
            .spawn(move || {
                let mut buffer = [0; 4096];
                let mut total = 0;
                loop {
                    match errors.read(&mut buffer) {
                        Ok(0) => return Ok(()),
                        Ok(count) => {
                            total += count;
                            if total > STDERR_LIMIT {
                                cancelled.cancel();
                                return Err(invalid("MCP stderr byte limit"));
                            }
                        }
                        Err(_) => {
                            cancelled.cancel();
                            return Err(invalid("MCP stderr pipe read failed"));
                        }
                    }
                }
            });
        let stderr_worker = match stderr_worker {
            Ok(worker) => worker,
            Err(_) => {
                let _ = job.terminate(130, CLEANUP);
                let _ = worker.join();
                return Err(invalid("MCP stderr worker unavailable"));
            }
        };
        // Bridge caller cancellation without changing CommandSpec/Job APIs.
        let bridge_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let monitor = {
            let done = bridge_stop.clone();
            let caller = caller.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("dependency-mcp-cancel".into())
                .spawn(move || {
                    while !done.load(std::sync::atomic::Ordering::Relaxed) {
                        if caller.is_cancelled() {
                            stop.cancel();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                })
        };
        if monitor.is_err() {
            stop.cancel();
        }
        let outcome = job.wait(&child, deadline, &stop, CLEANUP);
        bridge_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Ok(monitor) = monitor {
            let _ = monitor.join();
        }
        // wait() consumes the Job. On error, Drop still closes the kill-on-close
        // handle, so child pipe ends die and these joins cannot block on a live tree.
        let response = worker
            .join()
            .map_err(|_| invalid("MCP protocol worker failed"));
        let stderr = stderr_worker
            .join()
            .map_err(|_| invalid("MCP stderr worker failed"));
        // Handles, including inherited descendant pipe endpoints, are now closed.
        drop(state_pins);
        // Retain the native filesystem error code: TempDir::close wraps it with
        // a path context, which also makes raw_os_error unavailable here.
        let leftover = state.keep();
        let first_cleanup = fs::remove_dir_all(&leftover);
        let cleanup_retried = first_cleanup
            .as_ref()
            .err()
            .is_some_and(|error| error.kind() != io::ErrorKind::NotFound);
        let cleanup_os_error = first_cleanup
            .as_ref()
            .err()
            .and_then(io::Error::raw_os_error);
        let cleanup = match first_cleanup {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => remove_private_state(&leftover),
        };
        let result = (|| {
            let outcome = match outcome {
                Ok(value) => Ok(value),
                Err(_) => Err(invalid("MCP owned tree cleanup failed")),
            };
            if caller.is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "MCP probe cancelled",
                ));
            }
            let outcome = outcome?;
            match outcome.reason {
                StopReason::Timeout => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "MCP probe deadline exceeded",
                    ));
                }
                StopReason::MemoryLimit => return Err(invalid("MCP Job memory limit")),
                _ => {}
            }
            stderr??;
            // Keep the protocol/resource reason if closing a failed exchange also
            // makes the peer exit nonzero. A bare closed stream is instead explained
            // by an observed natural nonzero exit. Never include the foreign error.
            let mut summary = match response? {
                Ok(value) => value,
                Err(error) => {
                    if ![0, 130].contains(&outcome.process_exit_code)
                        && matches!(
                            error.to_string().as_str(),
                            "MCP EOF before response"
                                | "MCP input pipe closed"
                                | "MCP output pipe read failed"
                        )
                    {
                        return Err(invalid("MCP process did not exit successfully"));
                    }
                    return Err(error);
                }
            };
            if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
                return Err(invalid("MCP process did not exit successfully"));
            }
            summary["artifact_sha256"] = json!(expected.to_ascii_lowercase());
            summary["limits"] = json!({"wall_time_ms":TIMEOUT.as_millis(),"cleanup_ms":CLEANUP.as_millis(),"private_cleanup_ms":CLEANUP.as_millis(),"job_memory_bytes":memory,"job_cpu_percent":25,
            "stdout_bytes":STDOUT_LIMIT,"stderr_bytes":STDERR_LIMIT,"frame_bytes":LINE_LIMIT,"request_bytes":REQUEST_LIMIT,"notifications":NOTIFICATION_LIMIT,"tools":TOOL_LIMIT,"catalogue_pages":8});
            summary["isolation"] = json!({"assigned_before_execution":true,"private_environment":true,"owned_tree_stopped":outcome.job.active_processes==0,"temporary_state_removed":true,
            "private_cleanup_retried":cleanup_retried,"private_cleanup_initial_os_error":cleanup_os_error,
            "model_downloads_disabled":true,"desktop_browser_calls":false,"sandbox":false,"dll_provenance":"caller-audited"});
            Ok(summary)
        })();
        match (result, cleanup) {
            (Err(primary), Err(cleanup)) => Err(io::Error::new(
                primary.kind(),
                format!("{primary}; {cleanup}"),
            )),
            (Err(primary), Ok(())) => Err(primary),
            (Ok(_), Err(cleanup)) => Err(cleanup),
            (Ok(summary), Ok(())) => Ok(summary),
        }
    }
}
