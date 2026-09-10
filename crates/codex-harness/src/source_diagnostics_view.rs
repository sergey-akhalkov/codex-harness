//! Redacted interpretation of native diagnostics observations.
//!
//! Winners are reconstructed from native-parsed layers. This module does not
//! claim loaded-process freshness.
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, io, path::Path};

const SAFE_KEYS: [&str; 8] = [
    "model",
    "model_reasoning_effort",
    "approval_policy",
    "sandbox_mode",
    "developer_instructions",
    "features.hooks",
    "features.multi_agent",
    "features.memories",
];

fn invalid() -> io::Error {
    io::Error::other("malformed native diagnostics observation")
}

fn object(value: &Value) -> io::Result<&Map<String, Value>> {
    value.as_object().ok_or_else(invalid)
}

fn array(value: &Value) -> io::Result<&[Value]> {
    value.as_array().map(Vec::as_slice).ok_or_else(invalid)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn trim_slash(mut text: String) -> String {
    while text.ends_with('\\') {
        text.pop();
    }
    text
}

fn windows_full_path(path: &Path) -> io::Result<String> {
    // Windows absolute-path resolution normalizes dot segments before testing
    // ancestry, matching the original GetFullPath observation contract.
    let absolute = std::path::absolute(path).map_err(|_| invalid())?;
    let text = absolute.to_str().ok_or_else(invalid)?;
    Ok(trim_slash(
        text.strip_prefix("\\\\?\\")
            .unwrap_or(text)
            .replace('/', "\\"),
    ))
}

fn project_under_trust(project: &Path, configured: &str) -> io::Result<bool> {
    if configured.is_empty() {
        return Ok(false);
    }
    let project = windows_full_path(project)?;
    let trust = windows_full_path(Path::new(configured))?;
    if project.is_empty() || trust.is_empty() {
        return Ok(false);
    }
    let project: Vec<_> = project.encode_utf16().collect();
    let trust: Vec<_> = trust.encode_utf16().collect();
    if project.len() < trust.len()
        || project.len() > trust.len() && project[trust.len()] != u16::from(b'\\')
    {
        return Ok(false);
    }
    let length = i32::try_from(trust.len()).map_err(|_| invalid())?;
    let comparison = unsafe {
        windows_sys::Win32::Globalization::CompareStringOrdinal(
            project.as_ptr(),
            length,
            trust.as_ptr(),
            length,
            1,
        )
    };
    if comparison == 0 {
        return Err(invalid());
    }
    Ok(comparison == windows_sys::Win32::Globalization::CSTR_EQUAL)
}

fn profile_changes_context(project: &Path, profile: &Map<String, Value>) -> io::Result<bool> {
    if ["project_root_markers", "credential_broker", "skills"]
        .into_iter()
        .any(|key| profile.contains_key(key))
    {
        return Ok(true);
    }
    let Some(projects) = profile.get("projects") else {
        return Ok(false);
    };
    for path in object(projects)?.keys() {
        if project_under_trust(project, path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn diagnostic_value<'a>(config: &'a Value, key: &str) -> Option<&'a Value> {
    let mut current = config;
    for part in key.split('.') {
        current = current.as_object()?.get(part)?;
    }
    (!current.is_null()).then_some(current)
}

fn allowed_model(value: &str) -> bool {
    if value == "gpt-6-astra" {
        return true;
    }
    let Some(rest) = value.strip_prefix("xai/grok-4.") else {
        return false;
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return false;
    }
    match &rest[digits..] {
        "" => true,
        suffix => suffix.strip_prefix('-').is_some_and(|tail| {
            !tail.is_empty()
                && tail
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        }),
    }
}

fn allowed_preference(key: &str, value: &str) -> bool {
    match key {
        "model" => allowed_model(value),
        "model_reasoning_effort" => matches!(
            value,
            "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
        ),
        "approval_policy" => {
            matches!(value, "never" | "untrusted" | "on-request" | "on-failure")
        }
        "sandbox_mode" => {
            matches!(
                value,
                "read-only" | "workspace-write" | "danger-full-access"
            )
        }
        _ => false,
    }
}

fn diagnostic_preference(key: &str, value: &Value) -> Value {
    if key == "developer_instructions" {
        return json!("[present; content withheld]");
    }
    if let Some(flag) = value.as_bool() {
        return json!(flag);
    }
    if let Some(text) = value.as_str()
        && allowed_preference(key, text)
    {
        return json!(text);
    }
    json!("[value withheld]")
}

fn diagnostic_source(name: &Value) -> io::Result<Value> {
    let name = object(name)?;
    let kind = name
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let mut source = Map::new();
    source.insert("type".into(), json!(kind));
    for key in ["file", "dotCodexFolder", "profile"] {
        match name.get(key) {
            None => {}
            Some(Value::Null) => {
                source.insert(key.into(), Value::Null);
            }
            Some(Value::String(text)) => {
                source.insert(key.into(), json!(text));
            }
            Some(_) => return Err(invalid()),
        }
    }
    Ok(Value::Object(source))
}

fn source_profile(source: &Value) -> Option<&str> {
    source.get("profile").and_then(Value::as_str)
}

fn has_skill_errors(errors: Option<&Value>) -> bool {
    match errors {
        None | Some(Value::Null) => false,
        Some(Value::Array(items)) => !items.is_empty(),
        Some(_) => true,
    }
}

pub(crate) fn summarize(
    project: &Path,
    profile_name: &str,
    profile_path: &Path,
    base: &Value,
    profile: &Value,
    requirements: &Value,
    skills: &Value,
) -> io::Result<Value> {
    let base = object(base)?;
    if !base.contains_key("origins") {
        return Err(invalid());
    }
    let native_layers = array(base.get("layers").unwrap_or(&Value::Null))?;
    let profile_config = object(profile)?;
    let requirements = object(requirements)?;
    let listed = object(skills)?;

    let mut findings = Vec::new();
    let context_changed = profile_changes_context(project, profile_config)?;
    if context_changed {
        findings.push(json!({
            "code": "profile-context-unresolved",
            "source": path_text(profile_path),
            "action": "The profile can alter discovery, trust or skill loading. Use the selected-profile CLI to inspect this project; no effective settings are asserted."
        }));
    }
    let requirements_present = !requirements
        .get("requirements")
        .ok_or_else(invalid)?
        .is_null();
    if requirements_present {
        findings.push(json!({
            "code": "runtime-requirements-present",
            "source": "native requirements",
            "action": "Merged declarations may be constrained by managed requirements; check the selected-profile runtime before relying on these values."
        }));
    }

    let mut layers = Vec::new();
    let mut inserted = false;
    for layer in native_layers.iter().rev() {
        let layer = object(layer)?;
        let name = layer.get("name").ok_or_else(invalid)?;
        let config = layer.get("config").ok_or_else(invalid)?;
        object(name)?;
        object(config)?;
        let kind = name
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let disabled = layer.get("disabledReason").is_some_and(truthy);
        layers.push((name.clone(), config.clone(), disabled));
        if kind == "user" {
            layers.push((
                json!({
                    "type": "user",
                    "file": path_text(profile_path),
                    "profile": profile_name
                }),
                profile.clone(),
                false,
            ));
            inserted = true;
        }
    }
    if !inserted {
        return Err(io::Error::other("missing native user layer"));
    }

    let withhold = context_changed || requirements_present;
    let mut settings = Vec::new();
    for key in SAFE_KEYS {
        let mut declarations = Vec::new();
        for (name, config, disabled) in &layers {
            let Some(raw) = diagnostic_value(config, key) else {
                continue;
            };
            declarations.push(json!({
                "source": diagnostic_source(name)?,
                "active": !disabled,
                "value": diagnostic_preference(key, raw)
            }));
        }
        let winner = declarations
            .iter()
            .rfind(|row| row["active"] == true)
            .cloned();
        let (value, origin) = match (&winner, withhold) {
            (Some(row), false) => (row["value"].clone(), row["source"].clone()),
            _ => (Value::Null, Value::Null),
        };
        let profile_declared = declarations.iter().any(|row| {
            row["active"] == true && source_profile(&row["source"]) == Some(profile_name)
        });
        let overridden =
            profile_declared && !origin.is_null() && source_profile(&origin) != Some(profile_name);
        settings.push(json!({
            "key": key,
            "value": value,
            "origin": origin,
            "declarations": declarations,
            "overridden": overridden,
            "evidence": "inferred-from-native-layers",
            "runtimeDefault": "not-inferred"
        }));
        if overridden {
            findings.push(json!({
                "code": "setting-overridden",
                "key": key,
                "source": origin,
                "action": "Review the winning source; retain an intentional override or edit it explicitly to use the profile."
            }));
        }
    }

    let mut layer_rows = Vec::new();
    for (name, _, disabled) in &layers {
        layer_rows.push(json!({
            "source": diagnostic_source(name)?,
            "status": if *disabled { "disabled" } else { "active" }
        }));
    }

    let mut skill_rows = Vec::new();
    for entry in array(listed.get("data").unwrap_or(&Value::Null))? {
        let entry = object(entry)?;
        let cwd = entry
            .get("cwd")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        for skill in array(entry.get("skills").unwrap_or(&Value::Null))? {
            let skill = object(skill)?;
            let name = skill
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?;
            let path = skill
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?;
            let enabled = skill
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(invalid)?;
            let scope = skill
                .get("scope")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?;
            skill_rows.push((name.to_owned(), path.to_owned(), enabled, scope.to_owned()));
        }
        if has_skill_errors(entry.get("errors")) {
            findings.push(json!({
                "code": "skill-load-error",
                "source": cwd,
                "action": "Inspect skill frontmatter locally; native error text is withheld."
            }));
        }
    }

    let mut seen = BTreeSet::new();
    skill_rows.retain(|(name, path, _, _)| seen.insert((name.clone(), path.clone())));
    skill_rows.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));

    let mut index = 0;
    while index < skill_rows.len() {
        let name = skill_rows[index].0.clone();
        let mut sources = Vec::new();
        while index < skill_rows.len() && skill_rows[index].0 == name {
            if skill_rows[index].2 {
                sources.push(skill_rows[index].1.clone());
            }
            index += 1;
        }
        if sources.len() > 1 {
            findings.push(json!({
                "code": "skill-name-collision",
                "name": name,
                "sources": sources,
                "action": "Rename or explicitly disable the unintended skill source; same-name skills are ambiguous."
            }));
        }
    }

    let skills = skill_rows
        .into_iter()
        .map(|(name, path, enabled, scope)| {
            json!({"name": name, "path": path, "enabled": enabled, "scope": scope})
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "settings": settings,
        "layers": layer_rows,
        "skills": skills,
        "findings": findings
    }))
}

