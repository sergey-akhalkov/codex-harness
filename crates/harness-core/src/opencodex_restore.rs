//! Bounded skipHistory-equivalent native restore for pinned OpenCodex 2.44.0.
//!
//! Public `ocx restore` writes durable desired-state and can restore history.
//! The current kit uses `restoreNativeCodex({ skipHistory: true })`, which
//! restores config, profile and catalog ownership without history or
//! desired-state mutation. This module implements that limited contract in Rust.

use crate::build_identity;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const MARKER: &str = "# Auto-injected by opencodex";
const REALTIME_KEY: &str = "experimental_realtime_ws_base_url";
const SUBAGENT_MARKER: &str = "# Managed by opencodex: native subagent default";
const AGENTS_TABLE_MARKER: &str = "# Managed by opencodex: native subagent defaults table";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreResult {
    pub success: bool,
    pub message: String,
    pub config_changed: bool,
    pub history_skipped: bool,
    pub desired_state_written: bool,
}

#[derive(Deserialize)]
struct Journal {
    version: u32,
    #[serde(rename = "originalConfig")]
    original_config: String,
    #[serde(rename = "originalProfile")]
    original_profile: Option<String>,
    #[serde(rename = "injectedConfigHash")]
    injected_config_hash: Option<String>,
    #[serde(rename = "injectedProfileHash")]
    injected_profile_hash: Option<Value>,
    #[serde(rename = "injectedOpenaiBaseUrl")]
    injected_openai_base_url: Option<String>,
    #[serde(rename = "injectedRealtimeWsBaseUrl")]
    injected_realtime_ws_base_url: Option<String>,
    #[serde(rename = "injectedCatalogPath")]
    injected_catalog_path: Option<String>,
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let staging = path.with_extension("harness-tmp");
    {
        let mut file = fs::File::create(&staging)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(staging, path)
}

fn sha256_text(text: &str) -> String {
    build_identity::hash_bytes(text.as_bytes())
}

fn decode_snapshot(value: &str) -> io::Result<String> {
    let bytes = STANDARD
        .decode(value.trim())
        .map_err(|_| invalid("OpenCodex journal snapshot is not valid base64."))?;
    String::from_utf8(bytes).map_err(|_| invalid("OpenCodex journal snapshot is not UTF-8."))
}

fn first_table(lines: &[&str]) -> usize {
    lines
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .unwrap_or(lines.len())
}

fn parse_toml_string(raw: &str) -> String {
    if raw.starts_with('"') {
        serde_json::from_str(raw).unwrap_or_else(|_| {
            raw.get(1..raw.len().saturating_sub(1))
                .unwrap_or_default()
                .to_string()
        })
    } else if raw.len() >= 2 {
        raw[1..raw.len() - 1].to_string()
    } else {
        String::new()
    }
}

fn quoted_value(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let bytes = rest.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    match bytes[0] {
        34 => {
            let mut escaped = false;
            for (index, ch) in rest[1..].char_indices() {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '"' {
                    return Some(parse_toml_string(&rest[..=index + 1]));
                }
            }
            None
        }
        39 => rest[1..]
            .bytes()
            .position(|byte| byte == 39)
            .map(|end| parse_toml_string(&rest[..=end + 1])),
        _ => None,
    }
}

fn root_string(content: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = content.split('\n').collect();
    let root_end = first_table(&lines);
    lines[..root_end]
        .iter()
        .find_map(|line| quoted_value(line, key))
}

fn is_root_key_line(line: &str, key: &str) -> bool {
    quoted_value(line, key).is_some()
}

fn has_injected_openai_base_url(content: &str) -> bool {
    let lines: Vec<&str> = content.split('\n').collect();
    let root_end = first_table(&lines);
    (1..root_end).any(|index| {
        is_root_key_line(lines[index], "openai_base_url") && lines[index - 1].contains(MARKER)
    })
}

fn is_ocx_provider_header(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "[model_providers.opencodex]" || trimmed.starts_with("[model_providers.opencodex.")
}

fn has_ocx_provider_table(content: &str) -> bool {
    content
        .split('\n')
        .any(|line| is_ocx_provider_header(line.trim()))
}

fn has_opencodex_routing(content: &str) -> bool {
    has_ocx_provider_table(content)
        || content.lines().any(|line| {
            line.trim().starts_with("model_provider")
                && quoted_value(line, "model_provider").as_deref() == Some("opencodex")
        })
        || has_injected_openai_base_url(content)
}

fn keep_lines(lines: &[&str], drop: &[bool]) -> String {
    lines
        .iter()
        .enumerate()
        .filter(|(index, _)| !drop[*index])
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_injected_openai_base_url(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let root_end = first_table(&lines);
    let mut drop = vec![false; lines.len()];
    for index in 0..root_end {
        if !lines[index].contains(MARKER) {
            continue;
        }
        if index + 1 < root_end
            && (is_root_key_line(lines[index + 1], "openai_base_url")
                || is_root_key_line(lines[index + 1], REALTIME_KEY))
        {
            drop[index] = true;
            drop[index + 1] = true;
        } else if index + 1 >= root_end || lines[index + 1].trim().is_empty() {
            drop[index] = true;
        }
    }
    keep_lines(&lines, &drop)
}

fn strip_journaled_openai_base_url(
    content: &str,
    injected_url: Option<&str>,
    injected_realtime: Option<&str>,
) -> String {
    if injected_url.is_none() && injected_realtime.is_none() {
        return content.to_string();
    }
    let lines: Vec<&str> = content.split('\n').collect();
    let root_end = first_table(&lines);
    let mut drop = vec![false; lines.len()];
    for index in 0..root_end {
        let line = lines[index];
        let matches = if is_root_key_line(line, "openai_base_url") {
            injected_url
                .is_some_and(|url| quoted_value(line, "openai_base_url").as_deref() == Some(url))
        } else if is_root_key_line(line, REALTIME_KEY) {
            injected_realtime
                .is_some_and(|url| quoted_value(line, REALTIME_KEY).as_deref() == Some(url))
        } else {
            false
        };
        if !matches {
            continue;
        }
        drop[index] = true;
        if index > 0 && lines[index - 1].contains(MARKER) {
            drop[index - 1] = true;
        }
    }
    keep_lines(&lines, &drop)
}

fn collapse_blank_lines(content: &str) -> String {
    let mut out = String::new();
    let mut blank = 0usize;
    for line in content.split('\n') {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 2 {
                continue;
            }
        } else {
            blank = 0;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    format!("{}\n", out.trim_end())
}

fn remove_marked_section(content: &str, start: impl Fn(&str) -> bool) -> String {
    let mut filtered = Vec::new();
    let mut in_section = false;
    for line in content.split('\n') {
        if start(line) {
            in_section = true;
            continue;
        }
        if in_section {
            if line.trim_start().starts_with('[') && !start(line) {
                in_section = false;
                filtered.push(line);
            }
            continue;
        }
        filtered.push(line);
    }
    collapse_blank_lines(&filtered.join("\n"))
}

fn remove_ocx_section(content: &str) -> String {
    remove_marked_section(content, |line| {
        line.contains(MARKER) || is_ocx_provider_header(line.trim())
    })
}

fn remove_profile_section(content: &str) -> String {
    remove_marked_section(content, |line| line.trim() == "[profiles.opencodex]")
}

fn strip_root_routed_model(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let root_end = first_table(&lines);
    lines
        .into_iter()
        .enumerate()
        .filter(|(index, line)| {
            *index >= root_end
                || quoted_value(line, "model").is_none_or(|model| !model.contains('/'))
        })
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_opencodex_catalog_path(path: &str) -> bool {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .is_some_and(|name| name == "opencodex-catalog.json")
}

fn strip_opencodex_catalog_path(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let root_end = first_table(&lines);
    lines
        .into_iter()
        .enumerate()
        .filter(|(index, line)| {
            *index >= root_end
                || quoted_value(line, "model_catalog_json")
                    .is_none_or(|path| !is_opencodex_catalog_path(&path))
        })
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_managed_subagent_defaults(content: &str) -> Result<String, String> {
    if !content.contains(SUBAGENT_MARKER) && !content.contains(AGENTS_TABLE_MARKER) {
        return Ok(content.to_string());
    }
    let lines: Vec<&str> = content.split('\n').collect();
    let mut drop = vec![false; lines.len()];
    let mut in_owned_agents = false;
    for (index, line) in lines.iter().enumerate() {
        if line.contains(SUBAGENT_MARKER) {
            drop[index] = true;
            if index + 1 < lines.len() {
                drop[index + 1] = true;
            }
        }
        if line.contains(AGENTS_TABLE_MARKER) {
            drop[index] = true;
            in_owned_agents = true;
            continue;
        }
        if in_owned_agents {
            if line.trim_start().starts_with('[') && !line.contains(AGENTS_TABLE_MARKER) {
                in_owned_agents = false;
            } else {
                drop[index] = true;
            }
        }
    }
    Ok(keep_lines(&lines, &drop))
}

fn strip_opencodex_config(
    content: &str,
    journaled_base_url: Option<&str>,
    journaled_realtime: Option<&str>,
) -> Result<String, String> {
    let had_root_ocx = root_string(content, "model_provider").as_deref() == Some("opencodex");
    let had_injected = has_injected_openai_base_url(content)
        || journaled_base_url
            .is_some_and(|url| root_string(content, "openai_base_url").as_deref() == Some(url));
    let mut out = strip_injected_openai_base_url(content);
    out = strip_journaled_openai_base_url(&out, journaled_base_url, journaled_realtime);
    if has_ocx_provider_table(&out) {
        out = remove_ocx_section(&out);
    }
    out = remove_profile_section(&out);
    out = out
        .split('\n')
        .filter(|line| quoted_value(line, "model_provider").as_deref() != Some("opencodex"))
        .collect::<Vec<_>>()
        .join("\n");
    if had_root_ocx || had_injected {
        out = strip_root_routed_model(&out);
    }
    out = strip_managed_subagent_defaults(&out)?;
    out = strip_opencodex_catalog_path(&out);
    Ok(collapse_blank_lines(&out))
}

fn restore_catalog(codex_home: &Path, injected_catalog_path: Option<&str>) -> io::Result<()> {
    let catalog_path = injected_catalog_path
        .map(PathBuf::from)
        .unwrap_or_else(|| codex_home.join("opencodex-catalog.json"));
    if !catalog_path.is_file() {
        return Ok(());
    }
    let catalog: Value = serde_json::from_slice(&fs::read(&catalog_path)?)
        .map_err(|_| invalid("OpenCodex catalog is not valid JSON."))?;
    let Some(models) = catalog.get("models").and_then(Value::as_array) else {
        return Ok(());
    };
    let native: Vec<Value> = models
        .iter()
        .filter(|model| {
            model
                .get("slug")
                .and_then(Value::as_str)
                .is_none_or(|slug| !slug.contains('/'))
        })
        .cloned()
        .collect();
    if native.len() == models.len() {
        return Ok(());
    }
    let mut restored = catalog;
    restored["models"] = Value::Array(native);
    atomic_write(
        &catalog_path,
        format!("{}\n", serde_json::to_string_pretty(&restored)?).as_bytes(),
    )
}

fn restore_journal(codex_home: &Path) -> io::Result<(bool, bool, Option<Journal>)> {
    let path = codex_home.join("opencodex-journal.json");
    if !path.is_file() {
        return Ok((false, false, None));
    }
    let parsed: Result<Journal, _> = serde_json::from_slice(&fs::read(&path)?);
    let journal = match parsed {
        Ok(journal) if journal.version == 1 => journal,
        _ => {
            let _ = fs::remove_file(&path);
            return Ok((false, false, None));
        }
    };
    let config_path = codex_home.join("config.toml");
    let profile_path = codex_home.join("opencodex.config.toml");
    let current_config = if config_path.is_file() {
        fs::read_to_string(&config_path)?
    } else {
        String::new()
    };
    let current_profile = profile_path
        .is_file()
        .then(|| fs::read_to_string(&profile_path))
        .transpose()?;
    let config_unchanged = journal
        .injected_config_hash
        .as_deref()
        .is_none_or(|hash| sha256_text(&current_config) == hash);
    let profile_hash = match &journal.injected_profile_hash {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(value)) => Some(Some(value.as_str())),
        Some(_) => {
            let _ = fs::remove_file(&path);
            return Ok((false, false, None));
        }
    };
    let profile_unchanged = match profile_hash {
        None => true,
        Some(expected) => current_profile.as_deref().map(sha256_text).as_deref() == expected,
    };
    let mut config_restored = false;
    let mut profile_restored = false;
    if config_unchanged {
        atomic_write(
            &config_path,
            decode_snapshot(&journal.original_config)?.as_bytes(),
        )?;
        config_restored = true;
    }
    if profile_unchanged {
        if let Some(original) = &journal.original_profile {
            atomic_write(&profile_path, decode_snapshot(original)?.as_bytes())?;
            profile_restored = true;
        } else if profile_path.exists() {
            fs::remove_file(&profile_path)?;
            profile_restored = true;
        } else {
            profile_restored = true;
        }
    }
    if config_restored && profile_restored {
        let _ = fs::remove_file(&path);
    }
    Ok((config_restored, profile_restored, Some(journal)))
}

pub fn restore_skip_history(codex_home: &Path) -> io::Result<RestoreResult> {
    let (config_restored, profile_restored, journal) = restore_journal(codex_home)?;
    let config_path = codex_home.join("config.toml");
    let profile_path = codex_home.join("opencodex.config.toml");
    let mut config_changed = config_restored || profile_restored;
    if !config_restored {
        if !config_path.is_file() {
            if profile_path.exists() && !profile_restored {
                fs::remove_file(&profile_path)?;
                config_changed = true;
            }
        } else {
            let raw = fs::read_to_string(&config_path)?;
            let journaled_base = journal
                .as_ref()
                .and_then(|value| value.injected_openai_base_url.as_deref());
            let journaled_realtime = journal
                .as_ref()
                .and_then(|value| value.injected_realtime_ws_base_url.as_deref());
            let had = has_opencodex_routing(&raw)
                || journaled_base.is_some_and(|url| {
                    root_string(&raw, "openai_base_url").as_deref() == Some(url)
                })
                || journaled_realtime
                    .is_some_and(|url| root_string(&raw, REALTIME_KEY).as_deref() == Some(url));
            let stripped = strip_opencodex_config(&raw, journaled_base, journaled_realtime)
                .map_err(io::Error::other)?;
            if had || stripped != raw {
                atomic_write(&config_path, stripped.as_bytes())?;
                config_changed = true;
            }
            if profile_path.exists() && !profile_restored {
                fs::remove_file(&profile_path)?;
                config_changed = true;
            }
        }
    }
    restore_catalog(
        codex_home,
        journal
            .as_ref()
            .and_then(|value| value.injected_catalog_path.as_deref()),
    )?;
    Ok(RestoreResult {
        success: true,
        message: if config_changed {
            "Removed opencodex routing from Codex config + profile.".to_string()
        } else {
            "opencodex not present in Codex config.".to_string()
        },
        config_changed,
        history_skipped: true,
        desired_state_written: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn skip_history_restore_strips_injected_routing_without_desired_state() {
        let root = tempfile::Builder::new()
            .prefix("harness-ocx-skip-history-")
            .tempdir()
            .unwrap();
        let home = root.path();
        fs::write(
            home.join("config.toml"),
            concat!(
                "# Auto-injected by opencodex\n",
                "openai_base_url = \"http://127.0.0.1:10100/v1\"\n",
                "model = \"xai/grok-4.6\"\n",
                "model_catalog_json = \"opencodex-catalog.json\"\n",
                "[agents]\n",
                "keep = true\n"
            ),
        )
        .unwrap();
        fs::write(home.join("opencodex.config.toml"), "generated = true\n").unwrap();
        fs::write(
            home.join("opencodex-catalog.json"),
            serde_json::to_vec_pretty(&json!({
                "models": [
                    {"slug": "gpt-6-astra"},
                    {"slug": "xai/grok-4.6"}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let ocx = home.join("opencodex");
        fs::create_dir(&ocx).unwrap();
        fs::write(
            ocx.join("config.json"),
            "{\"clientIntegrations\":{\"codex\":true}}",
        )
        .unwrap();
        let result = restore_skip_history(home).unwrap();
        assert!(result.success);
        assert!(result.history_skipped);
        assert!(!result.desired_state_written);
        let config = fs::read_to_string(home.join("config.toml")).unwrap();
        assert!(!config.contains("openai_base_url"));
        assert!(!config.contains("xai/grok-4.6"));
        assert!(config.contains("keep = true"));
        assert!(!home.join("opencodex.config.toml").exists());
        assert_eq!(
            fs::read_to_string(ocx.join("config.json")).unwrap(),
            "{\"clientIntegrations\":{\"codex\":true}}"
        );
        let catalog: Value =
            serde_json::from_slice(&fs::read(home.join("opencodex-catalog.json")).unwrap())
                .unwrap();
        assert_eq!(catalog["models"].as_array().unwrap().len(), 1);
        assert_eq!(catalog["models"][0]["slug"], "gpt-6-astra");
    }

    #[test]
    fn skip_history_restore_replays_an_unchanged_journal_snapshot() {
        let root = tempfile::Builder::new()
            .prefix("harness-ocx-journal-")
            .tempdir()
            .unwrap();
        let home = root.path();
        let original = "model = \"gpt-6-astra\"\n";
        let injected = format!(
            "{MARKER}\nopenai_base_url = \"http://127.0.0.1:10100/v1\"\nmodel = \"xai/grok-4.6\"\n"
        );
        fs::write(home.join("config.toml"), &injected).unwrap();
        fs::write(
            home.join("opencodex-journal.json"),
            serde_json::to_vec(&json!({
                "version": 1,
                "originalConfig": STANDARD.encode(original.as_bytes()),
                "originalProfile": Value::Null,
                "injectedConfigHash": sha256_text(&injected),
                "injectedProfileHash": Value::Null,
                "injectedOpenaiBaseUrl": "http://127.0.0.1:10100/v1",
                "pid": 1,
                "timestamp": "2026-09-14T00:00:00.000Z"
            }))
            .unwrap(),
        )
        .unwrap();
        let result = restore_skip_history(home).unwrap();
        assert!(result.success);
        assert_eq!(
            fs::read_to_string(home.join("config.toml")).unwrap(),
            original
        );
        assert!(!home.join("opencodex-journal.json").exists());
        assert!(!result.desired_state_written);
    }
}
