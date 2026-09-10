//! Pure official-release parsing and compatibility planning.
//! No network, process or filesystem access. A staging action is never proved
//! compatible and never authorizes a shared replacement.
#![cfg(windows)]

use crate::dependency_fetch;
use serde_json::{Value, json};
use std::{collections::BTreeMap, io};

const MAX_SPECS: usize = 32;
const MAX_VERSION: usize = 64;
const MAX_BODY: usize = 16 * 1024 * 1024;
const ROLLBACK: &str =
    "Retain the prior installation and affected manager metadata before promotion.";
const TOOLCHAIN_POLICY: &str =
    "Compare the existing component; never update project Rust toolchains as a side effect.";
const TRANSPORT_REASONS: &[&str] = &[
    "unsupported-metadata-source",
    "metadata-output-unavailable",
    "metadata-output-too-large",
    "metadata-private-directory-unavailable",
    "metadata-private-cleanup-failed",
    "metadata-process-failed",
    "metadata-deadline-or-resource-limit",
    "metadata-http-error",
    "metadata-deadline",
    "metadata-tls-error",
    "metadata-transfer-failed",
    "metadata-http-status-rejected",
    "system-curl-unavailable",
    "system-curl-version-unsupported",
];

pub fn release(spec: &Value, fetched: Result<&[u8], &str>) -> Value {
    let source = official_endpoint(spec).ok();
    match official_endpoint(spec) {
        Err(reason) => unresolved(None, reason),
        Ok(_) => match fetched {
            Err(reason) => unresolved(source, transport_reason(reason)),
            Ok(body) if body.len() > MAX_BODY => unresolved(source, "metadata-output-too-large"),
            Ok(body) => match parse_release(spec, body) {
                Ok(parsed) => parsed.into_checked(source),
                Err(reason) => unresolved(source, reason),
            },
        },
    }
}

pub fn plan(
    catalogue: &Value,
    inventory: &Value,
    releases: &BTreeMap<String, Value>,
) -> io::Result<Value> {
    let specs = catalogue_specs(catalogue)?;
    let records = inventory_records(inventory, &specs)?;
    let mut items = Vec::with_capacity(specs.len());
    for spec in specs {
        let id = spec["id"].as_str().unwrap();
        let release = match releases.get(id) {
            Some(value) if value.is_object() => value.clone(),
            Some(_) => unresolved(official_endpoint(spec).ok(), "metadata-unparseable"),
            None => unresolved(official_endpoint(spec).ok(), "metadata-unavailable"),
        };
        items.push(plan_item(spec, records[id], release));
    }
    Ok(json!({"schema_version":1,"mode":"plan","read_only":true,"items":items}))
}

fn official_endpoint(spec: &Value) -> Result<&'static str, &'static str> {
    dependency_fetch::endpoint(spec)
}

fn unresolved(source: Option<&'static str>, reason: &'static str) -> Value {
    json!({"source":source,"state":"unresolved","version":Value::Null,"reason":reason})
}

fn transport_reason(reason: &str) -> &'static str {
    TRANSPORT_REASONS
        .iter()
        .copied()
        .find(|code| *code == reason)
        .unwrap_or("metadata-unavailable")
}

struct Parsed {
    version: String,
    rust: Option<RustMeta>,
}

struct RustMeta {
    component: Option<String>,
    date: Option<String>,
}

impl Parsed {
    fn into_checked(self, source: Option<&'static str>) -> Value {
        let mut value = json!({
            "source": source,
            "state": "checked",
            "version": self.version,
            "reason": Value::Null
        });
        if let Some(rust) = self.rust {
            value["version_kind"] = json!("rust-toolchain-cohort");
            value["component_metadata_version"] = json!(rust.component);
            value["channel_date"] = json!(rust.date);
            value["toolchain_policy"] = json!(TOOLCHAIN_POLICY);
        }
        value
    }
}

fn parse_release(spec: &Value, body: &[u8]) -> Result<Parsed, &'static str> {
    let package = spec["package"]
        .as_str()
        .ok_or("unsupported-metadata-source")?;
    let text = utf8(body)?;
    match spec["id"].as_str() {
        Some("serena" | "graphify") => parse_pypi(package, text),
        Some("codebase-memory" | "nuphus" | "python") => parse_npm(package, text),
        Some("codegraph") => crate::dependency_discovery::dependency_codegraph::parse_release(body)
            .map(|version| Parsed {
                version,
                rust: None,
            }),
        Some("rust") => parse_rust_channel(text),
        _ => Err("unsupported-metadata-source"),
    }
}

fn utf8(body: &[u8]) -> Result<&str, &'static str> {
    let text = std::str::from_utf8(body).map_err(|_| "metadata-unparseable")?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(text))
}

