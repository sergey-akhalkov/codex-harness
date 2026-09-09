//! Skill treatment edits and an independent before/after discovery comparison.
use serde_json::Value;
use std::{io, path::Path};
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

pub(super) const CANDIDATES: [&str; 2] = ["project-verification", "reproduce-regression"];
const LIMIT: usize = 4 * 1024 * 1024;

fn invalid() -> io::Error {
    io::Error::other("invalid or conflicting outcome skill configuration")
}

fn spelling(path: &str) -> io::Result<Vec<u16>> {
    if path.len() > 32 * 1024 || path.contains('\0') || !Path::new(path).is_absolute() {
        return Err(invalid());
    }
    let absolute = std::path::absolute(path)?;
    let text = absolute.to_str().ok_or_else(invalid)?;
    // Verbatim disk spelling is returned by canonicalize/discovery. It does
    // not create a different native override from the ordinary drive spelling.
    let text = text.strip_prefix("\\\\?\\").unwrap_or(text);
    Ok(text.encode_utf16().collect())
}

fn same_path(left: &str, right: &str) -> io::Result<bool> {
    let left = spelling(left)?;
    let right = spelling(right)?;
    match unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        )
    } {
        0 => Err(io::Error::last_os_error()),
        value => Ok(value == CSTR_EQUAL),
    }
}

pub(super) fn candidate_skills(skills: &[Value]) -> io::Result<Vec<&Value>> {
    let mut selected: Vec<&Value> = Vec::new();
    for skill in skills {
        if !CANDIDATES.contains(&skill["name"].as_str().unwrap_or("")) {
            continue;
        }
        let path = skill["path"].as_str().ok_or_else(invalid)?;
        spelling(path)?;
        for previous in &selected {
            if same_path(path, previous["path"].as_str().ok_or_else(invalid)?)? {
                return Err(invalid());
            }
        }
        selected.push(skill);
    }
    if !CANDIDATES
        .iter()
        .all(|name| selected.iter().any(|s| s["name"] == *name))
    {
        return Err(invalid());
    }
    Ok(selected)
}

pub(super) fn prepare(bytes: &[u8], skills: &[Value], enabled: bool) -> io::Result<Vec<u8>> {
    if bytes.len() > LIMIT {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let parsed: toml::Value =
        toml::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| invalid())?;
    let entries = parsed.get("skills").and_then(|s| s.get("config"));
    let mut existing: Vec<(&str, bool)> = Vec::new();
    if let Some(entries) = entries {
        for entry in entries.as_array().ok_or_else(invalid)? {
            let path = entry
                .get("path")
                .and_then(toml::Value::as_str)
                .ok_or_else(invalid)?;
            let state = entry
                .get("enabled")
                .and_then(toml::Value::as_bool)
                .ok_or_else(invalid)?;
            spelling(path)?;
            for (other, _) in &existing {
                if same_path(path, other)? {
                    return Err(invalid());
                }
            }
            existing.push((path, state));
        }
    }
    let mut result = text.to_owned();
    for skill in candidate_skills(skills)? {
        let path = skill["path"].as_str().ok_or_else(invalid)?;
        let mut found = false;
        for (prior_path, prior_state) in &existing {
            if same_path(path, prior_path)? {
                if *prior_state != enabled {
                    return Err(invalid());
                }
                found = true;
            }
        }
        if !found {
            result.push_str(&format!(
                "\n[[skills.config]]\npath = {}\nenabled = {enabled}\n",
                serde_json::to_string(path)?
            ));
        }
    }
    if result.len() > LIMIT {
        return Err(invalid());
    }
    let _: toml::Value =
        toml::from_str(result.trim_start_matches('\u{feff}')).map_err(|_| invalid())?;
    Ok(result.into_bytes())
}

