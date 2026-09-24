//! Kit orchestration roles. Missing profiles and invalid limits are errors.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, io, path::Path};

const SCHEMA: u32 = 1;
const MAX_EXECUTORS: u32 = 32;
const DEFAULT_VOTE_THRESHOLD: u32 = 3;
const DEFAULT_INCUBATOR_CAP: u32 = 32;
const DEFAULT_FEEDBACK_BATCH: u32 = 8;
const DEFAULT_WORKTREE_LIMIT: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Orchestration {
    pub schema: u32,
    pub lead_profile: String,
    pub successor_lead_profile: String,
    pub executor_profiles: Vec<String>,
    pub max_concurrent_executors: u32,
    #[serde(default = "default_vote_threshold")]
    pub vote_threshold: u32,
    #[serde(default = "default_incubator_cap")]
    pub incubator_size_cap: u32,
    #[serde(default = "default_feedback_batch")]
    pub feedback_batch_limit: u32,
    /// Lanes are reused, not multiplied; crossing this count means a lane was
    /// not reset or retired and needs lead attention.
    #[serde(default = "default_worktree_limit")]
    pub worktree_limit: u32,
}

fn default_vote_threshold() -> u32 {
    DEFAULT_VOTE_THRESHOLD
}
fn default_incubator_cap() -> u32 {
    DEFAULT_INCUBATOR_CAP
}
fn default_feedback_batch() -> u32 {
    DEFAULT_FEEDBACK_BATCH
}

