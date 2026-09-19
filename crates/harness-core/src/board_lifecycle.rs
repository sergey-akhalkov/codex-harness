//! Native `--board-only` Check/Install/Disconnect/Recover for pinned `bd`.
#![cfg(windows)]

use crate::{
    board_cli::{self, WINDOWS_AMD64_ARCHIVE, WINDOWS_AMD64_SHA256},
    build_identity, dependency_archive, dependency_assets,
    dependency_fetch::Client,
    installation_lock::InstallationLocks,
    installation_state::normal,
    inventory, native_build,
    registration_native::{self as native, FileGuard, StagedFile, StagedLink},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs, io,
    io::Read,
    path::{Path, PathBuf},
};

pub struct Request {
    pub source: PathBuf,
    pub codex_home: PathBuf,
    pub user_home: PathBuf,
    pub preview: bool,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub status: &'static str,
    pub model_calls: u32,
    pub bd_version: Option<String>,
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

fn read_json(path: &Path) -> io::Result<Option<Value>> {
    inventory::ordinary_parents(path)?;
    match fs::read(path) {
        Ok(bytes) => {
            Ok(Some(serde_json::from_slice(&bytes).map_err(|_| {
                conflict("board state is not JSON; preserving it")
            })?))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(conflict(&format!(
            "board destination changed; preserving {}",
            path.display()
        ))),
    }
}

fn json_bytes(value: &Value) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    let after = json_bytes(value)?;
    match FileGuard::read_regular(path) {
        Ok((guard, current)) => {
            let identity = guard.object_identity()?;
            drop(guard);
            FileGuard::replace_regular(path, &identity, &current, &after)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            StagedFile::create(path, &after)?.commit()
        }
        Err(error) => Err(error),
    }
}

fn delete_regular(path: &Path) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    match FileGuard::read_regular(path) {
        Ok((guard, _)) => guard.remove(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn local_drive(path: &Path) -> io::Result<PathBuf> {
    let text = path
        .to_str()
        .ok_or_else(|| conflict("board path is not UTF-8"))?;
    normal(Path::new(text.strip_prefix(r"\\?\").unwrap_or(text)))
}

fn path_text(path: &Path) -> io::Result<String> {
    local_drive(path)?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| conflict("board path is not UTF-8"))
}

fn current_target(path: &Path) -> io::Result<Option<PathBuf>> {
    match FileGuard::capture_link(path) {
        Ok(guard) => {
            let (target, directory) = guard.link_description()?;
            if directory {
                return Err(conflict(
                    "board destination is a directory link; preserving it",
                ));
            }
            Ok(Some(target))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn same_target(left: &Path, right: &Path) -> io::Result<bool> {
    native::targets_match(left, right)
}

fn owned_bin(home: &Path, destination: &Path) -> io::Result<PathBuf> {
    let destination = local_drive(destination)?;
    let bin = local_drive(&home.join("harness/bin"))?;
    if destination.parent() == Some(bin.as_path())
        && destination.file_name().and_then(OsStr::to_str) == Some("bd.exe")
    {
        Ok(destination)
    } else {
        Err(conflict(
            "board destination is outside its owned connections; preserving it",
        ))
    }
}

fn disconnect_link(destination: &Path, expected: &Path) -> io::Result<()> {
    match current_target(destination)? {
        Some(current) if same_target(&current, expected)? => {
            native::verified_link(destination, expected, false)?.remove()
        }
        Some(_) => Err(conflict(&format!(
            "Cannot restore changed destination: {}",
            destination.display()
        ))),
        None => Ok(()),
    }
}

fn definition(source: &Path) -> io::Result<Value> {
    let path = source.join("global/board.json");
    inventory::ordinary_parents(&path)?;
    let bytes = fs::read(&path).map_err(|_| conflict("board selection record is missing"))?;
    serde_json::from_slice(&bytes).map_err(|_| conflict("board selection record is not JSON"))
}

fn require_file(path: &Path, expected: &str, label: &str) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    build_identity::ordinary(path)?;
    if !path.is_file() {
        return Err(board_cli::unavailable(&format!("{label} is missing")));
    }
    let actual = build_identity::hash_file(path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(conflict(&format!(
            "{label} identity mismatch; preserving the unexpected file"
        )));
    }
    Ok(())
}

fn board_url(definition: &Value) -> io::Result<&str> {
    let url = definition["url"]
        .as_str()
        .ok_or_else(|| conflict("board download URL is missing"))?;
    let version = definition["version"]
        .as_str()
        .ok_or_else(|| conflict("board version is missing"))?;
    let expected = format!(
        "https://github.com/gastownhall/beads/releases/download/v{version}/{WINDOWS_AMD64_ARCHIVE}"
    );
    if url != expected {
        return Err(conflict(
            "board download URL is not the pinned official Windows x64 archive",
        ));
    }
    Ok(url)
}

struct Capture<'a> {
    input: &'a mut dyn Read,
    hash: Sha256,
}

impl Read for Capture<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(output)?;
        self.hash.update(&output[..count]);
        Ok(count)
    }
}

fn acquire_bd(home: &Path, definition: &Value) -> io::Result<PathBuf> {
    let version = definition["version"]
        .as_str()
        .ok_or_else(|| conflict("board version is missing"))?;
    let executable_hash = definition["executableSha256"]
        .as_str()
        .ok_or_else(|| conflict("board executable hash is missing"))?;
    let package = home.join(format!("harness/board/packages/{version}"));
    let bd = package.join("bd.exe");
    inventory::ordinary_parents(&bd)?;
    native_build::ordinary_ancestors(&package)?;
    if bd.is_file() {
        require_file(&bd, executable_hash, "bd binary")?;
        return Ok(bd);
    }
    fs::create_dir_all(&package)?;
    let archive_hash = definition["sha256"]
        .as_str()
        .ok_or_else(|| conflict("board archive hash is missing"))?;
    if archive_hash != WINDOWS_AMD64_SHA256 {
        return Err(conflict(
            "board archive hash is not the pinned Windows x64 checksum",
        ));
    }
    let url = board_url(definition)?;
    let client = Client::new().map_err(|error| {
        board_cli::unavailable(&format!("board download client is unavailable: {error}"))
    })?;
    let bytes = client
        .github_asset(&dependency_assets::Asset {
            url: url.to_owned(),
            sha256: archive_hash.to_ascii_lowercase(),
            size: 128 * 1024 * 1024,
            id: 1,
        })
        .map_err(|error| {
            board_cli::unavailable(&format!("board archive download failed: {error}"))
        })?;
    if build_identity::hash_bytes(&bytes) != archive_hash.to_ascii_lowercase() {
        return Err(conflict(
            "board release checksum mismatch; dependency not installed.",
        ));
    }
    extract_bd_archive(&package, &bd, &bytes, executable_hash)
}

fn extract_bd_archive(
    package: &Path,
    bd: &Path,
    bytes: &[u8],
    executable_hash: &str,
) -> io::Result<PathBuf> {
    let mut extracted = None;
    dependency_archive::visit_zip(bytes, |name, size, reader| {
        if Path::new(name).file_name() != Some(OsStr::new("bd.exe")) {
            return Ok(());
        }
        if extracted.is_some() {
            return Err(conflict("board archive contains more than one bd.exe"));
        }
        native_build::ordinary_ancestors(package)?;
        let mut capture = Capture {
            input: reader,
            hash: Sha256::new(),
        };
        StagedFile::from_reader(bd, &mut capture, size)?.commit()?;
        extracted = Some(format!("{:x}", capture.hash.finalize()));
        Ok(())
    })?;
    let extracted = extracted.ok_or_else(|| conflict("board archive is missing bd.exe"))?;
    if extracted != executable_hash.to_ascii_lowercase() {
        let _ = delete_regular(bd);
        return Err(conflict(
            "bd binary identity mismatch; extracted candidate was not kept.",
        ));
    }
    require_file(bd, executable_hash, "bd binary")?;
    Ok(bd.to_path_buf())
}

fn optional_path(value: &Value) -> io::Result<Option<PathBuf>> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => Ok(Some(PathBuf::from(text))),
        _ => Err(conflict("board pending path is not a string")),
    }
}

pub fn check(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let pending = home.join("harness/board-pending.json");
    inventory::ordinary_parents(&pending)?;
    if pending.exists() {
        return Err(conflict(
            "Board transaction is pending; use --board-only Recover.",
        ));
    }
    if request.preview {
        return Ok(Report {
            status: "Preview board Check",
            model_calls: 0,
            bd_version: None,
        });
    }
    let Some(state) = read_json(&home.join("harness/board.json"))? else {
        return Err(board_cli::unavailable("bd is not installed"));
    };
    if state["enabled"] != true {
        return Err(board_cli::unavailable("bd is not installed"));
    }
    let links = state["links"]
        .as_array()
        .ok_or_else(|| conflict("board links are missing"))?;
    for link in links {
        let destination = PathBuf::from(
            link["destination"]
                .as_str()
                .ok_or_else(|| conflict("board destination is missing"))?,
        );
        let source = PathBuf::from(
            link["source"]
                .as_str()
                .ok_or_else(|| conflict("board source is missing"))?,
        );
        owned_bin(&home, &destination)?;
        let expected = link["sha256"]
            .as_str()
            .ok_or_else(|| conflict("board hash is missing"))?;
        let target = fs::read_link(&destination).map_err(|_| {
            board_cli::unavailable(&format!("bd artifact mismatch: {}", destination.display()))
        })?;
        if target != source || !source.is_file() {
            return Err(board_cli::unavailable(&format!(
                "bd artifact mismatch: {}",
                destination.display()
            )));
        }
        if !build_identity::hash_file(&source)?.eq_ignore_ascii_case(expected) {
            return Err(conflict(&format!(
                "bd artifact mismatch: {}",
                destination.display()
            )));
        }
    }
    Ok(Report {
        status: "Board connected",
        model_calls: 0,
        bd_version: state["bdVersion"].as_str().map(str::to_owned),
    })
}

pub fn install(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let source = normal(&request.source)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let pending = home.join("harness/board-pending.json");
    inventory::ordinary_parents(&pending)?;
    if pending.exists() {
        return Err(conflict(
            "Board transaction is pending; use --board-only Recover.",
        ));
    }
    let state = read_json(&home.join("harness/board.json"))?;
    if request.preview {
        return Ok(Report {
            status: "Preview board Install",
            model_calls: 0,
            bd_version: state
                .as_ref()
                .and_then(|value| value["bdVersion"].as_str().map(str::to_owned)),
        });
    }
    let definition = definition(&source)?;
    let version = definition["version"]
        .as_str()
        .ok_or_else(|| conflict("board version is missing"))?;
    let bd = acquire_bd(&home, &definition)?;
    let destination = owned_bin(&home, &home.join("harness/bin/bd.exe"))?;
    let current = current_target(&destination)?;
    let previous = state.as_ref().and_then(|value| {
        value["links"].as_array().and_then(|items| {
            items.iter().find_map(|item| {
                item["destination"].as_str().and_then(|recorded| {
                    same_target(&PathBuf::from(recorded), &destination)
                        .ok()
                        .filter(|matched| *matched)
                        .and_then(|_| item["source"].as_str().map(PathBuf::from))
                })
            })
        })
    });
    if let (Some(current), Some(previous)) = (current.as_ref(), previous.as_ref())
        && !same_target(current, previous)?
    {
        return Err(conflict(&format!(
            "Board target conflict; preserving {}",
            destination.display()
        )));
    }
    if let Some(current) = current.as_ref()
        && previous.is_none()
        && !same_target(current, &bd)?
    {
        return Err(conflict(&format!(
            "Board target conflict; preserving {}",
            destination.display()
        )));
    }
    let mut operations = Vec::new();
    if current
        .as_ref()
        .is_none_or(|current| !same_target(current, &bd).unwrap_or(false))
    {
        operations.push(json!({
            "destination": path_text(&destination)?,
            "oldSource": current.as_ref().map(|path| path_text(path)).transpose()?,
            "newSource": path_text(&bd)?
        }));
    }
    let next_state = json!({
        "schemaVersion": 1,
        "enabled": true,
        "bdVersion": version,
        "links": [{
            "destination": path_text(&destination)?,
            "source": path_text(&bd)?,
            "sha256": build_identity::hash_file(&bd)?
        }]
    });
    write_json(
        &pending,
        &json!({
            "previousState": state.clone().unwrap_or(Value::Null),
            "plannedState": next_state,
            "operations": operations
        }),
    )?;
    for operation in &operations {
        let destination = PathBuf::from(operation["destination"].as_str().unwrap());
        let old = optional_path(&operation["oldSource"])?;
        let new = optional_path(&operation["newSource"])?
            .ok_or_else(|| conflict("board install source is missing"))?;
        if let Some(old) = old.as_deref() {
            disconnect_link(&destination, old)?;
        }
        StagedLink::create(&destination, &new, false)?.commit()?;
    }
    write_json(&home.join("harness/board.json"), &next_state)?;
    delete_regular(&pending)?;
    Ok(Report {
        status: "Board connected",
        model_calls: 0,
        bd_version: Some(version.to_owned()),
    })
}

pub fn disconnect(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let pending = home.join("harness/board-pending.json");
    inventory::ordinary_parents(&pending)?;
    if pending.exists() {
        return Err(conflict(
            "Board transaction is pending; use --board-only Recover.",
        ));
    }
    let state = read_json(&home.join("harness/board.json"))?;
    if state.as_ref().is_none_or(|value| value["enabled"] != true) {
        return Ok(Report {
            status: "Board disconnected",
            model_calls: 0,
            bd_version: None,
        });
    }
    let state = state.unwrap();
    if request.preview {
        return Ok(Report {
            status: "Preview board Disconnect",
            model_calls: 0,
            bd_version: state["bdVersion"].as_str().map(str::to_owned),
        });
    }
    if let Some(links) = state["links"].as_array() {
        for link in links {
            let destination = PathBuf::from(
                link["destination"]
                    .as_str()
                    .ok_or_else(|| conflict("board destination is missing"))?,
            );
            let source = PathBuf::from(
                link["source"]
                    .as_str()
                    .ok_or_else(|| conflict("board source is missing"))?,
            );
            owned_bin(&home, &destination)?;
            disconnect_link(&destination, &source)?;
        }
    }
    write_json(
        &home.join("harness/board.json"),
        &json!({
            "schemaVersion": 1,
            "enabled": false,
            "bdVersion": state["bdVersion"],
            "links": []
        }),
    )?;
    Ok(Report {
        status: "Board disconnected",
        model_calls: 0,
        bd_version: state["bdVersion"].as_str().map(str::to_owned),
    })
}

pub fn recover(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let pending_path = home.join("harness/board-pending.json");
    inventory::ordinary_parents(&pending_path)?;
    let Some(pending) = read_json(&pending_path)? else {
        return Ok(Report {
            status: "Board recovered",
            model_calls: 0,
            bd_version: None,
        });
    };
    if request.preview {
        return Ok(Report {
            status: "Preview board Recover",
            model_calls: 0,
            bd_version: pending["previousState"]["bdVersion"]
                .as_str()
                .map(str::to_owned),
        });
    }
    if let Some(operations) = pending["operations"].as_array() {
        for operation in operations.iter().rev() {
            let destination = PathBuf::from(
                operation["destination"]
                    .as_str()
                    .ok_or_else(|| conflict("board pending destination is missing"))?,
            );
            owned_bin(&home, &destination)?;
            let old = optional_path(&operation["oldSource"])?;
            let new = optional_path(&operation["newSource"])?;
            if let Some(new) = new.as_deref() {
                let _ = disconnect_link(&destination, new);
            }
            if let Some(old) = old.as_deref() {
                StagedLink::create(&destination, old, false)?.commit()?;
            }
        }
    }
    match &pending["previousState"] {
        Value::Null => delete_regular(&home.join("harness/board.json"))?,
        previous => write_json(&home.join("harness/board.json"), previous)?,
    }
    delete_regular(&pending_path)?;
    Ok(Report {
        status: "Board recovered",
        model_calls: 0,
        bd_version: pending["previousState"]["bdVersion"]
            .as_str()
            .map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(home: &Path, user: &Path, source: &Path) -> Request {
        Request {
            source: source.to_path_buf(),
            codex_home: home.to_path_buf(),
            user_home: user.to_path_buf(),
            preview: false,
        }
    }

    fn staged(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf, Request) {
        let home = std::path::absolute(root).unwrap().join("codex");
        let user = std::path::absolute(root).unwrap().join("user");
        let source = std::path::absolute(root).unwrap().join("source");
        fs::create_dir_all(home.join("harness/bin")).unwrap();
        fs::create_dir_all(home.join("harness/board/packages/1.3.0")).unwrap();
        fs::create_dir_all(source.join("global")).unwrap();
        fs::create_dir_all(&user).unwrap();
        let bd = home.join("harness/board/packages/1.3.0/bd.exe");
        fs::write(&bd, b"pinned-bd").unwrap();
        let hash = build_identity::hash_file(&bd).unwrap();
        fs::write(
            source.join("global/board.json"),
            serde_json::to_vec(&json!({
                "version": "1.3.0",
                "platform": "x86_64-pc-windows-msvc",
                "url": format!(
                    "https://github.com/gastownhall/beads/releases/download/v1.3.0/{WINDOWS_AMD64_ARCHIVE}"
                ),
                "sha256": WINDOWS_AMD64_SHA256,
                "executableSha256": hash
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(home.join("auth.json"), b"secret").unwrap();
        fs::write(home.join("config.toml"), b"model = 'keep'\n").unwrap();
        let req = request(&home, &user, &source);
        (home, user, source, bd, req)
    }

    #[test]
    fn kit_pin_matches_board_json() {
        let pin: Value =
            serde_json::from_slice(include_bytes!("../../../global/board.json")).unwrap();
        assert_eq!(pin["version"], board_cli::VERSION);
        assert_eq!(pin["sha256"], WINDOWS_AMD64_SHA256);
        assert_eq!(
            pin["url"].as_str().unwrap(),
            format!(
                "https://github.com/gastownhall/beads/releases/download/v{}/{WINDOWS_AMD64_ARCHIVE}",
                board_cli::VERSION
            )
        );
    }

    #[test]
    fn board_workflow_skill_is_in_kit_source() {
        let skill = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.agents/skills/board-workflow/SKILL.md");
        let text = fs::read_to_string(skill).unwrap();
        assert!(text.contains("name: board-workflow"));
        assert!(text.contains("bd init --skip-agents"));
        assert!(text.contains("--labels feedback"));
        assert!(text.contains("board unavailable"));
    }

    #[test]
    fn install_preview_does_not_create_bin_links() {
        let root = tempfile::tempdir().unwrap();
        let (home, user, source, _, mut req) = staged(root.path());
        req.preview = true;
        let report = install(&req).unwrap();
        assert_eq!(report.status, "Preview board Install");
        assert!(!home.join("harness/bin/bd.exe").exists());
        assert!(!home.join("harness/board.json").exists());
        assert_eq!(fs::read(home.join("auth.json")).unwrap(), b"secret");
        let _ = (user, source);
    }

    #[test]
    fn install_links_pre_staged_bd_and_preserves_credentials() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, bd, req) = staged(root.path());
        let foreign = home.join("harness/bin/foreign.txt");
        fs::write(&foreign, b"keep").unwrap();
        let report = install(&req).unwrap();
        assert_eq!(report.status, "Board connected");
        assert_eq!(fs::read_link(home.join("harness/bin/bd.exe")).unwrap(), bd);
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        assert_eq!(fs::read(home.join("auth.json")).unwrap(), b"secret");
        assert_eq!(
            fs::read(home.join("config.toml")).unwrap(),
            b"model = 'keep'\n"
        );
        assert!(!home.join("harness/board-pending.json").exists());
        let connected = check(&req).unwrap();
        assert_eq!(connected.status, "Board connected");
    }

    #[test]
    fn install_rejects_checksum_mismatch_without_creating_links() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, bd, req) = staged(root.path());
        fs::write(&bd, b"altered").unwrap();
        let error = install(&req).unwrap_err();
        assert!(error.to_string().contains("identity mismatch"));
        assert!(!home.join("harness/bin/bd.exe").exists());
        assert!(!home.join("harness/board.json").exists());
        assert_eq!(fs::read(home.join("auth.json")).unwrap(), b"secret");
    }

    #[test]
    fn check_reports_unavailable_without_substitution() {
        let root = tempfile::tempdir().unwrap();
        let (home, user, source, _, req) = staged(root.path());
        let error = check(&req).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("board unavailable"));
        assert!(!error.to_string().contains("substitut"));
        let _ = (home, user, source);
    }

    #[test]
    fn disconnect_removes_owned_link_and_preserves_foreign_and_credentials() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, _, req) = staged(root.path());
        install(&req).unwrap();
        let foreign = home.join("harness/bin/foreign.txt");
        fs::write(&foreign, b"keep").unwrap();
        let report = disconnect(&req).unwrap();
        assert_eq!(report.status, "Board disconnected");
        assert!(!home.join("harness/bin/bd.exe").exists());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        assert_eq!(fs::read(home.join("auth.json")).unwrap(), b"secret");
        let error = check(&req).unwrap_err();
        assert!(error.to_string().contains("board unavailable"));
    }

    #[test]
    fn recover_restores_previous_absence() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, bd, req) = staged(root.path());
        fs::write(
            home.join("harness/board-pending.json"),
            serde_json::to_vec(&json!({
                "previousState": null,
                "plannedState": {"enabled": true},
                "operations": [{
                    "destination": home.join("harness/bin/bd.exe"),
                    "oldSource": null,
                    "newSource": bd
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        std::os::windows::fs::symlink_file(&bd, home.join("harness/bin/bd.exe")).unwrap();
        let report = recover(&req).unwrap();
        assert_eq!(report.status, "Board recovered");
        assert!(!home.join("harness/bin/bd.exe").exists());
        assert!(!home.join("harness/board-pending.json").exists());
    }
}
