//! Read portable defaults live and express them as native CLI leaf overrides.
use std::{ffi::OsString, fs, io, path::Path};

/// Where an effective native preference came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferenceSource {
    Project,
    User,
    Shared,
    None,
}

impl PreferenceSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "trusted-project",
            Self::User => "user",
            Self::Shared => "shared-default",
            Self::None => "none",
        }
    }
}

/// Effective native model and reasoning effort for a plain session start in
/// `working_directory`, resolved over the installed consumer's configuration
/// layers. A read failure is an error so the caller can refuse to guess.
#[derive(Debug)]
pub struct EffectivePreferences {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub model_source: PreferenceSource,
    pub effort_source: PreferenceSource,
}

pub fn overrides(
    shared_file: &Path,
    codex_home: &Path,
    working_directory: &Path,
) -> io::Result<Vec<OsString>> {
    let shared = read(shared_file)?;
    let local = read_user(codex_home)?;
    let project = trusted_project_config(&local, working_directory)?;
    let mut result = Vec::new();
    for (key, value) in &shared {
        if key != "projects" {
            leaves(
                &mut result,
                &mut vec![key.as_str()],
                value,
                &local,
                project.as_ref(),
                codex_home,
            )?;
        }
    }
    Ok(result)
}

/// Resolve the model and reasoning effort a plain session start in
/// `working_directory` would use. The applicable trusted-project file wins
/// over the user configuration; the portable shared file remains the lowest
/// model default and never counts as a native effort choice, because the
/// launcher applies it as its own explicit override when nothing shields it.
pub fn effective_preferences(
    shared_file: &Path,
    codex_home: &Path,
    working_directory: &Path,
) -> io::Result<EffectivePreferences> {
    let shared = read(shared_file)?;
    let user = read_user(codex_home)?;
    let project = trusted_project_config(&user, working_directory)?;
    let preference = |table: Option<&toml::Table>, key: &str| {
        table.and_then(|table| {
            table
                .get(key)
                .and_then(|value| value.as_str().map(str::to_owned))
        })
    };
    let (model, model_source) = match (
        preference(project.as_ref(), "model"),
        preference(Some(&user), "model"),
        preference(Some(&shared), "model"),
    ) {
        (Some(value), _, _) => (Some(value), PreferenceSource::Project),
        (None, Some(value), _) => (Some(value), PreferenceSource::User),
        (None, None, Some(value)) => (Some(value), PreferenceSource::Shared),
        (None, None, None) => (None, PreferenceSource::None),
    };
    let (effort, effort_source) = match (
        preference(project.as_ref(), "model_reasoning_effort"),
        preference(Some(&user), "model_reasoning_effort"),
    ) {
        (Some(value), _) => (Some(value), PreferenceSource::Project),
        (None, Some(value)) => (Some(value), PreferenceSource::User),
        (None, None) => (None, PreferenceSource::None),
    };
    Ok(EffectivePreferences {
        model,
        effort,
        model_source,
        effort_source,
    })
}

fn read(path: &Path) -> io::Result<toml::Table> {
    parse(&fs::read_to_string(path)?)
}

fn read_user(codex_home: &Path) -> io::Result<toml::Table> {
    match fs::read_to_string(codex_home.join("config.toml")) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(error) => Err(error),
    }
}

/// The applicable trusted-project configuration file for a session started in
/// `working_directory`, following the installed consumer's observed rules: a
/// trust entry names the working directory itself or an ancestor inside its
/// git worktree, and the nearest `.codex/config.toml` from the working
/// directory up to that trusted directory applies. Untrusted or unrelated
/// files never apply. An unreadable or invalid applicable file is an error so
/// callers refuse to guess over it.
fn trusted_project_config(
    user: &toml::Table,
    working_directory: &Path,
) -> io::Result<Option<toml::Table>> {
    let Some(trusted_root) = trusted_directory(user, working_directory) else {
        return Ok(None);
    };
    let mut current = Some(working_directory);
    while let Some(directory) = current {
        let config = directory.join(".codex").join("config.toml");
        match fs::read_to_string(&config) {
            Ok(text) => return parse(&text).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if directory == trusted_root {
            return Ok(None);
        }
        current = directory.parent();
    }
    Ok(None)
}

/// The nearest directory from the working directory upward whose exact trust
/// entry marks it trusted. Outside a git worktree only the working directory
/// itself can be trusted; inside one, ancestors up to the worktree root can.
fn trusted_directory<'a>(user: &toml::Table, working_directory: &'a Path) -> Option<&'a Path> {
    let projects = user.get("projects").and_then(|value| value.as_table())?;
    let mut worktree_root = None;
    let mut current = Some(working_directory);
    while let Some(directory) = current {
        if directory.join(".git").exists() {
            worktree_root = Some(directory);
            break;
        }
        current = directory.parent();
    }
    let boundary = worktree_root.unwrap_or(working_directory);
    let mut current = Some(working_directory);
    while let Some(directory) = current {
        if projects
            .get(&path_key(directory))
            .and_then(|entry| entry.get("trust_level"))
            .and_then(|value| value.as_str())
            == Some("trusted")
        {
            return Some(directory);
        }
        if directory == boundary {
            return None;
        }
        current = directory.parent();
    }
    None
}

