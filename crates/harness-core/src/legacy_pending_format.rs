//! Bounded parser for the old PowerShell `pending.json` journal.
//!
//! Unknown fields are ignored. Exact `stateBeforeBytes` are restored when
//! present. When those bytes are missing or null, a nested `previousState`
//! object is serialized with `serde_json::to_vec_pretty` for semantic restore.
//! `verify_state` then accepts that exact fallback as a repeat-recovery hash,
//! in addition to `stateAfterHash` and the hash of the exact prior bytes or
//! empty. When `stateAfterHash` is absent, only missing current metadata,
//! exact prior bytes, or JSON equal to recorded planned/previous is accepted.
//! Unrelated current metadata is preserved. Decoded restore bytes are not
//! reparsed as schema-1 metadata; the old reader wrote them unchecked.
#![cfg(windows)]

use crate::{
    build_identity::hash_bytes,
    installation_state::{self, PathScope},
    inventory::ordinary_parents,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fmt, io,
    path::{Path, PathBuf},
};

const MAX_PAYLOAD: usize = 1024 * 1024;
const MAX_OPERATIONS: usize = 4096;
const MAX_LINKS: usize = 4096;
const MAX_PATH_UNITS: usize = 32767;

pub(crate) struct Pending {
    pub operations: Vec<Operation>,
    pub path_scope: PathScope,
    pub path_before: Option<String>,
    pub path_after: Option<String>,
    pub restore_state: Option<Vec<u8>>,
    pub state_after_hash: Option<String>,
    hash_before: Vec<u8>,
    accept_fallback_restore_hash: bool,
    planned_state: Value,
    previous_state: Option<Value>,
}

pub(crate) struct Operation {
    pub destination: PathBuf,
    pub old_source: Option<PathBuf>,
    pub new_source: Option<PathBuf>,
    pub old_directory: bool,
}

struct DestRecord {
    directory: bool,
    sources: HashSet<String>,
}

impl fmt::Debug for Pending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pending")
            .field("operations", &self.operations.len())
            .field("path_scope", &self.path_scope)
            .field("has_path_before", &self.path_before.is_some())
            .field("has_path_after", &self.path_after.is_some())
            .field("has_restore_state", &self.restore_state.is_some())
            .field("has_state_after_hash", &self.state_after_hash.is_some())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Operation")
            .field("has_old_source", &self.old_source.is_some())
            .field("has_new_source", &self.new_source.is_some())
            .field("old_directory", &self.old_directory)
            .finish_non_exhaustive()
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "legacy pending journal is invalid or belongs to another host; preserving it",
    )
}

fn concurrent() -> io::Error {
    io::Error::other(
        "legacy installation state changed after interruption; preserving concurrent changes",
    )
}

fn normalize(path: &Path) -> io::Result<PathBuf> {
    installation_state::normal(path).map_err(|_| invalid())
}

fn path_key(path: &Path) -> io::Result<String> {
    installation_state::key(path).map_err(|_| invalid())
}

fn object(value: &Value) -> io::Result<&serde_json::Map<String, Value>> {
    value.as_object().ok_or_else(invalid)
}

fn required<'a>(object: &'a serde_json::Map<String, Value>, name: &str) -> io::Result<&'a Value> {
    object.get(name).ok_or_else(invalid)
}

fn entries(value: &Value, max: usize) -> io::Result<Vec<&Value>> {
    match value {
        Value::Array(items) => {
            if items.len() > max {
                return Err(invalid());
            }
            Ok(items.iter().collect())
        }
        Value::Object(_) => Ok(vec![value]),
        _ => Err(invalid()),
    }
}