fn parse_pypi(package: &str, text: &str) -> Result<Parsed, &'static str> {
    let value: Value = serde_json::from_str(text).map_err(|_| "metadata-unparseable")?;
    let info = value.get("info").ok_or("metadata-unparseable")?;
    let name = bounded_str(info.get("name")).ok_or("metadata-package-mismatch")?;
    if normalize_pypi(name) != normalize_pypi(package) {
        return Err("metadata-package-mismatch");
    }
    let version = bounded_str(info.get("version")).ok_or("metadata-version-unusable")?;
    if yanked(info.get("yanked")) || !stable_version(version) {
        return Err("metadata-version-unusable");
    }
    if let Some(files) = value
        .get("releases")
        .and_then(Value::as_object)
        .and_then(|releases| releases.get(version))
        .and_then(Value::as_array)
        && !files.is_empty()
        && files.iter().all(|file| yanked(file.get("yanked")))
    {
        return Err("metadata-version-unusable");
    }
    Ok(Parsed {
        version: version.to_owned(),
        rust: None,
    })
}

fn parse_npm(package: &str, text: &str) -> Result<Parsed, &'static str> {
    let value: Value = serde_json::from_str(text).map_err(|_| "metadata-unparseable")?;
    let name = bounded_str(value.get("name")).ok_or("metadata-package-mismatch")?;
    if name != package {
        return Err("metadata-package-mismatch");
    }
    let version = bounded_str(value.get("version")).ok_or("metadata-version-unusable")?;
    if yanked(value.get("yanked")) || !stable_version(version) {
        return Err("metadata-version-unusable");
    }
    Ok(Parsed {
        version: version.to_owned(),
        rust: None,
    })
}

fn parse_rust_channel(text: &str) -> Result<Parsed, &'static str> {
    let value: toml::Value = toml::from_str(text).map_err(|_| "rust-channel-unusable")?;
    let pkg = value.get("pkg").ok_or("rust-channel-unusable")?;
    let rust = pkg
        .get("rust")
        .and_then(|table| table.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or("rust-channel-unusable")?;
    if rust.len() > 200 || rust.chars().any(char::is_control) {
        return Err("rust-channel-unusable");
    }
    let version = rust
        .split_whitespace()
        .next()
        .filter(|token| stable_version(token) && *token != "0.0.0")
        .ok_or("rust-channel-unusable")?;
    let component = pkg
        .get("rust-analyzer-preview")
        .and_then(|table| table.get("version"))
        .and_then(toml::Value::as_str)
        .and_then(bounded_component);
    let date = value
        .get("date")
        .and_then(toml::Value::as_str)
        .and_then(bounded_component);
    Ok(Parsed {
        version: version.to_owned(),
        rust: Some(RustMeta { component, date }),
    })
}

fn bounded_component(value: &str) -> Option<String> {
    (value.len() <= MAX_VERSION && !value.chars().any(char::is_control)).then(|| value.to_owned())
}

fn bounded_str(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|text| {
        !text.is_empty() && text.len() <= MAX_VERSION && !text.chars().any(char::is_control)
    })
}

fn yanked(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(true)) => true,
        Some(Value::String(text)) if !text.is_empty() => true,
        _ => false,
    }
}

fn normalize_pypi(name: &str) -> String {
    let mut normalized = String::new();
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            if !normalized.ends_with('-') {
                normalized.push('-');
            }
        } else {
            normalized.push(character.to_ascii_lowercase());
        }
    }
    normalized
}

fn stable_version(version: &str) -> bool {
    let version = version.strip_prefix('v').unwrap_or(version);
    iso_date(version) || numeric_dotted(version)
}