fn default_worktree_limit() -> u32 {
    DEFAULT_WORKTREE_LIMIT
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBinding {
    pub profile: String,
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SuccessorChoice {
    pub schema: u32,
    pub profile: String,
    pub model: String,
    pub title: String,
}

pub fn load(source_root: &Path) -> io::Result<Orchestration> {
    parse(&fs::read(source_root.join("global/orchestration.toml"))?)
}

pub fn profile_args(profile: &str) -> io::Result<Vec<String>> {
    if profile.is_empty() {
        return Err(invalid("orchestration profile is missing"));
    }
    if profile == "default" {
        return Ok(Vec::new());
    }
    Ok(vec!["--profile".into(), profile.to_owned()])
}

/// Executor sessions run as single-agent workers. This environment marker
/// lets the installed launcher recognize every Codex process started inside an
/// executor and keep the agent capability off.
pub const EXECUTOR_SESSION_ENV: &str = "HARNESS_EXECUTOR_SESSION";

/// Codex CLI's built-in configuration switch that forces the session's
/// multi-agent version to `Disabled`: it removes the agent tool set and its
/// usage instructions even when the selected model catalog advertises
/// multi-agent support.
pub const EXECUTOR_AGENT_TOOLS_OFF: [&str; 2] = ["-c", "agents.enabled=false"];

/// Executor session arguments: profile selection plus the single-agent limit,
/// in the order Codex requires (global options before the subcommand).
pub fn executor_session_args(profile: &str) -> io::Result<Vec<String>> {
    let mut args = profile_args(profile)?;
    args.extend(EXECUTOR_AGENT_TOOLS_OFF.map(str::to_owned));
    Ok(args)
}

pub fn executor_profile(config: &Orchestration, requested: Option<&str>) -> io::Result<String> {
    match requested {
        None => config
            .executor_profiles
            .first()
            .cloned()
            .ok_or_else(|| invalid("orchestration executor_profiles is missing")),
        Some(name) => {
            if config
                .executor_profiles
                .iter()
                .any(|profile| profile == name)
            {
                Ok(name.to_owned())
            } else {
                Err(invalid(&format!(
                    "orchestration profile '{name}' is not an executor"
                )))
            }
        }
    }
}

pub fn binding(codex_home: &Path, profile: &str) -> io::Result<ProfileBinding> {
    if profile == "default" {
        return table_binding(profile, &read_toml(&codex_home.join("config.toml"))?);
    }
    let file = codex_home.join(format!("{profile}.config.toml"));
    if file.is_file() {
        return table_binding(profile, &read_toml(&file)?);
    }
    let document = read_toml(&codex_home.join("config.toml"))?;
    let profiles = document
        .get("profiles")
        .and_then(|value| value.as_table())
        .ok_or_else(|| {
            invalid(&format!(
                "orchestration profile '{profile}' is not installed"
            ))
        })?;
    let table = profiles
        .get(profile)
        .and_then(|value| value.as_table())
        .ok_or_else(|| {
            invalid(&format!(
                "orchestration profile '{profile}' is not installed"
            ))
        })?;
    Ok(ProfileBinding {
        profile: profile.to_owned(),
        model: toml_string(table, "model"),
        model_provider: toml_string(table, "model_provider"),
        reasoning_effort: toml_string(table, "model_reasoning_effort"),
    })
}

/// Dotted `-c` overrides that layer one orchestration profile onto a native
/// process with no `--profile` flag, such as the app-server. The profile is
/// resolved exactly like [`binding`]: the installed profile file, otherwise
/// its `profiles.<name>` table in the base configuration; `default` adds
/// nothing because the base configuration already loads. Leaf values keep
/// their TOML form so the receiver parses them as the profile file would.
/// Keys that a dotted path cannot name are skipped: they cannot travel this
/// route, and the kit establishes their effects (for example slot trust)
/// through its own owners. A profile that is not installed adds nothing: the
/// dispatch path validates installation when it resolves the binding, and a
/// hand-built receipt without one keeps working; unreadable or invalid
/// configuration still fails instead of silently changing routing.
pub fn profile_config_overrides(codex_home: &Path, profile: &str) -> io::Result<Vec<String>> {
    if profile == "default" {
        return Ok(Vec::new());
    }
    let file = codex_home.join(format!("{profile}.config.toml"));
    let table = if file.is_file() {
        read_toml(&file)?
    } else {
        let base_path = codex_home.join("config.toml");
        if !base_path.is_file() {
            return Ok(Vec::new());
        }
        let base = read_toml(&base_path)?;
        base.get("profiles")
            .and_then(toml::Value::as_table)
            .and_then(|profiles| profiles.get(profile))
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default()
    };
    let mut overrides = Vec::new();
    flatten_table("", &table, &mut overrides);
    Ok(overrides)
}

fn flatten_table(prefix: &str, table: &toml::Table, overrides: &mut Vec<String>) {
    for (key, value) in table {
        if !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            continue;
        }
        let dotted = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => flatten_table(&dotted, inner, overrides),
            leaf => overrides.push(format!("{dotted}={leaf}")),
        }
    }
}

pub fn check_installation(source_root: &Path, codex_home: &Path) -> io::Result<()> {
    let path = source_root.join("global/orchestration.toml");
    match fs::read(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(bytes) => validate(&parse(&bytes)?, &installed_profiles(codex_home)?),
    }
}

pub fn parse(bytes: &[u8]) -> io::Result<Orchestration> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| invalid("orchestration configuration is not UTF-8"))?;
    toml::from_str(text).map_err(|_| invalid("orchestration configuration is invalid"))
}