fn decode_base64(input: &str) -> io::Result<Vec<u8>> {
    let mut filtered = Vec::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b' ' | b'\t' | b'\r' | b'\n' => {}
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=' => filtered.push(*byte),
            _ => return Err(invalid()),
        }
    }
    if filtered.is_empty() {
        return Ok(Vec::new());
    }
    if filtered.len() % 4 != 0 {
        return Err(invalid());
    }
    let pad = filtered
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    if pad > 2 || filtered[..filtered.len() - pad].contains(&b'=') {
        return Err(invalid());
    }
    let mut output = Vec::with_capacity(filtered.len() / 4 * 3);
    for chunk in filtered.chunks_exact(4) {
        let padding = usize::from(chunk[2] == b'=') + usize::from(chunk[3] == b'=');
        let a = six(chunk[0])?;
        let b = six(chunk[1])?;
        let c = if padding == 2 { 0 } else { six(chunk[2])? };
        let d = if padding == 0 { six(chunk[3])? } else { 0 };
        output.push((a << 2) | (b >> 4));
        if padding < 2 {
            output.push((b << 4) | (c >> 2));
        }
        if padding == 0 {
            output.push((c << 6) | d);
        }
        if output.len() > MAX_PAYLOAD {
            return Err(invalid());
        }
    }
    Ok(output)
}

fn six(byte: u8) -> io::Result<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(invalid()),
    }
}

fn parse_hash(value: &Value) -> io::Result<String> {
    let text = value.as_str().ok_or_else(invalid)?;
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    Ok(text.to_ascii_lowercase())
}

fn path_text(value: &Value) -> io::Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => {
            if text.contains('\0') || text.encode_utf16().count() > MAX_PATH_UNITS {
                return Err(invalid());
            }
            Ok(Some(text.clone()))
        }
        _ => Err(invalid()),
    }
}

fn path_scope(value: &Value) -> io::Result<PathScope> {
    value
        .as_str()
        .and_then(PathScope::parse)
        .ok_or_else(invalid)
}

fn json_path(value: &Value) -> io::Result<PathBuf> {
    let text = value.as_str().ok_or_else(invalid)?;
    if text.contains('\0') {
        return Err(invalid());
    }
    normalize(Path::new(text))
}

fn optional_source(value: Option<&Value>) -> io::Result<Option<PathBuf>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(json_path(value)?)),
    }
}

fn destination_is_directory(dest: &Path, codex: &Path, user: &Path) -> io::Result<bool> {
    let dest_key = path_key(dest)?;
    if dest_key == path_key(&codex.join("agents/codex-harness"))? {
        return Ok(true);
    }
    let skills = path_key(&user.join(".agents/skills"))?;
    if dest.file_name().is_some()
        && let Some(parent) = dest.parent()
        && path_key(parent)? == skills
    {
        return Ok(true);
    }
    for relative in [
        "AGENTS.md",
        "harness.config.toml",
        "harness/bin/codex.ps1",
        "harness/bin/codex-harness-check.ps1",
        "hooks.json",
        "harness/bin/hook.ps1",
    ] {
        if dest_key == path_key(&codex.join(relative))? {
            return Ok(false);
        }
    }
    Err(invalid())
}

fn kind_is_directory(kind: Option<&str>) -> Option<bool> {
    match kind {
        Some("skill" | "agents") => Some(true),
        Some(
            "instructions"
            | "profile"
            | "launcher"
            | "diagnostic-launcher"
            | "hooks"
            | "hook-launcher",
        ) => Some(false),
        Some(_) | None => None,
    }
}

fn record_link(
    records: &mut HashMap<String, DestRecord>,
    dest: &Path,
    source: &Path,
    directory: bool,
) -> io::Result<()> {
    let dest_key = path_key(dest)?;
    let source_key = path_key(source)?;
    match records.get_mut(&dest_key) {
        Some(existing) => {
            if existing.directory != directory {
                return Err(invalid());
            }
            existing.sources.insert(source_key);
        }
        None => {
            let mut sources = HashSet::new();
            sources.insert(source_key);
            records.insert(dest_key, DestRecord { directory, sources });
        }
    }
    Ok(())
}

fn schema_version_one(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_u64() == Some(1) || number.as_i64() == Some(1),
        _ => false,
    }
}

