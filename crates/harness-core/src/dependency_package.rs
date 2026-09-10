//! Read-only identities for external packages. No package entrypoint is executed.
#![cfg(windows)]

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
};

const DOCUMENT_LIMIT: u64 = 4 * 1024 * 1024;
const PAYLOAD_LIMIT: u64 = 512 * 1024 * 1024;

pub(crate) fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Dependency observation is unavailable or incompatible",
    )
}

pub(crate) fn plain(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

/// Resolve existing ancestors, retaining a missing leaf without following broken links.
pub(crate) fn resolved(path: &Path) -> io::Result<PathBuf> {
    let path = std::path::absolute(path)?;
    let mut cursor = path.as_path();
    let mut missing = Vec::new();
    loop {
        match fs::canonicalize(cursor) {
            Ok(found) => {
                let mut result = plain(&found);
                for part in missing.iter().rev() {
                    result.push(part);
                }
                return Ok(result);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if fs::symlink_metadata(cursor).is_ok() || missing.len() == 64 {
                    return Err(invalid());
                }
                missing.push(cursor.file_name().ok_or_else(invalid)?.to_os_string());
                cursor = cursor.parent().ok_or_else(invalid)?;
            }
            Err(error) => return Err(error),
        }
    }
}

pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
    let left: Vec<_> = left.as_os_str().encode_wide().collect();
    let right: Vec<_> = right.as_os_str().encode_wide().collect();
    if left.len() > i32::MAX as usize || right.len() > i32::MAX as usize {
        return false;
    }
    unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        ) == CSTR_EQUAL
    }
}

pub(crate) fn contained(path: &Path, root: &Path) -> bool {
    path.ancestors().any(|ancestor| same_path(ancestor, root))
}

pub(crate) fn package_file(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let path = resolved(&root.join(relative))?;
    if !contained(&path, root) {
        return Err(invalid());
    }
    Ok(path)
}

fn open(path: &Path) -> io::Result<File> {
    // Observe one object while refusing concurrent writers/deletion of that file.
    fs::OpenOptions::new().read(true).share_mode(1).open(path)
}