fn iso_date(version: &str) -> bool {
    let bytes = version.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

fn numeric_dotted(version: &str) -> bool {
    if version.is_empty() || version.len() > MAX_VERSION {
        return false;
    }
    let mut parts = version.split('.');
    parts.next().is_some_and(numeric_token) && parts.all(numeric_token)
}

fn numeric_token(part: &str) -> bool {
    !part.is_empty() && part.len() <= 10 && part.bytes().all(|byte| byte.is_ascii_digit())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VersionKind {
    Numeric,
    Date,
}

fn comparable(version: &str) -> Option<(VersionKind, Vec<u32>)> {
    if version.len() > 200 || version.chars().any(char::is_control) {
        return None;
    }
    let token = if let Some(rest) = version.strip_prefix("rust-analyzer ") {
        if let Some((token, suffix)) = rest.split_once(' ') {
            let (hash, date) = suffix
                .strip_prefix('(')?
                .strip_suffix(')')?
                .split_once(' ')?;
            if hash.is_empty()
                || hash.len() > 64
                || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !iso_date(date)
            {
                return None;
            }
            token
        } else {
            rest
        }
    } else {
        version
    };
    let token = token.strip_prefix('v').unwrap_or(token);
    if iso_date(token) {
        return parse_numbers(token, VersionKind::Date);
    }
    if numeric_dotted(token) {
        return parse_numbers(token, VersionKind::Numeric);
    }
    None
}

fn parse_numbers(token: &str, kind: VersionKind) -> Option<(VersionKind, Vec<u32>)> {
    let numbers = token
        .split(['.', '-'])
        .map(|part| part.parse::<u32>().ok())
        .collect::<Option<Vec<_>>>()?;
    (!numbers.is_empty()).then_some((kind, numbers))
}

fn newer(release: &str, installed: &str) -> Option<bool> {
    let (left_kind, mut left) = comparable(release)?;
    let (right_kind, mut right) = comparable(installed)?;
    if left_kind == VersionKind::Numeric && right_kind == VersionKind::Numeric {
        let width = left.len().max(right.len());
        left.resize(width, 0);
        right.resize(width, 0);
    }
    (left_kind == right_kind).then_some(left > right)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn identifier(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str).filter(|id| {
        !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_graphic())
    })
}

fn catalogue_specs(catalogue: &Value) -> io::Result<Vec<&Value>> {
    if !catalogue.is_object() {
        return Err(invalid("dependency plan catalogue is invalid"));
    }
    let mut specs = Vec::new();
    let mut seen = BTreeMap::new();
    for group in ["mcp", "languages"] {
        let Some(entries) = catalogue.get(group).and_then(Value::as_array) else {
            return Err(invalid("dependency plan catalogue is invalid"));
        };
        if entries.len() > MAX_SPECS {
            return Err(invalid("dependency plan catalogue is invalid"));
        }
        for spec in entries {
            if !spec.is_object() {
                return Err(invalid("dependency plan catalogue is invalid"));
            }
            let id =
                identifier(spec).ok_or_else(|| invalid("dependency plan catalogue is invalid"))?;
            if dependency_fetch::identity_endpoint(spec).is_err() {
                return Err(invalid("dependency plan records conflict"));
            }
            if seen.insert(id, group).is_some() {
                return Err(invalid("dependency plan records are duplicated"));
            }
            specs.push(spec);
        }
    }
    Ok(specs)
}

fn inventory_records<'a>(
    inventory: &'a Value,
    specs: &[&Value],
) -> io::Result<BTreeMap<&'a str, &'a Value>> {
    if !inventory.is_object() {
        return Err(invalid("dependency plan inventory is invalid"));
    }
    let mut records = BTreeMap::new();
    let mut groups = BTreeMap::new();
    for group in ["mcp", "languages"] {
        let Some(entries) = inventory.get(group).and_then(Value::as_array) else {
            return Err(invalid("dependency plan inventory is invalid"));
        };
        if entries.len() > MAX_SPECS {
            return Err(invalid("dependency plan inventory is invalid"));
        }
        for record in entries {
            if !record.is_object() {
                return Err(invalid("dependency plan inventory is invalid"));
            }
            let id = identifier(record)
                .ok_or_else(|| invalid("dependency plan inventory is invalid"))?;
            if records.insert(id, record).is_some() {
                return Err(invalid("dependency plan records are duplicated"));
            }
            groups.insert(id, group);
        }
    }
    let mut expected = BTreeMap::new();
    for spec in specs {
        let id = spec["id"].as_str().unwrap();
        let group = if matches!(
            id,
            "serena" | "graphify" | "codebase-memory" | "nuphus" | "codegraph"
        ) {
            "mcp"
        } else {
            "languages"
        };
        expected.insert(id, group);
        match records.get(id) {
            None => return Err(invalid("dependency plan records are missing")),
            Some(_) if groups.get(id) != Some(&group) => {
                return Err(invalid("dependency plan records conflict"));
            }
            Some(_) => {}
        }
    }
    if records.len() != expected.len() {
        return Err(invalid("dependency plan records conflict"));
    }
    Ok(records)
}