fn validate_state(
    state: &Value,
    codex: &Path,
    user: &Path,
    dependency: &Path,
    records: &mut HashMap<String, DestRecord>,
) -> io::Result<()> {
    let state = object(state)?;
    if !schema_version_one(required(state, "schemaVersion")?) {
        return Err(invalid());
    }
    if path_key(&json_path(required(state, "codexHome")?)?)? != path_key(codex)?
        || path_key(&json_path(required(state, "userHome")?)?)? != path_key(user)?
    {
        return Err(invalid());
    }
    if let Some(value) = state.get("dependencyUserHome")
        && path_key(&json_path(value)?)? != path_key(dependency)?
    {
        return Err(invalid());
    }
    for link in entries(required(state, "links")?, MAX_LINKS)? {
        let link = object(link)?;
        let destination = json_path(required(link, "destination")?)?;
        let source = json_path(required(link, "source")?)?;
        let dest_directory = destination_is_directory(&destination, codex, user)?;
        if let Some(kind_directory) = kind_is_directory(link.get("kind").and_then(Value::as_str))
            && kind_directory != dest_directory
        {
            return Err(invalid());
        }
        ordinary_parents(&destination)?;
        record_link(records, &destination, &source, dest_directory)?;
    }
    Ok(())
}

impl Pending {
    pub(crate) fn parse(
        bytes: &[u8],
        codex_home: &Path,
        user_home: &Path,
        dependency_user_home: &Path,
    ) -> io::Result<Self> {
        if bytes.len() > MAX_PAYLOAD {
            return Err(invalid());
        }
        let root: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        let top = object(&root)?;
        let codex = normalize(codex_home)?;
        let user = normalize(user_home)?;
        let dependency = normalize(dependency_user_home)?;
        let path_scope = path_scope(required(top, "pathScope")?)?;
        let path_before = path_text(required(top, "pathBefore")?)?;
        let path_after = path_text(required(top, "pathAfter")?)?;
        let mut records = HashMap::new();
        validate_state(
            required(top, "plannedState")?,
            &codex,
            &user,
            &dependency,
            &mut records,
        )?;
        let previous = match top.get("previousState") {
            None | Some(Value::Null) => None,
            Some(value) => {
                validate_state(value, &codex, &user, &dependency, &mut records)?;
                Some(value)
            }
        };
        let planned_state = required(top, "plannedState")?.clone();
        let previous_state = previous.cloned();
        let mut operations = Vec::new();
        let mut seen = HashSet::new();
        for operation in entries(required(top, "operations")?, MAX_OPERATIONS)? {
            let operation = object(operation)?;
            let destination = json_path(required(operation, "destination")?)?;
            let dest_key = path_key(&destination)?;
            if !seen.insert(dest_key.clone()) {
                return Err(invalid());
            }
            let record = records.get(&dest_key).ok_or_else(invalid)?;
            let old_source = optional_source(operation.get("oldSource"))?;
            let new_source = optional_source(operation.get("newSource"))?;
            for source in old_source.iter().chain(new_source.iter()) {
                if !record.sources.contains(&path_key(source)?) {
                    return Err(invalid());
                }
            }
            let old_directory = old_source.is_some() && record.directory;
            ordinary_parents(&destination)?;
            operations.push(Operation {
                destination,
                old_source,
                new_source,
                old_directory,
            });
        }
        let (restore_state, hash_before, accept_fallback_restore_hash) =
            match top.get("stateBeforeBytes") {
                Some(Value::String(text)) => {
                    let bytes = decode_base64(text)?;
                    if bytes.len() > MAX_PAYLOAD {
                        return Err(invalid());
                    }
                    (Some(bytes.clone()), bytes, false)
                }
                None | Some(Value::Null) => {
                    if let Some(previous) = previous {
                        let bytes = serde_json::to_vec_pretty(previous).map_err(|_| invalid())?;
                        if bytes.len() > MAX_PAYLOAD {
                            return Err(invalid());
                        }
                        (Some(bytes), Vec::new(), true)
                    } else {
                        (None, Vec::new(), false)
                    }
                }
                Some(_) => return Err(invalid()),
            };
        let state_after_hash = match top.get("stateAfterHash") {
            None => None,
            Some(value) => Some(parse_hash(value)?),
        };
        Ok(Self {
            operations,
            path_scope,
            path_before,
            path_after,
            restore_state,
            state_after_hash,
            hash_before,
            accept_fallback_restore_hash,
            planned_state,
            previous_state,
        })
    }