pub(super) fn verify_after(before: &[Value], after: &[Value], enabled: bool) -> io::Result<()> {
    candidate_skills(before)?;
    let selected = candidate_skills(after)?;
    if selected
        .iter()
        .any(|s| s["enabled"].as_bool() != Some(enabled))
    {
        return Err(invalid());
    }
    let mut expected: Vec<_> = before
        .iter()
        .cloned()
        .map(|mut skill| {
            if CANDIDATES.contains(&skill["name"].as_str().unwrap_or("")) {
                skill["enabled"] = Value::Bool(enabled);
            }
            skill.to_string()
        })
        .collect();
    let mut observed: Vec<_> = after.iter().map(Value::to_string).collect();
    expected.sort();
    observed.sort();
    if expected != observed {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn verify_configuration(before: &Value, after: &Value) -> io::Result<()> {
    fn unaffected(value: &Value) -> io::Result<Value> {
        let mut value = value.clone();
        let root = value.as_object_mut().ok_or_else(invalid)?;
        if let Some(skills) = root.get_mut("skills").and_then(Value::as_object_mut) {
            skills.remove("config");
            if skills.is_empty() {
                root.remove("skills");
            }
        }
        // Native optional skills are null when no overrides are configured.
        if root.get("skills").is_some_and(Value::is_null) {
            root.remove("skills");
        }
        Ok(value)
    }
    if unaffected(before)? != unaffected(after)? {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn skills() -> Vec<Value> {
        vec![
            json!({"name":CANDIDATES[0],"path":"C:\\one\\SKILL.md","enabled":true}),
            json!({"name":CANDIDATES[0],"path":"C:\\two\\SKILL.md","enabled":true}),
            json!({"name":CANDIDATES[1],"path":"C:\\three\\SKILL.md","enabled":true}),
            json!({"name":"personal","path":"C:\\personal\\SKILL.md","enabled":false}),
        ]
    }

    #[test]
    fn every_registration_is_disabled_and_unrelated_text_is_preserved() {
        let source = b"# retained\r\nmodel = 'gpt-6-astra'\r\n[[skills.config]]\r\npath = 'C:\\personal\\SKILL.md'\r\nenabled = false\r\n";
        let changed = prepare(source, &skills(), false).unwrap();
        assert!(changed.starts_with(source));
        let parsed: toml::Value = toml::from_str(std::str::from_utf8(&changed).unwrap()).unwrap();
        let entries = parsed["skills"]["config"].as_array().unwrap();
        assert_eq!(entries.len(), 4);
        assert!(
            entries
                .iter()
                .all(|s| s["enabled"].as_bool() == Some(false))
        );
        assert_eq!(prepare(&changed, &skills(), false).unwrap(), changed);
        assert!(prepare(&changed, &skills(), true).is_err());
    }

    #[test]
    fn native_path_spelling_conflicts_duplicates_and_invalid_shapes_fail() {
        let config = b"[[skills.config]]\npath='c:/ONE/SKILL.md'\nenabled=false\n";
        assert!(prepare(config, &skills(), true).is_err());
        let mut rows = skills();
        rows[0]["path"] = json!("\\\\?\\C:\\one\\SKILL.md");
        assert!(prepare(config, &rows, true).is_err());
        rows.push(rows[0].clone());
        assert!(prepare(b"", &rows, false).is_err());
        for invalid in [
            "[skills]\nconfig = 1",
            "[[skills.config]]\npath='relative'\nenabled=true",
            "[skills]\nconfig=[{path='C:\\one\\SKILL.md',enabled=1}]",
            "model = ",
        ] {
            assert!(prepare(invalid.as_bytes(), &skills(), false).is_err());
        }
        assert!(prepare(b"", &skills()[..2], false).is_err());
    }

    #[test]
    fn after_discovery_checks_all_candidates_and_all_unrelated_metadata() {
        let before = skills();
        let mut after = before.clone();
        for row in &mut after[..3] {
            row["enabled"] = json!(false);
        }
        after.reverse();
        verify_after(&before, &after, false).unwrap();
        for index in 0..after.len() {
            let mut bad = after.clone();
            bad[index]["enabled"] = json!(true);
            assert!(verify_after(&before, &bad, false).is_err());
        }
        let mut bad = after.clone();
        bad[0]["description"] = json!("foreign mutation");
        assert!(verify_after(&before, &bad, false).is_err());
        assert!(verify_after(&before, &after[..3], false).is_err());
    }

    #[test]
    fn skill_overrides_do_not_allow_an_unrelated_native_configuration_change() {
        let before = json!({"model":"gpt-6-astra","model_provider":"openai","skills":null});
        let after = json!({"model":"gpt-6-astra","model_provider":"openai","skills":{"config":[{"path":"C:/one/SKILL.md","enabled":false}]}});
        verify_configuration(&before, &after).unwrap();
        for (key, value) in [
            ("model_provider", json!("foreign")),
            ("model", json!("different")),
            ("approval_policy", json!("never")),
        ] {
            let mut changed = after.clone();
            changed[key] = value;
            assert!(verify_configuration(&before, &changed).is_err());
        }
        let mut changed = after;
        changed["skills"]["other_setting"] = json!(true);
        assert!(verify_configuration(&before, &changed).is_err());
    }
}