fn plan_item(spec: &Value, installed: &Value, release: Value) -> Value {
    let required = spec
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let status = installed
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let rust = spec["id"] == "rust";
    let checked = release["state"] == "checked";
    let release_version = bounded_str(release.get("version"));
    let installed_version = installed
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| rust || !version.starts_with("rust-analyzer "));
    let same_identity = installed_identity(installed)
        .zip(spec.get("package").and_then(Value::as_str))
        .is_some_and(|(found, expected)| found == expected);
    let (action, reason) = if !required && status == "missing" {
        ("conditional-absent", None)
    } else if status == "missing" {
        ("install-required", None)
    } else if matches!(
        status,
        "modified" | "ambiguous" | "broken" | "incomplete" | "unknown"
    ) {
        ("preserve-and-audit", Some(status))
    } else if status != "adopted" {
        ("preserve-and-audit", Some("unknown"))
    } else if !same_identity {
        ("preserve-and-audit", Some("package-identity-mismatch"))
    } else if !checked {
        (
            "metadata-unresolved",
            release.get("reason").and_then(Value::as_str),
        )
    } else {
        match (release_version, installed_version) {
            (Some(available), Some(current)) => match newer(available, current) {
                Some(true) if rust => ("reuse", Some("held-toolchain-policy")),
                Some(true) => match consumers(installed) {
                    Consumers::Idle => (
                        "stage-compatible-update",
                        Some("staging-needs-runtime-acceptance"),
                    ),
                    Consumers::Active => (
                        "update-pending-consumers",
                        Some(
                            "Shared installation has active consumers; no process will be stopped.",
                        ),
                    ),
                    Consumers::Unverified => ("preserve-and-audit", Some("consumers-unverified")),
                },
                Some(false) => ("reuse", None),
                None => ("preserve-and-audit", Some("installed-version-unusable")),
            },
            _ => ("preserve-and-audit", Some("installed-version-unusable")),
        }
    };
    json!({
        "id": spec["id"],
        "identity": spec["package"],
        "required": required,
        "installed_version": installed.get("version").cloned().unwrap_or(Value::Null),
        "installation_root": installed.get("installation_root").cloned().unwrap_or(Value::Null),
        "action": action,
        "reason": reason,
        "release": release,
        "runtime_prerequisites": spec.get("runtime").cloned().unwrap_or_else(|| json!([])),
        "rollback": ROLLBACK
    })
}

fn installed_identity(record: &Value) -> Option<&str> {
    let top = bounded_str(record.get("identity"));
    let provenance = bounded_str(record.pointer("/provenance/package_identity"));
    match (top, provenance) {
        (Some(left), Some(right)) if left == right => Some(left),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        _ => None,
    }
}

enum Consumers {
    Idle,
    Active,
    Unverified,
}

