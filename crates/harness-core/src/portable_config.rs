//! Read portable defaults live and express them as native CLI leaf overrides.
use std::{ffi::OsString, fs, io, path::Path};

pub fn overrides(shared_file: &Path, codex_home: &Path) -> io::Result<Vec<OsString>> {
    let shared = read(shared_file)?;
    let local = match fs::read_to_string(codex_home.join("config.toml")) {
        Ok(text) => parse(&text)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(error) => return Err(error),
    };
    let mut result = Vec::new();
    for (key, value) in &shared {
        if key != "projects" {
            leaves(
                &mut result,
                &mut vec![key.as_str()],
                value,
                &local,
                codex_home,
            )?;
        }
    }
    Ok(result)
}

fn read(path: &Path) -> io::Result<toml::Table> {
    parse(&fs::read_to_string(path)?)
}
fn parse(text: &str) -> io::Result<toml::Table> {
    text.trim_start_matches('\u{feff}').parse().map_err(|_| {
        io::Error::other("Invalid shared or local configuration TOML; inspect it locally.")
    })
}

fn leaves<'a>(
    output: &mut Vec<OsString>,
    keys: &mut Vec<&'a str>,
    value: &'a toml::Value,
    local: &toml::Table,
    home: &Path,
) -> io::Result<()> {
    // Codex -c paths are literal dotted components, not TOML quoted keys.
    if keys.iter().any(|key| {
        key.is_empty()
            || !key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    }) {
        return Err(io::Error::other(
            "Shared setting key cannot be represented by native dotted overrides.",
        ));
    }
    if let toml::Value::Table(table) = value {
        for (key, child) in table {
            keys.push(key);
            leaves(output, keys, child, local, home)?;
            keys.pop();
        }
        return Ok(());
    }
    if matches!(
        keys[0],
        "model" | "model_reasoning_effort" | "tui" | "notice"
    ) {
        let mut selected = local.get(keys[0]);
        for key in &keys[1..] {
            selected = selected.and_then(|v| v.get(*key));
        }
        if selected.is_some() {
            return Ok(());
        }
    }
    let value = if keys.len() == 3 && keys[0] == "agents" && keys[2] == "config_file" {
        let path = Path::new(
            value
                .as_str()
                .ok_or_else(|| io::Error::other("Agent config_file must be a string."))?,
        );
        toml::Value::String(
            if path.is_absolute() {
                path.to_owned()
            } else {
                home.join(path)
            }
            .to_string_lossy()
            .into_owned(),
        )
    } else {
        value.clone()
    };
    output.push("-c".into());
    output.push(format!("{}={value}", keys.join(".")).into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_policy_local_preferences_and_leaf_arguments() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared.toml");
        let home = root.path().join("local home");
        fs::create_dir(&home).unwrap();
        fs::write(
            home.join("config.toml"),
            "model='local-fixture'\n[tui]\npet='local'\n[agents.user]\nconfig_file='keep.toml'\n",
        )
        .unwrap();
        fs::write(&shared, "approval_policy='never'\nmodel='shared-fixture'\ndeveloper_instructions='''A \"quote\"\nКириллица'''\n[projects.'C:/synthetic-consumer']\ntrust_level='trusted'\n[tui]\npet='shared'\n[agents.worker]\nconfig_file='agents/worker.toml'\n").unwrap();
        let args = overrides(&shared, &home).unwrap();
        let text = args
            .iter()
            .map(|v| v.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("approval_policy=\"never\""));
        assert!(
            !text.contains("projects") && !text.contains("model=") && !text.contains("tui.pet")
        );
        assert!(text.contains("agents.worker.config_file="));
        assert!(!text.contains("agents="));
        for pair in args.chunks_exact(2) {
            assert_eq!(pair[0], "-c");
            let (_, encoded) = pair[1].to_str().unwrap().split_once('=').unwrap();
            let parsed: toml::Table = format!("value={encoded}").parse().unwrap();
            if pair[1]
                .to_string_lossy()
                .starts_with("developer_instructions=")
            {
                assert_eq!(parsed["value"].as_str(), Some("A \"quote\"\nКириллица"));
            }
        }
        fs::write(&shared, "approval_policy='on-request'\n").unwrap();
        assert_eq!(
            overrides(&shared, &home).unwrap(),
            vec![
                OsString::from("-c"),
                OsString::from("approval_policy=\"on-request\"")
            ]
        );
    }
}