pub fn validate(config: &Orchestration, installed: &BTreeSet<String>) -> io::Result<()> {
    if config.schema != SCHEMA {
        return Err(invalid("orchestration schema is unsupported"));
    }
    require_profile("lead_profile", &config.lead_profile, installed)?;
    require_profile(
        "successor_lead_profile",
        &config.successor_lead_profile,
        installed,
    )?;
    if config.executor_profiles.is_empty() {
        return Err(invalid("orchestration executor_profiles is missing"));
    }
    let mut seen = BTreeSet::new();
    for name in &config.executor_profiles {
        require_profile("executor_profiles", name, installed)?;
        if !seen.insert(name.clone()) {
            return Err(invalid(&format!(
                "orchestration executor profile '{name}' is duplicated"
            )));
        }
    }
    if config.max_concurrent_executors == 0 || config.max_concurrent_executors > MAX_EXECUTORS {
        return Err(invalid(
            "orchestration max_concurrent_executors must be a positive integer at most 32",
        ));
    }
    if config.vote_threshold < 2 || config.vote_threshold > MAX_EXECUTORS {
        return Err(invalid(
            "orchestration vote_threshold must be at least 2 and at most 32",
        ));
    }
    if config.incubator_size_cap == 0 || config.incubator_size_cap > 256 {
        return Err(invalid(
            "orchestration incubator_size_cap must be a positive integer at most 256",
        ));
    }
    if config.feedback_batch_limit == 0 || config.feedback_batch_limit > MAX_EXECUTORS {
        return Err(invalid(
            "orchestration feedback_batch_limit must be a positive integer at most 32",
        ));
    }
    if config.worktree_limit == 0 || config.worktree_limit > 64 {
        return Err(invalid(
            "orchestration worktree_limit must be a positive integer at most 64",
        ));
    }
    Ok(())
}

pub fn successor_choice(config: &Orchestration, codex_home: &Path) -> io::Result<SuccessorChoice> {
    let bound = binding(codex_home, &config.successor_lead_profile)?;
    let model = bound
        .model
        .ok_or_else(|| invalid("orchestration successor model is missing"))?;
    Ok(SuccessorChoice {
        schema: 1,
        profile: config.successor_lead_profile.clone(),
        model,
        title: format!("{} temporary lead", config.successor_lead_profile),
    })
}

pub fn persist_successor(directory: &Path, choice: &SuccessorChoice) -> io::Result<()> {
    fs::write(
        directory.join("successor.json"),
        serde_json::to_vec_pretty(choice)?,
    )
}

fn require_profile(field: &str, name: &str, installed: &BTreeSet<String>) -> io::Result<()> {
    if name.is_empty() {
        return Err(invalid(&format!("orchestration {field} is missing")));
    }
    if !installed.contains(name) {
        return Err(invalid(&format!(
            "orchestration profile '{name}' is not installed"
        )));
    }
    Ok(())
}

fn installed_profiles(codex_home: &Path) -> io::Result<BTreeSet<String>> {
    let mut names = BTreeSet::from(["default".to_owned()]);
    match fs::read(codex_home.join("config.toml")) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
        Ok(bytes) => {
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| invalid("base configuration cannot be inspected"))?;
            let document: toml::Table = toml::from_str(text)
                .map_err(|_| invalid("base configuration cannot be inspected"))?;
            if let Some(profiles) = document.get("profiles").and_then(|value| value.as_table()) {
                names.extend(profiles.keys().cloned());
            }
        }
    }
    match fs::read_dir(codex_home) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(names),
        Err(error) => return Err(error),
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(profile) = name.strip_suffix(".config.toml")
                    && !profile.is_empty()
                {
                    names.insert(profile.to_owned());
                }
            }
        }
    }
    Ok(names)
}

fn read_toml(path: &Path) -> io::Result<toml::Table> {
    let bytes = fs::read(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            invalid("orchestration profile is not installed")
        } else {
            error
        }
    })?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| invalid("orchestration profile cannot be inspected"))?;
    toml::from_str(text).map_err(|_| invalid("orchestration profile cannot be inspected"))
}

fn table_binding(profile: &str, table: &toml::Table) -> io::Result<ProfileBinding> {
    Ok(ProfileBinding {
        profile: profile.to_owned(),
        model: toml_string(table, "model"),
        model_provider: toml_string(table, "model_provider"),
        reasoning_effort: toml_string(table, "model_reasoning_effort"),
    })
}