    pub(crate) fn verify_state(&self, current: Option<&[u8]>) -> io::Result<()> {
        if current.map(|bytes| bytes.len()).unwrap_or(0) > MAX_PAYLOAD {
            return Err(concurrent());
        }
        let Some(expected) = &self.state_after_hash else {
            return self.verify_hashless(current);
        };
        let current = current.unwrap_or(&[]);
        let got = hash_bytes(current);
        let mut allowed = vec![expected.clone(), hash_bytes(&self.hash_before)];
        if self.accept_fallback_restore_hash
            && let Some(restore) = &self.restore_state
        {
            allowed.push(hash_bytes(restore));
        }
        if allowed.iter().any(|hash| hash == &got) {
            Ok(())
        } else {
            Err(concurrent())
        }
    }

    fn verify_hashless(&self, current: Option<&[u8]>) -> io::Result<()> {
        let Some(current) = current else {
            return Ok(());
        };
        if self.restore_state.as_deref() == Some(current) {
            return Ok(());
        }
        if let Ok(value) = serde_json::from_slice::<Value>(current)
            && (value == self.planned_state || self.previous_state.as_ref() == Some(&value))
        {
            return Ok(());
        }
        Err(concurrent())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    struct Homes {
        _guard: tempfile::TempDir,
        codex: PathBuf,
        user: PathBuf,
        dep: PathBuf,
        source: PathBuf,
    }

    impl Homes {
        fn new() -> Self {
            let guard = tempfile::TempDir::new().unwrap();
            let codex = guard.path().join("codex");
            let user = guard.path().join("user");
            let dep = guard.path().join("dep");
            let source = guard.path().join("source");
            fs::create_dir_all(codex.join("harness/bin")).unwrap();
            fs::create_dir_all(codex.join("agents")).unwrap();
            fs::create_dir_all(user.join(".agents/skills")).unwrap();
            fs::create_dir_all(&dep).unwrap();
            fs::create_dir_all(&source).unwrap();
            Self {
                _guard: guard,
                codex,
                user,
                dep,
                source,
            }
        }

        fn parse(&self, value: &Value) -> io::Result<Pending> {
            Pending::parse(
                &serde_json::to_vec(value).unwrap(),
                &self.codex,
                &self.user,
                &self.dep,
            )
        }

        fn agents(&self) -> PathBuf {
            self.codex.join("AGENTS.md")
        }

        fn skill(&self) -> PathBuf {
            self.user.join(".agents/skills/folder-name")
        }

        fn agents_dir(&self) -> PathBuf {
            self.codex.join("agents/codex-harness")
        }

        fn link(&self, kind: &str, dest: PathBuf, name: &str) -> Value {
            json!({
                "kind": kind,
                "name": name,
                "destination": dest,
                "source": self.source.join("item"),
                "owned": true,
                "unknownLinkField": 1,
            })
        }

        fn state(&self, links: Vec<Value>) -> Value {
            json!({
                "schemaVersion": 1,
                "codexHome": self.codex,
                "userHome": self.user,
                "dependencyUserHome": self.dep,
                "links": links,
                "profileName": "other",
                "unknownStateField": {"keep": true},
            })
        }

        fn journal(&self, previous: Value, planned: Value, operations: Vec<Value>) -> Value {
            json!({
                "previousState": previous,
                "plannedState": planned,
                "operations": operations,
                "pathScope": "Process",
                "pathBefore": "C:\\before",
                "pathAfter": "C:\\after",
                "unknownTop": [1, 2],
            })
        }
    }

    fn encode_base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut output = String::new();
        for chunk in bytes.chunks(3) {
            let n = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
                output.push(if index > chunk.len() {
                    '='
                } else {
                    ALPHABET[((n >> shift) & 63) as usize] as char
                });
            }
        }
        output
    }

    #[test]
    fn current_writer_bytes_round_trip() {
        let homes = Homes::new();
        let previous = homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]);
        let planned = homes.state(vec![
            homes.link("instructions", homes.agents(), "AGENTS"),
            homes.link("skill", homes.skill(), "a-name-independent-of-folder"),
            homes.link("agents", homes.agents_dir(), "codex-harness"),
        ]);
        let before = b"exact-prior-bytes";
        let after = serde_json::to_vec_pretty(&planned).unwrap();
        let mut journal = homes.journal(
            previous,
            planned,
            vec![
                json!({"destination": homes.agents(), "oldSource": homes.source.join("item"), "newSource": homes.source.join("item")}),
                json!({"destination": homes.skill(), "oldSource": homes.source.join("item"), "newSource": Value::Null}),
                json!({"destination": homes.agents_dir(), "oldSource": Value::Null, "newSource": homes.source.join("item")}),
            ],
        );
        journal["stateBeforeBytes"] = json!(encode_base64(before));
        journal["stateAfterHash"] = json!(hash_bytes(&after).to_ascii_uppercase());
        let pending = homes.parse(&journal).unwrap();
        assert_eq!(pending.operations.len(), 3);
        assert!(!pending.operations[0].old_directory);
        assert!(pending.operations[1].old_directory);
        assert!(!pending.operations[2].old_directory);
        assert_eq!(pending.restore_state.as_deref(), Some(before.as_slice()));
        pending.verify_state(Some(&after)).unwrap();
        pending.verify_state(Some(before)).unwrap();
        assert!(pending.verify_state(Some(b"foreign")).is_err());
        let rendered = format!("{:?}", pending);
        assert!(!rendered.contains("AGENTS.md"));
        assert!(!rendered.contains("exact-prior-bytes"));
        assert!(!rendered.contains(&homes.codex.to_string_lossy().into_owned()));
    }

    #[test]
    fn historical_hashless_and_unknown_fields() {
        let homes = Homes::new();
        let mut previous = homes.state(vec![homes.link("skill", homes.skill(), "other-name")]);
        previous["custom"] = json!({"keep": true});
        previous
            .as_object_mut()
            .unwrap()
            .remove("dependencyUserHome");
        let planned = previous.clone();
        let journal = homes.journal(
            previous.clone(),
            planned,
            vec![json!({"destination": homes.skill(), "oldSource": homes.source.join("item")})],
        );
        let pending = homes.parse(&journal).unwrap();
        let restored: Value =
            serde_json::from_slice(pending.restore_state.as_ref().unwrap()).unwrap();
        assert_eq!(restored["custom"], json!({"keep": true}));
        assert_eq!(restored["unknownStateField"], json!({"keep": true}));
        pending.verify_state(None).unwrap();
        pending
            .verify_state(Some(serde_json::to_vec(&previous).unwrap().as_slice()))
            .unwrap();
        pending
            .verify_state(Some(
                serde_json::to_vec_pretty(&previous).unwrap().as_slice(),
            ))
            .unwrap();
        assert!(pending.verify_state(Some(b"anything-historical")).is_err());
        assert!(pending.verify_state(Some(b"{\"unrelated\":true}")).is_err());
        assert!(pending.verify_state(Some(b"")).is_err());
        assert!(pending.operations[0].old_directory);
    }

    #[test]
    fn fallback_previous_retry_hash_is_exact_restore_only() {
        let homes = Homes::new();
        let previous = homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]);
        let planned = previous.clone();
        let after = serde_json::to_vec(&planned).unwrap();
        let mut journal = homes.journal(
            previous,
            planned,
            vec![json!({"destination": homes.agents(), "newSource": homes.source.join("item")})],
        );
        journal["stateBeforeBytes"] = Value::Null;
        journal["stateAfterHash"] = json!(hash_bytes(&after));
        let pending = homes.parse(&journal).unwrap();
        let restore = pending.restore_state.clone().unwrap();
        assert_ne!(restore, after);
        pending.verify_state(Some(&after)).unwrap();
        pending.verify_state(Some(&restore)).unwrap();
        pending.verify_state(None).unwrap();
        assert!(pending.verify_state(Some(b"not-the-fallback")).is_err());
    }

    #[test]
    fn fresh_null_previous_uses_empty_hash_before() {
        let homes = Homes::new();
        let planned = homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]);
        let after = serde_json::to_vec(&planned).unwrap();
        let mut journal = homes.journal(
            Value::Null,
            planned,
            vec![json!({"destination": homes.agents(), "newSource": homes.source.join("item")})],
        );
        journal["stateAfterHash"] = json!(hash_bytes(&after));
        let pending = homes.parse(&journal).unwrap();
        assert!(pending.restore_state.is_none());
        pending.verify_state(Some(&after)).unwrap();
        pending.verify_state(None).unwrap();
        assert!(pending.verify_state(Some(b"nope")).is_err());
    }

    #[test]
    fn hashless_empty_current_requires_recorded_empty_restore() {
        let homes = Homes::new();
        let planned = homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]);
        let mut journal = homes.journal(
            Value::Null,
            planned,
            vec![json!({"destination": homes.agents(), "newSource": homes.source.join("item")})],
        );
        journal["stateBeforeBytes"] = json!("");
        let pending = homes.parse(&journal).unwrap();
        assert_eq!(pending.restore_state.as_deref(), Some(b"".as_slice()));
        pending.verify_state(Some(b"")).unwrap();
        pending.verify_state(None).unwrap();
    }

    #[test]
    fn owner_dest_source_and_scope_are_rejected() {
        let homes = Homes::new();
        let planned = homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]);
        let mut journal = homes.journal(
            Value::Null,
            planned.clone(),
            vec![json!({"destination": homes.agents(), "newSource": homes.source.join("item")})],
        );
        assert!(
            Pending::parse(
                &serde_json::to_vec(&journal).unwrap(),
                &homes.codex,
                &homes.dep,
                &homes.dep,
            )
            .is_err()
        );
        journal["pathScope"] = json!("Machine");
        assert!(homes.parse(&journal).is_err());
        journal["pathScope"] = json!("Process");
        journal["operations"] = json!([{
            "destination": homes.codex.join("foreign.md"),
            "newSource": homes.source.join("item"),
        }]);
        assert!(homes.parse(&journal).is_err());
        journal["operations"] = json!([{
            "destination": homes.agents(),
            "newSource": homes.codex.join("escape"),
        }]);
        assert!(homes.parse(&journal).is_err());
        let mut parent_escape = planned.clone();
        parent_escape["links"] = json!([{
            "destination": homes.codex.join("AGENTS.md").join("..").join("Windows"),
            "source": homes.source.join("item"),
        }]);
        journal["plannedState"] = parent_escape;
        journal["operations"] = json!([]);
        assert!(homes.parse(&journal).is_err());
    }

    #[test]
    fn powershell_scope_spelling_is_read_without_changing_raw_history() {
        let homes = Homes::new();
        let planned = homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]);
        let mut journal = homes.journal(
            Value::Null,
            planned,
            vec![json!({"destination": homes.agents(), "newSource": homes.source.join("item")})],
        );
        for (spelling, expected) in [
            ("uSeR", PathScope::User),
            ("pRoCeSs", PathScope::Process),
            ("USER", PathScope::User),
        ] {
            journal["pathScope"] = json!(spelling);
            assert_eq!(homes.parse(&journal).unwrap().path_scope, expected);
            let scope: PathScope = serde_json::from_value(json!(spelling)).unwrap();
            assert_eq!(scope, expected);
            assert_eq!(
                serde_json::to_value(scope).unwrap(),
                json!(if expected == PathScope::User {
                    "User"
                } else {
                    "Process"
                })
            );
        }
        for invalid in [
            json!("Machine"),
            json!(" User"),
            json!("Process "),
            json!(null),
            json!(1),
            json!("uſer"),
        ] {
            journal["pathScope"] = invalid.clone();
            assert!(homes.parse(&journal).is_err());
            assert!(serde_json::from_value::<PathScope>(invalid).is_err());
        }
    }

    #[test]
    fn duplicate_operations_are_ambiguous() {
        let homes = Homes::new();
        let planned = homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]);
        let op = json!({"destination": homes.agents(), "newSource": homes.source.join("item")});
        let journal = homes.journal(Value::Null, planned, vec![op.clone(), op]);
        assert!(homes.parse(&journal).is_err());
    }

    #[test]
    fn kind_conflict_and_destination_derived_directory() {
        let homes = Homes::new();
        let planned = homes.state(vec![homes.link("skill", homes.agents(), "AGENTS")]);
        let journal = homes.journal(Value::Null, planned, vec![]);
        assert!(homes.parse(&journal).is_err());
        let mut planned = homes.state(vec![json!({
            "destination": homes.skill(),
            "source": homes.source.join("item"),
        })]);
        planned["links"] =
            json!({"destination": homes.skill(), "source": homes.source.join("item")});
        let mut journal = homes.journal(Value::Null, planned, vec![]);
        journal["operations"] =
            json!({"destination": homes.skill(), "oldSource": homes.source.join("item")});
        let pending = homes.parse(&journal).unwrap();
        assert_eq!(pending.operations.len(), 1);
        assert!(pending.operations[0].old_directory);
    }

    #[test]
    fn missing_path_before_and_bounds() {
        let homes = Homes::new();
        let planned = homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]);
        let mut journal = homes.journal(Value::Null, planned, vec![]);
        journal.as_object_mut().unwrap().remove("pathBefore");
        assert!(homes.parse(&journal).is_err());
        assert!(
            Pending::parse(
                &[b'x'; MAX_PAYLOAD + 1],
                &homes.codex,
                &homes.user,
                &homes.dep
            )
            .is_err()
        );
        let too_long = "a".repeat(MAX_PATH_UNITS + 1);
        journal = homes.journal(
            Value::Null,
            homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]),
            vec![],
        );
        journal["pathBefore"] = json!(too_long);
        assert!(homes.parse(&journal).is_err());
        journal["pathBefore"] = json!("ok\0no");
        assert!(homes.parse(&journal).is_err());
        let ops = vec![json!({"destination": "C:\\x"}); MAX_OPERATIONS + 1];
        journal["pathBefore"] = json!("C:\\before");
        journal["operations"] = Value::Array(ops);
        assert!(homes.parse(&journal).is_err());
    }

    #[test]
    fn base64_vectors_malformed_and_dotnet_pad_bits() {
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("   \t\r\n").unwrap(), b"");
        assert_eq!(decode_base64("Zg==").unwrap(), b"f");
        assert_eq!(decode_base64("Zm8=").unwrap(), b"fo");
        assert_eq!(decode_base64("Zm9v").unwrap(), b"foo");
        assert_eq!(decode_base64("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(decode_base64("Zm9vYmE=").unwrap(), b"fooba");
        assert_eq!(decode_base64("Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(decode_base64(" Zm\n9v\r\n").unwrap(), b"foo");
        assert_eq!(decode_base64("Zh==").unwrap(), b"f");
        assert_eq!(decode_base64("Zm9=").unwrap(), b"fo");
        assert_eq!(decode_base64("AAB=").unwrap(), [0, 0]);
        assert!(decode_base64("Zm8").is_err());
        assert!(decode_base64("Zg=").is_err());
        assert!(decode_base64("Zg===").is_err());
        assert!(decode_base64("Zm9v=").is_err());
        assert!(decode_base64("-_==").is_err());
        assert!(decode_base64("====").is_err());
        let homes = Homes::new();
        let planned = homes.state(vec![homes.link("instructions", homes.agents(), "AGENTS")]);
        let mut journal = homes.journal(Value::Null, planned, vec![]);
        journal["stateBeforeBytes"] = json!("bm90LWpzb24=");
        let pending = homes.parse(&journal).unwrap();
        assert_eq!(
            pending.restore_state.as_deref(),
            Some(b"not-json".as_slice())
        );
        journal["stateBeforeBytes"] = json!("Zg=");
        assert!(homes.parse(&journal).is_err());
    }
}