fn read_bytes(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let file = match open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() || file.metadata()?.len() > DOCUMENT_LIMIT {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(DOCUMENT_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > DOCUMENT_LIMIT {
        return Err(invalid());
    }
    Ok(Some(bytes))
}

pub(crate) fn read_text(path: &Path) -> io::Result<Option<String>> {
    let Some(bytes) = read_bytes(path)? else {
        return Ok(None);
    };
    let text = String::from_utf8(bytes).map_err(|_| invalid())?;
    Ok(Some(
        text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned(),
    ))
}

fn json_document(path: &Path) -> io::Result<Option<(Value, String)>> {
    let Some(bytes) = read_bytes(path)? else {
        return Ok(None);
    };
    let raw = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    let data =
        serde_json::from_str(raw.strip_prefix('\u{feff}').unwrap_or(raw)).map_err(|_| invalid())?;
    Ok(Some((data, crate::build_identity::hash_bytes(&bytes))))
}

pub(crate) fn read_json(path: &Path) -> io::Result<Option<Value>> {
    read_text(path)?
        .map(|text| serde_json::from_str(&text).map_err(|_| invalid()))
        .transpose()
}

pub(crate) fn fingerprint(path: &Path) -> io::Result<String> {
    let mut file = open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > PAYLOAD_LIMIT {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > PAYLOAD_LIMIT {
            return Err(invalid());
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(crate) fn text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|text| text.len() <= 4096 && !text.chars().any(char::is_control))
}

pub(crate) fn base(spec: &Value) -> Value {
    json!({"id":spec["id"],"identity":spec["package"],"manager":spec["manager"],
        "version":null,"executable":null,"command":[],"installation_root":null,
        "status":"missing","ownership":"not-found","provenance":{"source":spec["source"]},
        "health":{"installed":false,"identity_verified":false,"integrity":"unknown","callable":null,"checked_operations":[]},
        "update_safe":false,"active_consumers":{"state":"not-checked","processes":[]},
        "evidence":[],"candidates":[],"verification":{"state":"unverified","evidence":[]}})
}

pub(crate) fn select(mut record: Value, candidates: Vec<Value>) -> Value {
    let mut distinct: Vec<Value> = Vec::new();
    for candidate in candidates {
        let duplicate = distinct.iter().any(|prior| {
            match (
                text(&prior["installation_root"]),
                text(&candidate["installation_root"]),
            ) {
                (Some(a), Some(b)) => {
                    same_path(Path::new(a), Path::new(b))
                        && prior["command"] == candidate["command"]
                }
                _ => false,
            }
        });
        if !duplicate {
            distinct.push(candidate);
        }
    }
    let usable: Vec<_> = distinct
        .iter()
        .filter(|candidate| candidate["status"] == "adopted")
        .collect();
    let chosen = if usable.len() == 1 {
        usable.first().copied()
    } else if distinct.len() == 1 {
        distinct.first()
    } else {
        None
    };
    if let Some(chosen) = chosen {
        let source = record["provenance"]["source"].clone();
        if let Some(fields) = chosen.as_object() {
            record.as_object_mut().unwrap().extend(fields.clone());
            if record["provenance"]["source"].is_null() {
                record["provenance"]["source"] = source;
            }
        }
    } else if !distinct.is_empty() {
        record["status"] = json!("ambiguous");
        record["ownership"] = json!("selection-required");
        record["evidence"].as_array_mut().unwrap().push(json!({"kind":"selection-conflict","detail":"Multiple distinct installations; explicit selection required."}));
    }
    record["candidates"] = json!(distinct);
    record
}

pub(crate) fn npm(
    package: &str,
    node_modules: &Path,
    node: Option<&Path>,
    command_name: Option<&str>,
    owner: &str,
) -> io::Result<Option<Value>> {
    let root = resolved(&node_modules.join(package))?;
    let manifest = package_file(&root, "package.json")?;
    let Some((data, manifest_hash)) = json_document(&manifest)? else {
        return Ok(None);
    };
    if data["name"] != package {
        return Ok(None);
    }
    let version = text(&data["version"]).ok_or_else(invalid)?;
    let entry = match &data["bin"] {
        Value::String(bin)
            if command_name.is_none_or(|command| Some(command) == package.rsplit('/').next()) =>
        {
            Some(bin.as_str())
        }
        Value::Object(bins) => command_name
            .and_then(|name| bins.get(name))
            .or_else(|| {
                if command_name.is_none() {
                    bins.values().next()
                } else {
                    None
                }
            })
            .and_then(text),
        _ => None,
    };
    let script = entry.map(|entry| resolved(&root.join(entry))).transpose()?;
    let safe = script
        .as_deref()
        .is_some_and(|script| contained(script, &root) && script.is_file());
    let installed = safe && node.is_some_and(Path::is_file);
    let mut evidence =
        vec![json!({"kind":"npm-package-identity","path":manifest,"sha256":manifest_hash})];
    if safe {
        let script = script.as_ref().unwrap();
        evidence.push(
            json!({"kind":"entrypoint-fingerprint","path":script,"sha256":fingerprint(script)?}),
        );
    }
    evidence.push(json!({"kind":"limitation","detail":"No trusted per-file npm checksum inventory; compare official tarball before replacement."}));
    let mut record = json!({"manager":if owner == "npm-global" {"npm"} else {owner},"version":version,
        "executable":node,"command":if installed {json!([node,script])} else {json!([])},
        "paths":{"node":node,"module_root":root,"entrypoint":script},"installation_root":root,
        "status":if installed {"adopted"} else {"broken"},"ownership":"adopted-shared","update_safe":false,
        "health":{"installed":installed,"identity_verified":true,"integrity":"unknown","callable":null,"checked_operations":[]},
        "provenance":{"metadata":manifest,"package_identity":package,"entrypoint":script},"evidence":evidence});
    if package == "codebase-memory-mcp" {
        let native = resolved(&root.join("bin/codebase-memory-mcp.exe"))?;
        let available = contained(&native, &root) && native.is_file();
        record["paths"]["native_executable"] = json!(native);
        record["executable"] = json!(native);
        record["command"] = if available {
            json!([native])
        } else {
            json!([])
        };
        record["status"] = json!(if available { "adopted" } else { "broken" });
        record["health"]["installed"] = json!(available);
        record["evidence"].as_array_mut().unwrap().push(if available {
            json!({"kind":"native-payload-fingerprint","path":native,"sha256":fingerprint(&native)?})
        } else { json!({"kind":"missing-native-payload","path":native}) });
    } else if package == "@nuphus/nuphus-mcp" {
        let companion_name = "@nuphus/nuphus-mcp-win32-x64";
        let mut chosen = None;
        for relative in [
            root.join("node_modules").join(companion_name),
            node_modules.join(companion_name),
        ] {
            let companion = resolved(&relative)?;
            let Some(metadata) = read_json(&package_file(&companion, "package.json")?)? else {
                continue;
            };
            if metadata["name"] != companion_name || metadata["version"] != version {
                continue;
            }
            let binary = resolved(&companion.join("bin/nuphus-mcp.exe"))?;
            let patched = resolved(&companion.join("bin/nuphus-mcp-schema-fixed.exe"))?;
            if !contained(&binary, &companion) || !binary.is_file() {
                continue;
            }
            let variant = contained(&patched, &companion) && patched.is_file();
            let native = if variant { &patched } else { &binary };
            record["paths"]["native_executable"] = json!(native);
            record["paths"]["original_native_executable"] = json!(binary);
            record["provenance"]["platform_package"] = json!(companion_name);
            record["provenance"]["platform_version"] = metadata["version"].clone();
            record["evidence"].as_array_mut().unwrap().push(json!({"kind":"native-payload-fingerprint","path":native,"sha256":fingerprint(native)?}));
            if variant {
                record["status"] = json!("modified");
                record["health"]["integrity"] = json!("modified");
                record["evidence"].as_array_mut().unwrap().push(json!({"kind":"local-variant","detail":"Schema-fixed executable selected by the installed wrapper; preserve and audit before replacement."}));
            }
            chosen = Some(native.clone());
            break;
        }
        // The actual MCP consumer must use the original native payload through
        // the owning Rust protocol adapter, never an install-capable JS shim.
        record["executable"] = json!(chosen);
        record["command"] = chosen
            .as_ref()
            .map_or_else(|| json!([]), |path| json!([path]));
        record["health"]["installed"] = json!(chosen.is_some());
        if chosen.is_none() {
            record["status"] = json!("broken");
            record["evidence"]
                .as_array_mut()
                .unwrap()
                .push(json!({"kind":"missing-platform-payload","identity":companion_name}));
        } else if record["status"] != "modified" {
            record["status"] = json!("adopted");
        }
    }
    Ok(Some(record))
}