/// The absolute path string a native `projects` trust key uses, without the
/// verbatim `\\?\` prefix `canonicalize` produces on Windows.
fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\")
        .unwrap_or(text.as_ref())
        .to_owned()
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
    project: Option<&toml::Table>,
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
            leaves(output, keys, child, local, project, home)?;
            keys.pop();
        }
        return Ok(());
    }
    if matches!(
        keys[0],
        "model" | "model_reasoning_effort" | "features" | "tui" | "notice"
    ) {
        let mut selected = local.get(keys[0]);
        for key in &keys[1..] {
            selected = selected.and_then(|v| v.get(*key));
        }
        let mut project_selected = project.and_then(|table| table.get(keys[0]));
        for key in &keys[1..] {
            project_selected = project_selected.and_then(|v| v.get(*key));
        }
        if selected.is_some() || project_selected.is_some() {
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
            "model='local-fixture'\n[features]\ncode_mode=true\n[tui]\npet='local'\n[agents.user]\nconfig_file='keep.toml'\n",
        )
        .unwrap();
        fs::write(&shared, "approval_policy='never'\nmodel='shared-fixture'\ndeveloper_instructions='''A \"quote\"\nКириллица'''\n[features]\napps=false\n[projects.'C:/synthetic-consumer']\ntrust_level='trusted'\n[tui]\npet='shared'\n[agents.worker]\nconfig_file='agents/worker.toml'\n").unwrap();
        let args = overrides(&shared, &home, root.path()).unwrap();
        let text = args
            .iter()
            .map(|v| v.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("approval_policy=\"never\""));
        // A machine-local feature leaf wins over the portable default, while
        // an absent local leaf still receives the portable value.
        assert!(text.contains("features.apps=false"));
        assert!(
            !text.contains("projects") && !text.contains("model=") && !text.contains("tui.pet")
        );
        assert!(text.contains("agents.worker.config_file="));
        assert!(!text.contains("agents="));
        for pair in args.as_chunks::<2>().0 {
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
        fs::write(
            home.join("config.toml"),
            "model='local-fixture'\n[features]\ncode_mode=true\napps=true\n",
        )
        .unwrap();
        let local_apps = overrides(&shared, &home, root.path())
            .unwrap()
            .iter()
            .map(|v| v.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!local_apps.contains("features.apps="));
        fs::write(&shared, "approval_policy='on-request'\n").unwrap();
        assert_eq!(
            overrides(&shared, &home, root.path()).unwrap(),
            vec![
                OsString::from("-c"),
                OsString::from("approval_policy=\"on-request\"")
            ]
        );
    }

    #[test]
    fn effective_preferences_follow_the_qualified_native_layers() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared.toml");
        let home = root.path().join("home");
        let project = root.path().join("project");
        fs::create_dir_all(project.join(".codex")).unwrap();
        fs::write(&shared, "model='shared-model'\n").unwrap();

        // No user or project configuration: only the shared model default.
        fs::create_dir(&home).unwrap();
        let resolved = effective_preferences(&shared, &home, &project).unwrap();
        assert_eq!(resolved.model.as_deref(), Some("shared-model"));
        assert_eq!(resolved.model_source, PreferenceSource::Shared);
        assert_eq!(resolved.effort, None);
        assert_eq!(resolved.effort_source, PreferenceSource::None);

        // User model and effort survive; an untrusted project file never
        // applies, matching the installed consumer.
        fs::write(
            home.join("config.toml"),
            "model='user-model'\nmodel_reasoning_effort='low'\n",
        )
        .unwrap();
        fs::write(
            project.join(".codex/config.toml"),
            "model='project-model'\nmodel_reasoning_effort='medium'\n",
        )
        .unwrap();
        let resolved = effective_preferences(&shared, &home, &project).unwrap();
        assert_eq!(resolved.model.as_deref(), Some("user-model"));
        assert_eq!(resolved.model_source, PreferenceSource::User);
        assert_eq!(resolved.effort.as_deref(), Some("low"));
        assert_eq!(resolved.effort_source, PreferenceSource::User);

        // An exact trust entry for the working directory applies the project
        // file over the user values.
        let key = path_key(&project);
        fs::write(
            home.join("config.toml"),
            format!(
                "model='user-model'\nmodel_reasoning_effort='low'\n[projects.'{key}']\ntrust_level='trusted'\n"
            ),
        )
        .unwrap();
        let resolved = effective_preferences(&shared, &home, &project).unwrap();
        assert_eq!(resolved.model.as_deref(), Some("project-model"));
        assert_eq!(resolved.model_source, PreferenceSource::Project);
        assert_eq!(resolved.effort.as_deref(), Some("medium"));
        assert_eq!(resolved.effort_source, PreferenceSource::Project);
    }

    #[test]
    fn trusted_project_layers_follow_worktree_boundaries() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared.toml");
        let home = root.path().join("home");
        let repository = root.path().join("repository");
        let nested = repository.join("nested");
        let leaf = nested.join("leaf");
        fs::create_dir_all(leaf.clone()).unwrap();
        fs::create_dir(&home).unwrap();
        fs::write(&shared, "model='shared-model'\n").unwrap();
        fs::write(
            home.join("config.toml"),
            "model='user-model'\nmodel_reasoning_effort='low'\n",
        )
        .unwrap();

        // A trust entry for an ancestor only applies inside a git worktree.
        let repository_key = path_key(&repository);
        fs::write(
            home.join("config.toml"),
            format!("model='user-model'\n[projects.'{repository_key}']\ntrust_level='trusted'\n"),
        )
        .unwrap();
        fs::create_dir_all(repository.join(".codex")).unwrap();
        fs::write(
            repository.join(".codex/config.toml"),
            "model='repository-model'\n",
        )
        .unwrap();
        let resolved = effective_preferences(&shared, &home, &leaf).unwrap();
        assert_eq!(resolved.model.as_deref(), Some("user-model"));

        // Inside the worktree the trusted ancestor applies, and the nearest
        // project file from the working directory upward wins.
        fs::create_dir_all(repository.join(".git")).unwrap();
        fs::create_dir_all(nested.join(".codex")).unwrap();
        fs::write(nested.join(".codex/config.toml"), "model='nested-model'\n").unwrap();
        let resolved = effective_preferences(&shared, &home, &leaf).unwrap();
        assert_eq!(resolved.model.as_deref(), Some("nested-model"));
        assert_eq!(resolved.model_source, PreferenceSource::Project);

        // A trust entry outside the worktree never applies.
        let root_key = path_key(root.path());
        fs::write(
            home.join("config.toml"),
            format!(
                "model='user-model'\n[projects.'{root_key}']\ntrust_level='trusted'\n[projects.'{repository_key}']\ntrust_level='untrusted'\n"
            ),
        )
        .unwrap();
        let resolved = effective_preferences(&shared, &home, &leaf).unwrap();
        assert_eq!(resolved.model.as_deref(), Some("user-model"));
    }

    #[test]
    fn unresolvable_native_configuration_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared.toml");
        let home = root.path().join("home");
        let project = root.path().join("project");
        fs::create_dir_all(project.join(".codex")).unwrap();
        fs::create_dir(&home).unwrap();
        fs::write(&shared, "model='shared-model'\n").unwrap();
        fs::write(home.join("config.toml"), "model = [\n").unwrap();
        assert!(effective_preferences(&shared, &home, &project).is_err());

        fs::write(
            home.join("config.toml"),
            format!(
                "[projects.'{}']\ntrust_level='trusted'\n",
                path_key(&project)
            ),
        )
        .unwrap();
        fs::write(project.join(".codex/config.toml"), "model = [\n").unwrap();
        assert!(effective_preferences(&shared, &home, &project).is_err());
    }

    #[test]
    fn trusted_project_configuration_shields_shared_defaults() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared.toml");
        let home = root.path().join("home");
        let project = root.path().join("project");
        fs::create_dir_all(project.join(".codex")).unwrap();
        fs::create_dir(&home).unwrap();
        fs::write(
            &shared,
            "model='shared-model'\nmodel_reasoning_effort='xhigh'\n",
        )
        .unwrap();
        fs::write(
            home.join("config.toml"),
            format!(
                "[projects.'{}']\ntrust_level='trusted'\n",
                path_key(&project)
            ),
        )
        .unwrap();
        fs::write(
            project.join(".codex/config.toml"),
            "model='project-model'\nmodel_reasoning_effort='medium'\n",
        )
        .unwrap();
        let args = overrides(&shared, &home, &project).unwrap();
        assert!(args.is_empty());

        // Without the project layer both shared defaults stay applicable.
        fs::write(
            project.join(".codex/config.toml"),
            "approval_policy='never'\n",
        )
        .unwrap();
        let args = overrides(&shared, &home, &project)
            .unwrap()
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"model=\"shared-model\"".to_owned()));
        assert!(args.contains(&"model_reasoning_effort=\"xhigh\"".to_owned()));
    }
}