fn toml_string(table: &toml::Table, key: &str) -> Option<String> {
    table.get(key)?.as_str().map(str::to_owned)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{message}; preserving installation"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn installed(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn kit_defaults_require_installed_profiles_and_positive_limits() {
        let config = parse(include_bytes!("../../../global/orchestration.toml")).unwrap();
        assert_eq!(config.vote_threshold, 3);
        assert_eq!(config.incubator_size_cap, 32);
        assert_eq!(config.feedback_batch_limit, 8);
        validate(&config, &installed(&["default", "xai", "zai"])).unwrap();
        let error = validate(&config, &installed(&["default", "zai"])).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("profile 'xai' is not installed"));
        assert!(!error.to_string().contains("substitut"));
    }

    #[test]
    fn missing_profile_and_invalid_limit_are_explicit() {
        let config = Orchestration {
            schema: 1,
            lead_profile: "missing".into(),
            successor_lead_profile: "xai".into(),
            executor_profiles: vec!["xai".into()],
            max_concurrent_executors: 2,
            vote_threshold: 3,
            incubator_size_cap: 32,
            feedback_batch_limit: 8,
            worktree_limit: 6,
        };
        let error = validate(&config, &installed(&["default", "xai"])).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("profile 'missing' is not installed")
        );

        let config = Orchestration {
            schema: 1,
            lead_profile: "default".into(),
            successor_lead_profile: "xai".into(),
            executor_profiles: vec!["xai".into()],
            max_concurrent_executors: 0,
            vote_threshold: 3,
            incubator_size_cap: 32,
            feedback_batch_limit: 8,
            worktree_limit: 6,
        };
        let error = validate(&config, &installed(&["default", "xai"])).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("max_concurrent_executors must be a positive integer")
        );

        let config = Orchestration {
            schema: 1,
            lead_profile: "default".into(),
            successor_lead_profile: "xai".into(),
            executor_profiles: vec!["xai".into()],
            max_concurrent_executors: 2,
            vote_threshold: 1,
            incubator_size_cap: 32,
            feedback_batch_limit: 8,
            worktree_limit: 6,
        };
        let error = validate(&config, &installed(&["default", "xai"])).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("vote_threshold must be at least 2")
        );
    }

    #[test]
    fn installation_check_reads_config_toml_and_profile_files() {
        let root =
            std::env::temp_dir().join(format!("orchestration-config-{}", std::process::id()));
        let source = root.join("source");
        let home = root.join("home");
        fs::create_dir_all(source.join("global")).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(
            source.join("global/orchestration.toml"),
            include_bytes!("../../../global/orchestration.toml"),
        )
        .unwrap();
        fs::write(home.join("xai.config.toml"), "model = 'grok-4.7'\n").unwrap();
        fs::write(home.join("zai.config.toml"), "model = 'glm-5.3'\n").unwrap();
        check_installation(&source, &home).unwrap();
        fs::remove_file(home.join("xai.config.toml")).unwrap();
        let error = check_installation(&source, &home).unwrap_err();
        assert!(error.to_string().contains("profile 'xai' is not installed"));
        fs::write(
            home.join("config.toml"),
            "[profiles.xai]\nmodel = 'grok-4.7'\n\n[profiles.zai]\nmodel = 'glm-5.3'\n",
        )
        .unwrap();
        check_installation(&source, &home).unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn profile_overrides_layer_profiles_for_processes_without_a_profile_flag() {
        let root = std::env::temp_dir().join(format!(
            "orchestration-overrides-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("ds.config.toml"),
            concat!(
                "model = \"deepseek-flash\"\n",
                "model_reasoning_effort = \"max\"\n",
                "[features]\n",
                "hooks = true\n",
                "[model_providers.deepseek]\n",
                "base_url = \"https://api.example.test/v1\"\n",
                "wire_api = \"responses\"\n",
                "[projects.'d:/unsafe/key']\n",
                "trust_level = \"trusted\"\n",
            ),
        )
        .unwrap();
        let overrides = profile_config_overrides(&root, "ds").unwrap();
        assert!(overrides.contains(&"model=\"deepseek-flash\"".to_owned()));
        assert!(overrides.contains(&"model_reasoning_effort=\"max\"".to_owned()));
        assert!(overrides.contains(&"features.hooks=true".to_owned()));
        assert!(overrides.contains(
            &"model_providers.deepseek.base_url=\"https://api.example.test/v1\"".to_owned()
        ));
        assert!(overrides.contains(&"model_providers.deepseek.wire_api=\"responses\"".to_owned()));
        assert!(
            overrides
                .iter()
                .all(|entry| !entry.starts_with("projects.")),
            "keys a dotted path cannot name must not travel as overrides: {overrides:?}"
        );
        assert!(
            profile_config_overrides(&root, "default")
                .unwrap()
                .is_empty()
        );

        fs::remove_file(root.join("ds.config.toml")).unwrap();
        fs::write(
            root.join("config.toml"),
            "[profiles.ds]\nmodel = 'deepseek-flash'\n[profiles.ds.model_providers.deepseek]\nbase_url = 'https://api.example.test/v1'\n",
        )
        .unwrap();
        let table = profile_config_overrides(&root, "ds").unwrap();
        assert!(table.contains(&"model=\"deepseek-flash\"".to_owned()));
        assert!(table.contains(
            &"model_providers.deepseek.base_url=\"https://api.example.test/v1\"".to_owned()
        ));
        assert!(
            profile_config_overrides(&root, "missing")
                .unwrap()
                .is_empty()
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn configured_executor_uses_exact_profile_args() {
        let config = load(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .as_path(),
        )
        .unwrap();
        let profile = executor_profile(&config, None).unwrap();
        assert_eq!(profile, "xai");
        assert_eq!(profile_args(&profile).unwrap(), ["--profile", "xai"]);
        assert!(profile_args("default").unwrap().is_empty());
        assert_eq!(
            executor_session_args(&profile).unwrap(),
            ["--profile", "xai", "-c", "agents.enabled=false"]
        );
        assert_eq!(
            executor_session_args("default").unwrap(),
            ["-c", "agents.enabled=false"]
        );
        let error = executor_profile(&config, Some("gpt")).unwrap_err();
        assert!(error.to_string().contains("is not an executor"));
        assert!(!error.to_string().contains("substitut"));
    }

    #[test]
    fn binding_reads_profile_file_without_substitution() {
        let root =
            std::env::temp_dir().join(format!("orchestration-binding-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("xai.config.toml"),
            "model_provider = 'xai'\nmodel = 'grok-4.6'\nmodel_reasoning_effort = 'xhigh'\n",
        )
        .unwrap();
        let bound = binding(&root, "xai").unwrap();
        assert_eq!(bound.profile, "xai");
        assert_eq!(bound.model.as_deref(), Some("grok-4.6"));
        assert_eq!(bound.model_provider.as_deref(), Some("xai"));
        assert_eq!(bound.reasoning_effort.as_deref(), Some("xhigh"));
        let error = binding(&root, "zai").unwrap_err();
        assert!(error.to_string().contains("is not installed"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn successor_choice_uses_configured_profile_model() {
        let config = parse(include_bytes!("../../../global/orchestration.toml")).unwrap();
        assert_eq!(config.successor_lead_profile, "zai");
        let root =
            std::env::temp_dir().join(format!("orchestration-successor-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("zai.config.toml"),
            "model = 'glm-5.3'\nmodel_provider = 'zai'\n",
        )
        .unwrap();
        let choice = successor_choice(&config, &root).unwrap();
        assert_eq!(choice.profile, "zai");
        assert_eq!(choice.model, "glm-5.3");
        persist_successor(&root, &choice).unwrap();
        let saved: SuccessorChoice =
            serde_json::from_slice(&fs::read(root.join("successor.json")).unwrap()).unwrap();
        assert_eq!(saved.model, "glm-5.3");
        let _ = fs::remove_dir_all(&root);
    }
}