fn consumers(record: &Value) -> Consumers {
    let Some(active) = record.get("active_consumers") else {
        return Consumers::Unverified;
    };
    if matches!(active["state"].as_str(), Some("observed" | "incomplete"))
        && active["processes"]
            .as_array()
            .is_some_and(|processes| !processes.is_empty())
    {
        return Consumers::Active;
    }
    match active.get("state").and_then(Value::as_str) {
        Some("observed") => match active.get("processes").and_then(Value::as_array) {
            Some(processes) if processes.is_empty() => Consumers::Idle,
            Some(_) => Consumers::Active,
            None => Consumers::Unverified,
        },
        _ => Consumers::Unverified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str) -> Value {
        match id {
            "serena" => {
                json!({"id":"serena","package":"serena-agent","manager":"uv","metadata":"https://pypi.org/pypi/serena-agent/json","runtime":["Python >=3.11","uv"]})
            }
            "graphify" => {
                json!({"id":"graphify","package":"graphifyy","manager":"uv","metadata":"https://pypi.org/pypi/graphifyy/json","runtime":["Python >=3.11","uv"]})
            }
            "codebase-memory" => {
                json!({"id":"codebase-memory","package":"codebase-memory-mcp","manager":"npm","metadata":"https://registry.npmjs.org/codebase-memory-mcp/latest","runtime":["Node.js"]})
            }
            "nuphus" => {
                json!({"id":"nuphus","package":"@nuphus/nuphus-mcp","manager":"npm","metadata":"https://registry.npmjs.org/@nuphus%2fnuphus-mcp/latest","runtime":["Node.js"]})
            }
            "codegraph" => crate::dependency_discovery::dependency_codegraph::spec(),
            "python" => {
                json!({"id":"python","package":"basedpyright","manager":"npm","required":true,"metadata":"https://registry.npmjs.org/basedpyright/latest","runtime":["Node.js"]})
            }
            "rust" => {
                json!({"id":"rust","package":"rust-analyzer","manager":"rustup","required":true,"metadata":"https://static.rust-lang.org/dist/channel-rust-stable.toml","metadata_format":"rust-channel","runtime":["project Rust toolchain"]})
            }
            _ => panic!("unsupported fixture id"),
        }
    }

    fn record(id: &str, status: &str, version: Value, consumers: Value) -> Value {
        let identity = spec(id)["package"].clone();
        json!({
            "id": id,
            "identity": identity,
            "status": status,
            "version": version,
            "installation_root": "C:/owned/dep",
            "ownership": "adopted-shared",
            "active_consumers": consumers,
            "provenance": {"package_identity": identity}
        })
    }

    fn idle() -> Value {
        json!({"state":"observed","processes":[]})
    }

    fn catalogue(ids: &[&str]) -> Value {
        let mut mcp = Vec::new();
        let mut languages = Vec::new();
        for id in ids {
            match *id {
                "serena" | "graphify" | "codebase-memory" | "nuphus" | "codegraph" => {
                    mcp.push(spec(id))
                }
                _ => languages.push(spec(id)),
            }
        }
        json!({"mcp":mcp,"languages":languages,"retired_language_candidates":[{"id":"typescript","package":"typescript-language-server"}]})
    }

    fn inventory(records: &[Value]) -> Value {
        let mut mcp = Vec::new();
        let mut languages = Vec::new();
        for item in records {
            match item["id"].as_str() {
                Some("serena" | "graphify" | "codebase-memory" | "nuphus" | "codegraph") => {
                    mcp.push(item.clone())
                }
                _ => languages.push(item.clone()),
            }
        }
        json!({"mcp":mcp,"languages":languages})
    }

    fn pypi(name: &str, version: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"info":{"name":name,"version":version}})).unwrap()
    }

    fn npm(name: &str, version: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"name":name,"version":version})).unwrap()
    }

    fn rust_channel(cohort: &str, component: &str, date: &str) -> Vec<u8> {
        format!(
            "date = \"{date}\"\n[pkg.rust]\nversion = \"{cohort} (fixture)\"\n[pkg.rust-analyzer-preview]\nversion = \"{component}\"\n"
        )
        .into_bytes()
    }

    fn checked(id: &str, version: &str) -> Value {
        let body = match id {
            "serena" => pypi("serena-agent", version),
            "graphify" => pypi("Graphifyy", version),
            "codebase-memory" => npm("codebase-memory-mcp", version),
            "nuphus" => npm("@nuphus/nuphus-mcp", version),
            "codegraph" => serde_json::to_vec(&json!({
                "tag_name": format!("v{version}"),
                "draft": false,
                "prerelease": false,
                "assets": [{
                    "name": crate::dependency_discovery::dependency_codegraph::ARCHIVE_NAME,
                    "id": crate::dependency_discovery::dependency_codegraph::ASSET_ID,
                    "size": crate::dependency_discovery::dependency_codegraph::ARCHIVE_BYTES,
                    "state": "uploaded",
                    "digest": format!("sha256:{}", crate::dependency_discovery::dependency_codegraph::ARCHIVE_SHA256),
                    "browser_download_url": "https://github.com/colbymchenry/codegraph/releases/download/v1.6.0/codegraph-win32-x64.zip"
                }]
            })).unwrap(),
            "python" => npm("basedpyright", version),
            "rust" => rust_channel(version, "0.0.0", "2026-09-03"),
            _ => panic!("unsupported fixture id"),
        };
        release(&spec(id), Ok(&body))
    }

    fn actions(planned: &Value) -> Vec<(&str, &str, Option<&str>)> {
        planned["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                (
                    item["id"].as_str().unwrap(),
                    item["action"].as_str().unwrap(),
                    item["reason"].as_str(),
                )
            })
            .collect()
    }

    #[test]
    fn release_rejects_malformed_mismatched_and_untrusted_versions_without_echo() {
        let serena = spec("serena");
        let bad = release(&serena, Ok(b"not-json { token=secret-body"));
        assert_eq!(bad["state"], "unresolved");
        assert_eq!(bad["reason"], "metadata-unparseable");
        assert!(bad["version"].is_null());
        let text = bad.to_string();
        assert!(!text.contains("secret-body"));
        assert!(!text.contains("token="));

        let mismatched = release(&serena, Ok(&pypi("other-agent", "9.9.9")));
        assert_eq!(mismatched["reason"], "metadata-package-mismatch");
        assert!(!mismatched.to_string().contains("other-agent"));
        assert!(!mismatched.to_string().contains("9.9.9"));

        let mut yanked = serde_json::from_slice::<Value>(&pypi("serena-agent", "1.7.0")).unwrap();
        yanked["info"]["yanked"] = json!(true);
        let yanked = release(&serena, Ok(&serde_json::to_vec(&yanked).unwrap()));
        assert_eq!(yanked["reason"], "metadata-version-unusable");

        let prerelease = release(&spec("python"), Ok(&npm("basedpyright", "1.29.0-rc.1")));
        assert_eq!(prerelease["reason"], "metadata-version-unusable");
        assert!(prerelease["version"].is_null());

        let npm_mismatch = release(&spec("python"), Ok(&npm("pyright", "1.29.0")));
        assert_eq!(npm_mismatch["reason"], "metadata-package-mismatch");
        assert!(npm_mismatch["version"].is_null());
        assert_eq!(npm_mismatch.as_object().unwrap().len(), 4);
        assert_eq!(npm_mismatch["source"], spec("python")["metadata"]);

        let mut poisoned = spec("python");
        poisoned["metadata"] =
            json!("https://user:PRIVATE_CREDENTIAL@registry.npmjs.org/basedpyright/latest");
        let leaked = release(&poisoned, Ok(&npm("basedpyright", "1.29.0")));
        assert_eq!(leaked["reason"], "unsupported-metadata-source");
        assert!(leaked["source"].is_null());
        assert!(leaked["version"].is_null());
        assert!(!leaked.to_string().contains("PRIVATE_CREDENTIAL"));
        assert!(!leaked.to_string().contains("user:"));

        let echoed = release(&serena, Err("OSError: access_token=fixture-secret"));
        assert_eq!(echoed["reason"], "metadata-unavailable");
        assert_eq!(echoed["source"], "https://pypi.org/pypi/serena-agent/json");
        assert!(!echoed.to_string().contains("fixture-secret"));
        assert!(!echoed.to_string().contains("OSError"));

        let transport = release(&serena, Err("metadata-http-error"));
        assert_eq!(transport["reason"], "metadata-http-error");
        assert_eq!(transport["state"], "unresolved");
    }

    #[test]
    fn version_comparators_accept_numeric_and_iso_and_reject_prerelease() {
        assert_eq!(newer("1.10.0", "1.9.0"), Some(true));
        assert_eq!(newer("1.2.0", "1.2.0"), Some(false));
        assert_eq!(newer("1.2.0", "1.2"), Some(false));
        assert_eq!(newer("1.2", "1.2.0"), Some(false));
        assert_eq!(newer("1.1.0", "1.2.0"), Some(false));
        assert_eq!(newer("2026-09-03", "2026-02-08"), Some(true));
        assert_eq!(newer("1.2.0", "2026-02-08"), None);
        assert_eq!(
            newer("1.98.1", "rust-analyzer 1.97.1 (8bab26f4 2026-07-14)"),
            Some(true)
        );
        assert_eq!(
            newer("1.97.1", "rust-analyzer 1.97.1 (8bab26f4 2026-07-14)"),
            Some(false)
        );
        assert_eq!(newer("1.2.0-rc.1", "1.1.0"), None);
        assert_eq!(newer("5.12.0-1.26426.8", "5.11.0"), None);
        assert!(stable_version("5.11.0"));
        assert!(stable_version("2026-02-08"));
        assert!(stable_version("v1.2.3"));
        assert!(!stable_version("2.0.0-preview.1"));
        assert!(!stable_version("5.12.0-1.26426.8"));
        for bad in [
            "1.2.3 PRIVATE",
            " 1.2.3",
            "1.2.3 ",
            "rust-analyzer 1.2.3 PRIVATE",
            "rust-analyzer 1.2.3 (not-a-hash 2026-07-14)",
        ] {
            assert_eq!(newer("1.3.0", bad), None, "{bad}");
        }
    }

    #[test]
    fn plan_preserves_modified_ambiguous_and_incomplete_installations() {
        let cat = catalogue(&["nuphus", "python", "codebase-memory"]);
        let inv = inventory(&[
            record("nuphus", "modified", json!("0.2.1"), idle()),
            record("python", "ambiguous", json!("1.28.0"), idle()),
            record("codebase-memory", "incomplete", json!("0.10.7"), idle()),
        ]);
        let mut releases = BTreeMap::new();
        releases.insert("nuphus".into(), checked("nuphus", "0.2.2"));
        releases.insert("python".into(), checked("python", "1.29.0"));
        releases.insert(
            "codebase-memory".into(),
            checked("codebase-memory", "0.10.8"),
        );
        let planned = plan(&cat, &inv, &releases).unwrap();
        assert_eq!(
            actions(&planned),
            vec![
                ("nuphus", "preserve-and-audit", Some("modified")),
                ("codebase-memory", "preserve-and-audit", Some("incomplete")),
                ("python", "preserve-and-audit", Some("ambiguous")),
            ]
        );
        assert_eq!(planned["mode"], "plan");
        assert_eq!(planned["read_only"], true);
    }

    #[test]
    fn plan_active_consumers_and_missing_metadata_do_not_authorize_replacement() {
        let busy = record(
            "codebase-memory",
            "adopted",
            json!("0.10.7"),
            json!({"state":"observed","processes":[{"pid":4242}]}),
        );
        let unverified = record(
            "python",
            "adopted",
            json!("1.28.0"),
            json!({"state":"incomplete","processes":[]}),
        );
        let planned = plan(
            &catalogue(&["codebase-memory", "python"]),
            &inventory(&[busy.clone(), unverified]),
            &BTreeMap::from([(
                "codebase-memory".into(),
                checked("codebase-memory", "0.10.8"),
            )]),
        )
        .unwrap();
        assert_eq!(planned["items"][0]["action"], "update-pending-consumers");
        assert_eq!(planned["items"][1]["action"], "metadata-unresolved");
        assert_eq!(planned["items"][1]["reason"], "metadata-unavailable");
        assert_eq!(planned["items"][1]["release"]["state"], "unresolved");
        let text = planned.to_string();
        assert!(!text.contains("4242"));
        assert!(!text.contains("pid"));

        let mut unchecked = busy;
        unchecked["active_consumers"] = json!({"state":"not-checked","processes":[]});
        let planned = plan(
            &catalogue(&["codebase-memory"]),
            &inventory(&[unchecked]),
            &BTreeMap::from([(
                "codebase-memory".into(),
                checked("codebase-memory", "0.10.8"),
            )]),
        )
        .unwrap();
        assert_eq!(planned["items"][0]["action"], "preserve-and-audit");
        assert_eq!(planned["items"][0]["reason"], "consumers-unverified");

        let observed = record(
            "codebase-memory",
            "adopted",
            json!("0.10.7"),
            json!({"state":"incomplete","processes":[{"pid":4242}]}),
        );
        let planned = plan(
            &catalogue(&["codebase-memory"]),
            &inventory(&[observed]),
            &BTreeMap::from([(
                "codebase-memory".into(),
                checked("codebase-memory", "0.10.8"),
            )]),
        )
        .unwrap();
        assert_eq!(planned["items"][0]["action"], "update-pending-consumers");
    }

    #[test]
    fn rust_cohort_compares_analyzer_line_and_never_proposes_project_update() {
        let checked_release = release(
            &spec("rust"),
            Ok(&rust_channel("1.98.1", "0.0.0", "2026-09-03")),
        );
        assert_eq!(checked_release["state"], "checked");
        assert_eq!(checked_release["version"], "1.98.1");
        assert_eq!(checked_release["version_kind"], "rust-toolchain-cohort");
        assert_eq!(checked_release["component_metadata_version"], "0.0.0");
        assert_eq!(checked_release["channel_date"], "2026-09-03");
        assert_eq!(checked_release["toolchain_policy"], TOOLCHAIN_POLICY);
        assert_ne!(checked_release["version"], "0.0.0");

        let placeholder = release(
            &spec("rust"),
            Ok(&rust_channel("0.0.0", "0.0.0", "2026-09-03")),
        );
        assert_eq!(placeholder["state"], "unresolved");
        assert_eq!(placeholder["reason"], "rust-channel-unusable");

        let installed = record(
            "rust",
            "adopted",
            json!("rust-analyzer 1.97.1 (8bab26f4 2026-07-14)"),
            idle(),
        );
        let planned = plan(
            &catalogue(&["rust"]),
            &inventory(&[installed]),
            &BTreeMap::from([("rust".into(), checked_release)]),
        )
        .unwrap();
        assert_eq!(planned["items"][0]["action"], "reuse");
        assert_eq!(planned["items"][0]["reason"], "held-toolchain-policy");
        assert_ne!(planned["items"][0]["action"], "stage-compatible-update");
    }

    #[test]
    fn missing_required_is_install_required_with_unresolved_metadata_visible() {
        let planned = plan(
            &catalogue(&["serena", "python"]),
            &inventory(&[
                record("serena", "missing", Value::Null, idle()),
                record("python", "missing", Value::Null, idle()),
            ]),
            &BTreeMap::from([("serena".into(), checked("serena", "1.7.0"))]),
        )
        .unwrap();
        assert_eq!(planned["items"][0]["action"], "install-required");
        assert_eq!(planned["items"][0]["release"]["state"], "checked");
        assert_eq!(planned["items"][1]["action"], "install-required");
        assert_eq!(planned["items"][1]["release"]["state"], "unresolved");
        assert_eq!(
            planned["items"][1]["release"]["reason"],
            "metadata-unavailable"
        );
    }

    #[test]
    fn adopted_equal_or_newer_is_reused_and_idle_newer_is_only_staged() {
        let equal = plan(
            &catalogue(&["python"]),
            &inventory(&[record("python", "adopted", json!("1.29.0"), idle())]),
            &BTreeMap::from([("python".into(), checked("python", "1.29.0"))]),
        )
        .unwrap();
        assert_eq!(equal["items"][0]["action"], "reuse");
        assert!(equal["items"][0]["reason"].is_null());

        let newer_installed = plan(
            &catalogue(&["python"]),
            &inventory(&[record("python", "adopted", json!("1.30.0"), idle())]),
            &BTreeMap::from([("python".into(), checked("python", "1.29.0"))]),
        )
        .unwrap();
        assert_eq!(newer_installed["items"][0]["action"], "reuse");

        let staged = plan(
            &catalogue(&["python"]),
            &inventory(&[record("python", "adopted", json!("1.28.0"), idle())]),
            &BTreeMap::from([("python".into(), checked("python", "1.29.0"))]),
        )
        .unwrap();
        assert_eq!(staged["items"][0]["action"], "stage-compatible-update");
        assert_eq!(
            staged["items"][0]["reason"],
            "staging-needs-runtime-acceptance"
        );
    }

    #[test]
    fn python_pyright_identity_is_not_compared_across_packages() {
        let mut installed = record("python", "adopted", json!("1.1.400"), idle());
        installed["identity"] = json!("pyright");
        installed["provenance"]["package_identity"] = json!("pyright");
        let planned = plan(
            &catalogue(&["python"]),
            &inventory(&[installed]),
            &BTreeMap::from([("python".into(), checked("python", "1.29.0"))]),
        )
        .unwrap();
        assert_eq!(planned["items"][0]["action"], "preserve-and-audit");
        assert_eq!(planned["items"][0]["reason"], "package-identity-mismatch");
        assert_eq!(planned["items"][0]["identity"], "basedpyright");
    }

    #[test]
    fn plan_rejects_duplicate_missing_and_conflicting_records_and_skips_retired() {
        let mut cat = catalogue(&["python"]);
        cat["languages"]
            .as_array_mut()
            .unwrap()
            .push(spec("python"));
        assert_eq!(
            plan(
                &cat,
                &inventory(&[record("python", "missing", Value::Null, idle())]),
                &BTreeMap::new()
            )
            .unwrap_err()
            .to_string(),
            "dependency plan records are duplicated"
        );

        assert_eq!(
            plan(
                &catalogue(&["python", "rust"]),
                &inventory(&[record("python", "missing", Value::Null, idle())]),
                &BTreeMap::new()
            )
            .unwrap_err()
            .to_string(),
            "dependency plan records are missing"
        );

        let extra = inventory(&[
            record("python", "missing", Value::Null, idle()),
            record("rust", "missing", Value::Null, idle()),
        ]);
        assert_eq!(
            plan(&catalogue(&["python"]), &extra, &BTreeMap::new())
                .unwrap_err()
                .to_string(),
            "dependency plan records conflict"
        );

        let mut foreign = catalogue(&["python"]);
        foreign["mcp"].as_array_mut().unwrap().push(json!({
            "id":"other",
            "package":"other",
            "manager":"npm",
            "metadata":"https://example.invalid"
        }));
        assert!(
            plan(
                &foreign,
                &inventory(&[record("python", "missing", Value::Null, idle())]),
                &BTreeMap::new()
            )
            .is_err()
        );

        let retired_only = json!({
            "mcp":[],
            "languages":[],
            "retired_language_candidates":[spec("python")]
        });
        let planned = plan(
            &retired_only,
            &json!({"mcp":[],"languages":[]}),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(planned["items"].as_array().unwrap().len(), 0);
        assert_eq!(planned["mode"], "plan");
        assert_eq!(planned["read_only"], true);
    }

    #[test]
    fn pypi_normalized_name_and_npm_exact_name_are_required() {
        let graphify = release(&spec("graphify"), Ok(&pypi("Graphifyy", "0.9.55")));
        assert_eq!(graphify["state"], "checked");
        assert_eq!(graphify["version"], "0.9.55");
        assert_eq!(graphify["source"], "https://pypi.org/pypi/graphifyy/json");
        let scoped = release(&spec("nuphus"), Ok(&npm("@nuphus/nuphus-mcp", "0.2.2")));
        assert_eq!(scoped["state"], "checked");
        let wrong_scope = release(&spec("nuphus"), Ok(&npm("nuphus-mcp", "0.2.2")));
        assert_eq!(wrong_scope["reason"], "metadata-package-mismatch");
    }

    #[test]
    fn current_catalogue_plan_reports_six_read_only_items() {
        let ids = [
            "serena",
            "codebase-memory",
            "graphify",
            "nuphus",
            "python",
            "rust",
        ];
        let cat = catalogue(&ids);
        let inv = inventory(&[
            record("serena", "adopted", json!("1.7.0"), idle()),
            record("codebase-memory", "adopted", json!("0.10.8"), idle()),
            record("graphify", "adopted", json!("0.9.55"), idle()),
            record("nuphus", "modified", json!("0.2.2"), idle()),
            record("python", "adopted", json!("1.29.0"), idle()),
            record(
                "rust",
                "adopted",
                json!("rust-analyzer 1.97.1 (8bab26f4 2026-07-14)"),
                idle(),
            ),
        ]);
        let mut releases = BTreeMap::new();
        releases.insert("serena".into(), checked("serena", "1.7.0"));
        releases.insert(
            "codebase-memory".into(),
            checked("codebase-memory", "0.10.8"),
        );
        releases.insert("graphify".into(), checked("graphify", "0.9.55"));
        releases.insert("nuphus".into(), checked("nuphus", "0.2.2"));
        releases.insert("python".into(), checked("python", "1.29.0"));
        releases.insert("rust".into(), checked("rust", "1.98.1"));
        let planned = plan(&cat, &inv, &releases).unwrap();
        assert_eq!(planned["mode"], "plan");
        assert_eq!(planned["read_only"], true);
        assert_eq!(planned["items"].as_array().unwrap().len(), 6);
        assert_eq!(
            planned["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(planned["items"][3]["action"], "preserve-and-audit");
        assert_eq!(planned["items"][5]["action"], "reuse");
        assert_eq!(planned["items"][5]["reason"], "held-toolchain-policy");
    }
}