#[cfg(test)]
mod tests {
    use super::summarize;
    use serde_json::{Value, json};
    use std::path::Path;

    const SENTINEL: &str = "DIAGNOSTIC_PRIVATE_SENTINEL_9b38";
    const PROJECT: &str = r"D:\consumer project";
    const PROFILE: &str = r"D:\home\.codex\harness.config.toml";

    fn leaked(value: &impl ToString) -> bool {
        let text = value.to_string();
        text.contains(SENTINEL) || text.contains("gpt-5") || text.contains("not-a-bool")
    }

    fn report(
        base: &Value,
        profile: &Value,
        requirements: &Value,
        skills: &Value,
    ) -> Result<Value, String> {
        summarize(
            Path::new(PROJECT),
            "harness",
            Path::new(PROFILE),
            base,
            profile,
            requirements,
            skills,
        )
        .map_err(|error| error.to_string())
    }

    fn setting<'a>(value: &'a Value, key: &str) -> &'a Value {
        value["settings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == key)
            .unwrap()
    }

    fn layer(kind: &str, file: &str, config: Value, disabled: Option<&str>) -> Value {
        let mut name = json!({"type": kind, "file": file});
        if kind == "project" {
            name["dotCodexFolder"] = json!(r"D:\consumer project\.codex");
        }
        let mut row = json!({"name": name, "config": config});
        if let Some(reason) = disabled {
            row["disabledReason"] = json!(reason);
        }
        row
    }

    fn base(layers: Vec<Value>) -> Value {
        json!({"origins": {"note": SENTINEL}, "layers": layers})
    }

    fn empty_skills() -> Value {
        json!({"data": [{"cwd": PROJECT, "skills": [], "errors": []}]})
    }

    fn view(layers: Vec<Value>, profile: Value, requirements: Value, skills: Value) -> Value {
        let result = report(&base(layers), &profile, &requirements, &skills);
        assert!(!leaked(&format!("{result:?}")));
        result.expect("valid observation")
    }

    fn codes(value: &Value) -> Vec<&str> {
        value["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["code"].as_str())
            .collect()
    }

    #[test]
    fn privacy_sentinel_is_withheld_across_unknown_values_developer_text_skill_descriptions_and_parser_errors()
     {
        let profile = json!({
            "model": "gpt-6-astra",
            "model_reasoning_effort": "xhigh",
            "developer_instructions": SENTINEL
        });
        let project = json!({
            "model": "gpt-5",
            "developer_instructions": SENTINEL,
            "features": {"hooks": "not-a-bool"}
        });
        let skills = json!({
            "data": [{
                "cwd": PROJECT,
                "skills": [{
                    "name": "openspec-apply-change",
                    "path": r"D:\consumer project\.agents\skills\one\SKILL.md",
                    "enabled": true,
                    "scope": "repo",
                    "description": SENTINEL,
                    "body": SENTINEL
                }],
                "errors": [SENTINEL, {"message": SENTINEL}]
            }]
        });
        let value = view(
            vec![
                layer(
                    "project",
                    r"D:\consumer project\.codex\config.toml",
                    project,
                    None,
                ),
                layer(
                    "system",
                    r"D:\home\.codex\system.toml",
                    json!({"model": SENTINEL}),
                    Some(SENTINEL),
                ),
                layer(
                    "user",
                    r"D:\home\.codex\config.toml",
                    json!({"model": SENTINEL}),
                    None,
                ),
            ],
            profile,
            json!({"requirements": null}),
            skills,
        );
        let encoded = serde_json::to_string(&value).unwrap();
        assert!(!leaked(&encoded));
        assert_eq!(setting(&value, "model")["value"], "[value withheld]");
        assert_eq!(
            setting(&value, "developer_instructions")["value"],
            "[present; content withheld]"
        );
        assert_eq!(
            setting(&value, "features.hooks")["value"],
            "[value withheld]"
        );
        assert_eq!(
            codes(&value)
                .into_iter()
                .filter(|code| *code == "skill-load-error")
                .count(),
            1
        );
        assert_eq!(value["layers"][2]["status"], "disabled");
        assert_eq!(value["layers"][2]["source"]["type"], "system");
        assert_eq!(value["skills"][0]["name"], "openspec-apply-change");
        assert!(value["skills"][0].get("description").is_none());
        assert_eq!(value["skills"][0].as_object().unwrap().len(), 4);
    }

    #[test]
    fn explicit_false_project_winner_and_disabled_layer() {
        let profile = json!({
            "model": "xai/grok-4.6",
            "model_reasoning_effort": "xhigh",
            "features": {"hooks": true, "multi_agent": false}
        });
        let trusted = view(
            vec![
                layer(
                    "project",
                    r"D:\consumer project\.codex\config.toml",
                    json!({
                        "model_reasoning_effort": "low",
                        "features": {"hooks": false}
                    }),
                    None,
                ),
                layer(
                    "user",
                    r"D:\home\.codex\config.toml",
                    json!({"approval_policy": "untrusted"}),
                    None,
                ),
            ],
            profile.clone(),
            json!({"requirements": null}),
            empty_skills(),
        );
        assert_eq!(trusted["layers"][0]["source"]["type"], "user");
        assert!(trusted["layers"][0]["source"].get("profile").is_none());
        assert_eq!(trusted["layers"][1]["source"]["profile"], "harness");
        assert_eq!(trusted["layers"][1]["source"]["file"], PROFILE);
        assert_eq!(trusted["layers"][2]["source"]["type"], "project");
        assert_eq!(trusted["layers"][2]["status"], "active");
        let hooks = setting(&trusted, "features.hooks");
        assert_eq!(hooks["value"], false);
        assert_eq!(hooks["origin"]["type"], "project");
        assert_eq!(hooks["overridden"], true);
        assert_eq!(hooks["evidence"], "inferred-from-native-layers");
        assert_eq!(hooks["runtimeDefault"], "not-inferred");
        assert_eq!(setting(&trusted, "model")["value"], "xai/grok-4.6");
        assert_eq!(setting(&trusted, "model_reasoning_effort")["value"], "low");
        assert_eq!(
            setting(&trusted, "model_reasoning_effort")["origin"]["type"],
            "project"
        );
        assert_eq!(setting(&trusted, "features.multi_agent")["value"], false);
        assert_eq!(
            setting(&trusted, "features.multi_agent")["origin"]["profile"],
            "harness"
        );
        assert_eq!(setting(&trusted, "approval_policy")["value"], "untrusted");
        assert!(codes(&trusted).contains(&"setting-overridden"));

        let untrusted = view(
            vec![
                layer(
                    "project",
                    r"D:\consumer project\.codex\config.toml",
                    json!({"model_reasoning_effort": "low", "features": {"hooks": false}}),
                    Some("untrusted"),
                ),
                layer("user", r"D:\home\.codex\config.toml", json!({}), None),
            ],
            profile,
            json!({"requirements": null}),
            empty_skills(),
        );
        let reasoning = setting(&untrusted, "model_reasoning_effort");
        assert_eq!(reasoning["value"], "xhigh");
        assert_eq!(reasoning["origin"]["profile"], "harness");
        assert_eq!(reasoning["overridden"], false);
        assert_eq!(untrusted["layers"][2]["status"], "disabled");
        assert_eq!(
            reasoning["declarations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["source"]["type"] == "project")
                .unwrap()["active"],
            false
        );
        assert!(!codes(&untrusted).contains(&"setting-overridden"));
    }

    #[test]
    fn profile_context_and_requirements_withhold_winners() {
        let layers = vec![
            layer(
                "project",
                r"D:\consumer project\.codex\config.toml",
                json!({"model_reasoning_effort": "low"}),
                None,
            ),
            layer("user", r"D:\home\.codex\config.toml", json!({}), None),
        ];
        let context = view(
            layers.clone(),
            json!({
                "model": "gpt-6-astra",
                "model_reasoning_effort": "xhigh",
                "projects": { "D:\\consumer project": {"trust_level": "trusted"} }
            }),
            json!({"requirements": null}),
            empty_skills(),
        );
        assert!(codes(&context).contains(&"profile-context-unresolved"));
        assert_eq!(context["findings"][0]["source"], PROFILE);
        assert!(
            context["settings"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["origin"].is_null()
                    && row["value"].is_null()
                    && row["overridden"] == false)
        );

        let nested = summarize(
            Path::new(r"D:\consumer project\nested"),
            "harness",
            Path::new(PROFILE),
            &base(layers.clone()),
            &json!({"model": "gpt-6-astra", "projects": { "D:\\consumer project": {} }}),
            &json!({"requirements": null}),
            &empty_skills(),
        )
        .unwrap();
        assert!(codes(&nested).contains(&"profile-context-unresolved"));
        assert!(setting(&nested, "model")["origin"].is_null());

        let markers = view(
            layers.clone(),
            json!({"model": "gpt-6-astra", "project_root_markers": []}),
            json!({"requirements": null}),
            empty_skills(),
        );
        assert!(codes(&markers).contains(&"profile-context-unresolved"));
        assert!(setting(&markers, "model")["origin"].is_null());

        let broker = view(
            layers.clone(),
            json!({"model": "gpt-6-astra", "credential_broker": {"command": SENTINEL}}),
            json!({"requirements": null}),
            empty_skills(),
        );
        assert!(codes(&broker).contains(&"profile-context-unresolved"));
        assert!(!leaked(&serde_json::to_string(&broker).unwrap()));

        let managed = view(
            layers.clone(),
            json!({"model": "gpt-6-astra"}),
            json!({"requirements": {"id": SENTINEL}}),
            empty_skills(),
        );
        assert!(codes(&managed).contains(&"runtime-requirements-present"));
        assert!(
            managed["settings"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["origin"].is_null() && row["value"].is_null())
        );
        assert!(!leaked(&serde_json::to_string(&managed).unwrap()));

        let sibling = view(
            layers,
            json!({
                "model": "gpt-6-astra",
                "projects": { "D:\\other project": {"trust_level": "trusted"} }
            }),
            json!({"requirements": null}),
            empty_skills(),
        );
        assert!(!codes(&sibling).contains(&"profile-context-unresolved"));
        assert!(!codes(&sibling).contains(&"runtime-requirements-present"));
        assert_eq!(setting(&sibling, "model")["value"], "gpt-6-astra");
        assert_eq!(setting(&sibling, "model_reasoning_effort")["value"], "low");
    }

    #[test]
    fn skill_collisions_and_dedup() {
        let skills = json!({
            "data": [{
                "cwd": PROJECT,
                "skills": [
                    {
                        "name": "openspec-apply-change",
                        "path": r"D:\a\SKILL.md",
                        "enabled": true,
                        "scope": "repo",
                        "description": SENTINEL
                    },
                    {
                        "name": "openspec-apply-change",
                        "path": r"D:\a\SKILL.md",
                        "enabled": false,
                        "scope": "user"
                    },
                    {
                        "name": "openspec-apply-change",
                        "path": r"D:\b\SKILL.md",
                        "enabled": true,
                        "scope": "user"
                    },
                    {
                        "name": "openspec-apply-change",
                        "path": r"D:\c\SKILL.md",
                        "enabled": false,
                        "scope": "repo"
                    },
                    {
                        "name": "unique-skill",
                        "path": r"D:\d\SKILL.md",
                        "enabled": true,
                        "scope": "repo"
                    }
                ],
                "errors": []
            }]
        });
        let value = view(
            vec![layer(
                "user",
                r"D:\home\.codex\config.toml",
                json!({}),
                None,
            )],
            json!({"model": "gpt-6-astra"}),
            json!({"requirements": null}),
            skills,
        );
        assert_eq!(
            value["skills"],
            json!([
                {"name": "openspec-apply-change", "path": r"D:\a\SKILL.md", "enabled": true, "scope": "repo"},
                {"name": "openspec-apply-change", "path": r"D:\b\SKILL.md", "enabled": true, "scope": "user"},
                {"name": "openspec-apply-change", "path": r"D:\c\SKILL.md", "enabled": false, "scope": "repo"},
                {"name": "unique-skill", "path": r"D:\d\SKILL.md", "enabled": true, "scope": "repo"}
            ])
        );
        let collisions: Vec<_> = value["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["code"] == "skill-name-collision")
            .collect();
        assert_eq!(collisions.len(), 1);
        assert_eq!(collisions[0]["name"], "openspec-apply-change");
        assert_eq!(
            collisions[0]["sources"],
            json!([r"D:\a\SKILL.md", r"D:\b\SKILL.md"])
        );
        assert!(!leaked(&serde_json::to_string(&value).unwrap()));
    }

    #[test]
    fn malformed_required_fields_return_generic_errors_without_input() {
        let profile = json!({"model": "gpt-6-astra"});
        let skills = empty_skills();
        let requirements = json!({"requirements": null});
        let valid_layers = vec![layer(
            "user",
            r"D:\home\.codex\config.toml",
            json!({"model": SENTINEL}),
            None,
        )];
        assert!(report(&base(valid_layers.clone()), &profile, &json!({}), &skills).is_err());
        let cases = [
            json!({"layers": valid_layers, "config": {"model": SENTINEL}}),
            json!({"origins": {}, "layers": {"model": SENTINEL}}),
            json!({"origins": {}, "layers": [{"name": {"type": "user"}, "config": SENTINEL}]}),
            json!({"origins": {}, "layers": [{"config": {}, "name": SENTINEL}]}),
            json!({"origins": {}, "layers": [{"name": {"type": "user", "file": {"path": SENTINEL}}, "config": {}}]}),
            json!({"origins": {}, "layers": [{"name": {"file": r"D:\home\.codex\config.toml"}, "config": {}}]}),
            json!({"origins": {}, "layers": []}),
            json!({"origins": {}, "layers": [layer("project", r"D:\p\.codex\config.toml", json!({}), None)]}),
        ];
        for candidate in cases {
            let result = report(&candidate, &profile, &requirements, &skills);
            assert!(result.is_err(), "accepted malformed observation");
            assert!(!leaked(&format!("{result:?}")));
            assert!(!format!("{result:?}").contains("SENTINEL"));
        }
        let result = report(
            &base(valid_layers.clone()),
            &json!([SENTINEL]),
            &requirements,
            &skills,
        );
        assert!(result.is_err());
        assert!(!leaked(&format!("{result:?}")));
        let result = report(
            &base(valid_layers.clone()),
            &profile,
            &json!([SENTINEL]),
            &skills,
        );
        assert!(result.is_err());
        assert!(!leaked(&format!("{result:?}")));
        let result = report(
            &base(valid_layers.clone()),
            &profile,
            &requirements,
            &json!({"data": SENTINEL}),
        );
        assert!(result.is_err());
        assert!(!leaked(&format!("{result:?}")));
        let result = report(
            &base(valid_layers.clone()),
            &profile,
            &requirements,
            &json!({"data": [{"cwd": PROJECT, "skills": [{"path": r"D:\a\SKILL.md", "enabled": true, "scope": "repo", "description": SENTINEL}], "errors": []}]}),
        );
        assert!(result.is_err());
        assert!(!leaked(&format!("{result:?}")));
        let result = report(
            &base(valid_layers),
            &profile,
            &requirements,
            &json!({"data": [{"cwd": PROJECT, "skills": [{"name": "x", "path": r"D:\a\SKILL.md", "enabled": SENTINEL, "scope": "repo"}], "errors": []}]}),
        );
        assert!(result.is_err());
        assert!(!leaked(&format!("{result:?}")));
    }

    #[test]
    fn windows_trust_ancestry_normalizes_roots_dot_segments_and_unicode_case() {
        for (project, trust) in [
            (r"D:\child", r"D:\"),
            (r"D:\ПРОЕКТ\child", r"d:\проект"),
            (r"D:\parent\project\child", r"D:\parent\unused\..\project\."),
        ] {
            assert!(super::project_under_trust(Path::new(project), trust).unwrap());
        }
        assert!(!super::project_under_trust(Path::new(r"D:\проект-other"), r"D:\ПРОЕКТ").unwrap());
        let result = report(
            &base(vec![layer("user", PROFILE, json!({}), None)]),
            &json!({"model":"gpt-6-astra","projects":{"D:\\":{"trust_level":"trusted"}}}),
            &json!({"requirements":null}),
            &empty_skills(),
        )
        .unwrap();
        assert!(
            result["settings"]
                .as_array()
                .unwrap()
                .iter()
                .all(|setting| setting["origin"].is_null())
        );
        assert!(codes(&result).contains(&"profile-context-unresolved"));
    }
}
