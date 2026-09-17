//! Shared Serena route resolution and worker configuration identity.
//!
//! Ports the seam's route parsing, registered-project resolution and
//! configuration key: every native CLI option keeps its order, project
//! selection is removed from the forwarded arguments, a stdio proxy never
//! silently switches transport, and the worker identity covers the route plus
//! every configuration file that can change tool exposure. YAML reading is a
//! strict bounded subset; anything richer fails loudly instead of guessing.
#![cfg(windows)]

use crate::dependency_package::resolved;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

const MAX_CONFIG: u64 = 1024 * 1024;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// A bounded mapping of scalar YAML values, sufficient for the Serena
/// configuration shapes this owner reads. Richer syntax is rejected.
#[derive(Debug, Default)]
pub struct YamlMapping {
    entries: Vec<(String, YamlValue)>,
}

#[derive(Debug)]
pub enum YamlValue {
    Scalar(String),
    Sequence(Vec<String>),
}

impl YamlMapping {
    pub fn scalar(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, value)| match value {
                YamlValue::Scalar(text) => Some(text.as_str()),
                YamlValue::Sequence(_) => None,
            })
    }

    pub fn sequence(&self, name: &str) -> Option<&[String]> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, value)| match value {
                YamlValue::Sequence(items) => Some(items.as_slice()),
                YamlValue::Scalar(_) => None,
            })
    }
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

fn parse_flow_sequence(value: &str) -> io::Result<Vec<String>> {
    let trimmed = value.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Err(invalid("unsupported YAML flow value"));
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .map(|item| {
            let item = unquote(item);
            if item.contains(['[', ']', '{', '}']) {
                return Err(invalid("nested YAML flow syntax is unsupported"));
            }
            Ok(item)
        })
        .collect()
}

/// Parse the supported YAML subset: top-level scalar mappings, block and flow
/// scalar sequences and comments. Documents that need anything else fail with
/// an explicit error instead of a partial view.
pub fn read_yaml_mapping(path: &Path) -> io::Result<YamlMapping> {
    if !path.is_file() {
        return Ok(YamlMapping::default());
    }
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_CONFIG {
        return Err(invalid("Serena configuration exceeds the size bound"));
    }
    let text = read_config_text(path)?;
    let mut entries: Vec<(String, YamlValue)> = Vec::new();
    let mut pending_key: Option<String> = None;
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let indented = line.starts_with([' ', '\t']);
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        if stripped.starts_with("- ") || stripped == "-" {
            let key = pending_key
                .clone()
                .ok_or_else(|| invalid("sequence entries require a mapping key in this subset"))?;
            let item = unquote(stripped.trim_start_matches('-').trim());
            if item.contains(": ") || item.ends_with(':') {
                return Err(invalid("structured YAML sequence items are unsupported"));
            }
            match entries.iter_mut().find(|(name, _)| name == &key) {
                Some((_, YamlValue::Sequence(items))) => items.push(item),
                Some(_) => return Err(invalid("conflicting YAML value shapes")),
                None => entries.push((key, YamlValue::Sequence(vec![item]))),
            }
            continue;
        }
        if indented {
            // Nested mappings (settings blocks such as `ls_specific_settings`)
            // are not part of a routing decision; they stay Serena's own
            // concern and are skipped instead of failing the whole route.
            continue;
        }
        let Some((key, value)) = split_yaml_entry(stripped) else {
            return Err(invalid("unsupported YAML line"));
        };
        if value.trim().is_empty() {
            pending_key = Some(key.clone());
            if !entries.iter().any(|(name, _)| name == &key) {
                entries.push((key, YamlValue::Sequence(Vec::new())));
            }
            continue;
        }
        pending_key = None;
        if value.trim_start().starts_with('[') {
            entries.push((key, YamlValue::Sequence(parse_flow_sequence(value)?)));
        } else {
            entries.push((key, YamlValue::Scalar(unquote(value))));
        }
    }
    Ok(YamlMapping { entries })
}

fn split_yaml_entry(line: &str) -> Option<(String, &str)> {
    let mut quote: Option<char> = None;
    for (index, character) in line.char_indices() {
        match quote {
            Some(open) if character == open => quote = None,
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character == ':' => {
                let after = line[index + 1..].chars().next();
                if after.is_none_or(|next| next.is_whitespace()) {
                    return Some((unquote(&line[..index]), &line[index + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn read_config_text(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8(strip_bom(&bytes).into_owned())
        .map_err(|_| invalid("Serena configuration is not UTF-8"))?;
    Ok(text)
}

fn strip_bom(bytes: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]) {
        Some(rest) => std::borrow::Cow::Borrowed(rest),
        None => std::borrow::Cow::Owned(bytes.to_vec()),
    }
}

/// The shared Serena resource policy from the checkout's tool resources.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub max_projects: usize,
    pub idle_seconds: u64,
}

pub fn policy(source: &Path) -> io::Result<Policy> {
    let path = source.join("global/tool-resources.json");
    let bytes = fs::read(&path).map_err(|_| invalid("tool resources are unavailable"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| invalid("tool resources are invalid"))?;
    let serena = &value["serena"];
    let max_projects = serena["max_projects"].as_u64().unwrap_or(0) as usize;
    let idle_seconds = serena["idle_seconds"].as_u64().unwrap_or(0);
    if !(1..=16).contains(&max_projects) || !(1..=3600).contains(&idle_seconds) {
        return Err(invalid("invalid Serena resource policy"));
    }
    Ok(Policy {
        max_projects,
        idle_seconds,
    })
}

fn project_folder(root: &Path, config: &YamlMapping) -> PathBuf {
    let template = config
        .scalar("project_serena_folder_location")
        .filter(|template| !template.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "$projectDir/.serena".into());
    let name = root.file_name().map(OsString::from).unwrap_or_default();
    let substituted = template
        .replace("$projectDir", &root.to_string_lossy())
        .replace("$projectFolderName", &name.to_string_lossy());
    let configured =
        resolved(Path::new(&substituted)).unwrap_or_else(|_| PathBuf::from(substituted));
    let default = root.join(".serena");
    if configured.is_dir() || !default.is_dir() {
        configured
    } else {
        default
    }
}

/// Resolve a project selection exactly as the seam does: registered names
/// first (requires an unambiguous match), then paths relative to the caller.
pub fn canonical_project(
    value: &str,
    cwd: &Path,
    home: &Path,
    known: &[String],
    removed: &[String],
) -> io::Result<PathBuf> {
    let config = read_yaml_mapping(&home.join("serena_config.yml"))?;
    let mut names = Vec::new();
    let mut candidates: Vec<String> = Vec::new();
    if let Some(projects) = config.sequence("projects") {
        candidates.extend(projects.iter().map(String::as_str).map(str::to_owned));
    }
    candidates.extend(known.iter().cloned());
    for registered in candidates {
        let root = resolved(Path::new(&registered)).unwrap_or_else(|_| PathBuf::from(&registered));
        if removed.iter().any(|item| item == &registered) || !root.is_dir() {
            continue;
        }
        let folder = project_folder(&root, &config);
        let project = read_yaml_mapping(&folder.join("project.yml"))?;
        let name = project
            .scalar("project_name")
            .map(str::to_owned)
            .unwrap_or_else(|| {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        if name == value {
            names.push(root);
        }
    }
    if names.len() > 1 {
        return Err(io::Error::other(format!(
            "Multiple Serena projects named {value:?}; use an absolute project path"
        )));
    }
    let root = if let Some(root) = names.pop() {
        root
    } else {
        let expanded = if let Some(rest) = value.strip_prefix("~/") {
            home_dir()
                .unwrap_or_else(|| PathBuf::from(value))
                .join(rest)
        } else {
            PathBuf::from(value)
        };
        let joined = if expanded.is_absolute() {
            expanded
        } else {
            cwd.join(expanded)
        };
        resolved(&joined).unwrap_or(joined)
    };
    let root = fs::canonicalize(&root).unwrap_or(root);
    let root = resolved(&root).unwrap_or(root);
    if !root.is_dir() {
        return Err(io::Error::other(format!(
            "No registered Serena project or directory: {value}"
        )));
    }
    Ok(root)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// A resolved client route. The forwarded arguments never carry project
/// selection; the worker command appends the resolved project explicitly.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub project: Option<PathBuf>,
    pub cwd: PathBuf,
    pub arguments: Vec<OsString>,
    pub removed_projects: Vec<String>,
    pub mutation_owner: Option<String>,
}

impl Route {
    /// Reconstruct a client route from the broker's route echo. Unknown or
    /// malformed shapes are refused instead of guessed.
    pub fn from_json(value: &Value) -> io::Result<Self> {
        let field = |name: &str| {
            value
                .get(name)
                .ok_or_else(|| invalid("Serena route echo is incomplete"))
        };
        let project = match field("project")? {
            Value::Null => None,
            project => Some(PathBuf::from(
                project
                    .as_str()
                    .ok_or_else(|| invalid("Serena route project is invalid"))?,
            )),
        };
        let cwd = PathBuf::from(
            field("cwd")?
                .as_str()
                .ok_or_else(|| invalid("Serena route cwd is invalid"))?,
        );
        let mut arguments = Vec::new();
        for argument in field("arguments")?
            .as_array()
            .ok_or_else(|| invalid("Serena route arguments are invalid"))?
        {
            arguments.push(std::ffi::OsString::from(
                argument
                    .as_str()
                    .ok_or_else(|| invalid("Serena route argument is invalid"))?,
            ));
        }
        let mut removed_projects = Vec::new();
        for name in field("removed_projects")?.as_array().unwrap_or(&Vec::new()) {
            removed_projects.push(
                name.as_str()
                    .ok_or_else(|| invalid("Serena removed project is invalid"))?
                    .to_owned(),
            );
        }
        let mutation_owner = match field("mutation_owner")? {
            Value::Null => None,
            owner => Some(
                owner
                    .as_str()
                    .ok_or_else(|| invalid("Serena mutation owner is invalid"))?
                    .to_owned(),
            ),
        };
        Ok(Self {
            project,
            cwd,
            arguments,
            removed_projects,
            mutation_owner,
        })
    }

    /// The route echo consumed by the stdio proxy.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "project": self.project,
            "cwd": self.cwd,
            "arguments": self.arguments.iter().map(|value| value.to_string_lossy()).collect::<Vec<_>>(),
            "removed_projects": self.removed_projects,
            "mutation_owner": self.mutation_owner,
        })
    }
}

const VALUE_OPTIONS: [&str; 13] = [
    "--context",
    "--mode",
    "--add-mode",
    "--language-backend",
    "--transport",
    "--host",
    "--port",
    "--enable-web-dashboard",
    "--enable-gui-log-window",
    "--open-web-dashboard",
    "--log-level",
    "--trace-lsp-communication",
    "--tool-timeout",
];

pub fn parse_route(arguments: &[OsString], cwd: &Path, home: &Path) -> io::Result<Route> {
    if arguments.first().and_then(|value| value.to_str()) != Some("start-mcp-server") {
        return Err(invalid(
            "Shared Serena supports the start-mcp-server entry point",
        ));
    }
    let mut forwarded: Vec<OsString> = vec!["start-mcp-server".into()];
    let mut explicit: Option<OsString> = None;
    let mut positional: Option<OsString> = None;
    let mut from_cwd = false;
    let mut index = 1;
    while index < arguments.len() {
        let item = arguments[index].to_string_lossy().into_owned();
        if item == "--project-from-cwd" {
            from_cwd = true;
        } else if item == "--project" || item == "--project-file" {
            index += 1;
            let value = arguments
                .get(index)
                .ok_or_else(|| invalid("Missing Serena project option value"))?;
            explicit = Some(value.clone());
        } else if item.starts_with("--project=") || item.starts_with("--project-file=") {
            let (_, value) = item.split_once('=').expect("checked prefix");
            explicit = Some(OsString::from(value));
        } else if VALUE_OPTIONS.contains(&item.as_str()) {
            index += 1;
            let value = arguments
                .get(index)
                .ok_or_else(|| invalid("Missing Serena option value"))?;
            let mut value = value.to_string_lossy().into_owned();
            if ["--context", "--mode", "--add-mode"].contains(&item.as_str()) {
                let candidate = cwd.join(&value);
                if candidate.is_file() {
                    let canonical = fs::canonicalize(&candidate).unwrap_or(candidate);
                    value = resolved(&canonical)
                        .unwrap_or(canonical)
                        .to_string_lossy()
                        .into_owned();
                }
            }
            forwarded.push(item.into());
            forwarded.push(value.into());
        } else if !item.starts_with('-') {
            if positional.is_some() {
                return Err(invalid("Serena accepts one positional project"));
            }
            positional = Some(arguments[index].clone());
        } else if let Some((option, value)) = ["--context=", "--mode=", "--add-mode="]
            .iter()
            .find_map(|prefix| item.strip_prefix(prefix).map(|rest| (*prefix, rest)))
        {
            let candidate = cwd.join(value);
            if candidate.is_file() {
                let canonical = fs::canonicalize(&candidate).unwrap_or(candidate);
                let resolved_value = resolved(&canonical).unwrap_or(canonical);
                forwarded.push(format!("{option}{}", resolved_value.to_string_lossy()).into());
            } else {
                forwarded.push(item.clone().into());
            }
        } else {
            forwarded.push(item.clone().into());
        }
        index += 1;
    }
    let explicit = positional.or(explicit);
    if explicit.is_some() && from_cwd {
        return Err(invalid(
            "--project-from-cwd cannot be combined with --project",
        ));
    }
    let mut project = None;
    if let Some(explicit) = explicit {
        project = Some(canonical_project(
            &explicit.to_string_lossy(),
            cwd,
            home,
            &[],
            &[],
        )?);
    } else if from_cwd {
        let start = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let start = resolved(&start).unwrap_or(start);
        project = start
            .ancestors()
            .find(|path| path.join(".serena/project.yml").is_file() || path.join(".git").exists())
            .map(Path::to_path_buf);
    }
    // A stdio proxy cannot silently switch to a network or native UI transport.
    for (index, item) in forwarded.iter().enumerate() {
        let item = item.to_string_lossy();
        if item == "--transport"
            && (index + 1 == forwarded.len() || forwarded[index + 1].to_string_lossy() != "stdio")
        {
            return Err(invalid("Shared Serena requires stdio transport"));
        }
        if item.starts_with("--transport=") && item != "--transport=stdio" {
            return Err(invalid("Shared Serena requires stdio transport"));
        }
    }
    Ok(Route {
        project,
        cwd: fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf()),
        arguments: forwarded,
        removed_projects: Vec::new(),
        mutation_owner: None,
    })
}

/// The worker configuration identity: the route (project normalized, cwd only
/// when no project anchors the worker), the protocol version and the bytes of
/// every configuration file that can change behavior or tool exposure.
pub fn configuration_key(route: &Route, initialize: &Value, home: &Path) -> io::Result<String> {
    let global_config = home.join("serena_config.yml");
    let config = read_yaml_mapping(&global_config)?;
    let mut files = vec![global_config.clone()];
    if let Some(root) = &route.project {
        let folder = project_folder(root, &config);
        files.push(folder.join("project.yml"));
        files.push(folder.join("project.local.yml"));
        for name in [
            "tsconfig.json",
            "jsconfig.json",
            "pyrightconfig.json",
            "pyproject.toml",
            "package.json",
            "Cargo.toml",
            "go.mod",
            "global.json",
        ] {
            files.push(root.join(name));
        }
    }
    for name in ["contexts", "modes", "prompt_templates"] {
        collect_yml_files(&home.join(name), &mut files)?;
    }
    for value in &route.arguments {
        let path = Path::new(value);
        if path.is_file() {
            files.push(resolved(path)?);
        }
    }
    let normcase = |path: &Path| path.to_string_lossy().to_lowercase();
    let identity = serde_json::json!({
        "arguments": route.arguments.iter().map(|value| value.to_string_lossy()).collect::<Vec<_>>(),
        "cwd": route.project.is_none().then(|| normcase(&route.cwd)),
        "project": route.project.as_ref().map(|path| normcase(path)),
        "removed_projects": route.removed_projects,
        "mutation_owner": route.mutation_owner,
        "protocol": initialize.get("protocolVersion"),
    });
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(&identity)?);
    for path in &files {
        digest.update(path.to_string_lossy().as_bytes());
        match fs::read(path) {
            Ok(bytes) => digest.update(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => digest.update(b"<absent>"),
            Err(error) => return Err(error),
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn collect_yml_files(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();
    for entry in entries {
        let path = entry?.path();
        if path.is_file()
            && path.extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
            })
        {
            found.insert(resolved(&path)?);
        }
    }
    files.extend(found);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("harness-serena-route-{tag}-"))
            .tempdir()
            .unwrap()
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn managed_arguments_resolve_the_project_from_cwd() {
        let root = temp("cwd");
        let project = root.path().join("repo");
        fs::create_dir_all(project.join(".git")).unwrap();
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let route = parse_route(
            &args(&[
                "start-mcp-server",
                "--context",
                "codex",
                "--project-from-cwd",
                "--enable-web-dashboard",
                "false",
            ]),
            &project,
            &home,
        )
        .unwrap();
        assert_eq!(route.project.as_deref(), Some(project.as_path()));
        assert_eq!(
            route.arguments,
            args(&[
                "start-mcp-server",
                "--context",
                "codex",
                "--enable-web-dashboard",
                "false"
            ])
        );
    }

    #[test]
    fn project_options_are_removed_from_forwarded_arguments() {
        let root = temp("explicit");
        let project = root.path().join("repo");
        fs::create_dir_all(&project).unwrap();
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let route = parse_route(
            &args(&["start-mcp-server", "--project", "repo", "--mode", "codex"]),
            root.path(),
            &home,
        )
        .unwrap();
        assert_eq!(route.project.as_deref(), Some(project.as_path()));
        assert_eq!(
            route.arguments,
            args(&["start-mcp-server", "--mode", "codex"])
        );

        let error = parse_route(
            &args(&[
                "start-mcp-server",
                "--project",
                "repo",
                "--project-from-cwd",
            ]),
            root.path(),
            &home,
        )
        .unwrap_err();
        assert!(error.to_string().contains("cannot be combined"));
    }

    #[test]
    fn positional_project_and_missing_values_are_rejected() {
        let root = temp("positional");
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let error = parse_route(
            &args(&["start-mcp-server", "one", "two"]),
            root.path(),
            &home,
        )
        .unwrap_err();
        assert!(error.to_string().contains("one positional project"));

        let error = parse_route(
            &args(&["start-mcp-server", "--context"]),
            root.path(),
            &home,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Missing Serena option value"));

        let error = parse_route(&args(&["run"]), root.path(), &home).unwrap_err();
        assert!(error.to_string().contains("start-mcp-server"));
    }

    #[test]
    fn only_stdio_transport_is_accepted() {
        let root = temp("transport");
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        parse_route(
            &args(&["start-mcp-server", "--transport", "stdio"]),
            root.path(),
            &home,
        )
        .unwrap();
        for rejected in [
            vec!["start-mcp-server", "--transport", "sse"],
            vec!["start-mcp-server", "--transport=sse"],
        ] {
            let error = parse_route(&args(&rejected), root.path(), &home).unwrap_err();
            assert!(error.to_string().contains("stdio transport"));
        }
    }

    #[test]
    fn context_mode_file_values_resolve_to_absolute_paths() {
        let root = temp("context-file");
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let context = root.path().join("codex.yml");
        fs::write(&context, "tools: []\n").unwrap();
        let route = parse_route(
            &args(&["start-mcp-server", "--context", "codex.yml"]),
            root.path(),
            &home,
        )
        .unwrap();
        assert_eq!(
            route.arguments,
            args(&[
                "start-mcp-server",
                "--context",
                context.to_string_lossy().as_ref()
            ])
        );

        let route = parse_route(
            &args(&["start-mcp-server", "--mode=codex.yml"]),
            root.path(),
            &home,
        )
        .unwrap();
        assert_eq!(
            route.arguments,
            args(&[
                "start-mcp-server",
                &format!("--mode={}", context.to_string_lossy())
            ])
        );
    }

    fn registered_home(root: &Path, projects: &[&str]) -> PathBuf {
        let home = root.join("serena-home");
        fs::create_dir_all(&home).unwrap();
        let listing = projects
            .iter()
            .map(|project| format!("  - {project}\n"))
            .collect::<String>();
        fs::write(
            home.join("serena_config.yml"),
            format!("projects:\n{listing}"),
        )
        .unwrap();
        home
    }

    #[test]
    fn registered_names_resolve_and_ambiguity_is_rejected() {
        let root = temp("names");
        let first = root.path().join("first");
        let second = root.path().join("second");
        for (path, name) in [(&first, "alpha"), (&second, "beta")] {
            fs::create_dir_all(path.join(".serena")).unwrap();
            fs::write(
                path.join(".serena/project.yml"),
                format!("project_name: {name}\n"),
            )
            .unwrap();
        }
        let home = registered_home(
            root.path(),
            &[&first.to_string_lossy(), &second.to_string_lossy()],
        );
        assert_eq!(
            canonical_project("alpha", root.path(), &home, &[], &[]).unwrap(),
            first
        );
        assert_eq!(
            canonical_project("beta", root.path(), &home, &[], &[]).unwrap(),
            second
        );
        // A path selection bypasses the registry.
        assert_eq!(
            canonical_project(&second.to_string_lossy(), root.path(), &home, &[], &[]).unwrap(),
            second
        );
        // A removed registration no longer resolves by name.
        let error = canonical_project(
            "beta",
            root.path(),
            &home,
            &[],
            &[second.to_string_lossy().into_owned()],
        )
        .unwrap_err();
        assert!(error.to_string().contains("No registered Serena project"));

        fs::create_dir_all(root.path().join("third/.serena")).unwrap();
        fs::write(
            root.path().join("third/.serena/project.yml"),
            "project_name: alpha\n",
        )
        .unwrap();
        let home = registered_home(
            root.path(),
            &[
                &first.to_string_lossy(),
                &root.path().join("third").to_string_lossy(),
            ],
        );
        let error = canonical_project("alpha", root.path(), &home, &[], &[]).unwrap_err();
        assert!(error.to_string().contains("Multiple Serena projects"));
    }

    #[test]
    fn configuration_key_tracks_configuration_and_normalizes_the_project() {
        let root = temp("key");
        let project = root.path().join("repo");
        fs::create_dir_all(project.join(".serena")).unwrap();
        fs::write(project.join(".serena/project.yml"), "language: python\n").unwrap();
        let home = root.path().join("home");
        fs::create_dir_all(home.join("contexts")).unwrap();
        fs::write(home.join("serena_config.yml"), "projects: []\n").unwrap();
        fs::write(home.join("contexts/codex.yml"), "tools: []\n").unwrap();
        let route = parse_route(
            &args(&[
                "start-mcp-server",
                "--project-from-cwd",
                "--context",
                "codex",
            ]),
            &project,
            &home,
        )
        .unwrap();
        let initialize = json!({"protocolVersion": "2024-11-05"});
        let first = configuration_key(&route, &initialize, &home).unwrap();
        let second = configuration_key(&route, &initialize, &home).unwrap();
        assert_eq!(first, second);

        // Case differences in the selected project path keep one worker.
        let cased = parse_route(
            &args(&[
                "start-mcp-server",
                "--project",
                &project.to_string_lossy().to_uppercase(),
                "--context",
                "codex",
            ]),
            &project,
            &home,
        )
        .unwrap();
        assert_eq!(
            configuration_key(&cased, &initialize, &home).unwrap(),
            first
        );

        // Configuration bytes participate in the identity.
        fs::write(home.join("contexts/codex.yml"), "tools:\n  - find_symbol\n").unwrap();
        let changed = configuration_key(&route, &initialize, &home).unwrap();
        assert_ne!(changed, first);

        // The protocol version participates.
        let other =
            configuration_key(&route, &json!({"protocolVersion": "2025-01-01"}), &home).unwrap();
        assert_ne!(other, changed);

        // Without a project the cwd anchors the worker identity.
        let floating = Route {
            project: None,
            cwd: project.clone(),
            arguments: route.arguments.clone(),
            removed_projects: Vec::new(),
            mutation_owner: None,
        };
        assert_ne!(
            configuration_key(&floating, &initialize, &home).unwrap(),
            changed
        );
    }

    #[test]
    fn yaml_subset_reader_supports_the_serena_shapes_and_rejects_the_rest() {
        let root = temp("yaml");
        let path = root.path().join("config.yml");
        fs::write(
            &path,
            "\u{feff}# comment\nprojects:\n  - \"C:/one\"\n  - C:/two\nflag: yes\nflow: [a, b]\n",
        )
        .unwrap();
        let mapping = read_yaml_mapping(&path).unwrap();
        assert_eq!(
            mapping.sequence("projects").unwrap(),
            &["C:/one".to_owned(), "C:/two".to_owned()]
        );
        assert_eq!(mapping.scalar("flag"), Some("yes"));
        assert_eq!(
            mapping.sequence("flow").unwrap(),
            &["a".to_owned(), "b".to_owned()]
        );
        assert_eq!(
            read_yaml_mapping(&root.path().join("absent.yml"))
                .unwrap()
                .scalar("x"),
            None
        );

        // Nested settings blocks (for example `ls_specific_settings`) are not
        // part of a routing decision and are skipped, not rejected.
        fs::write(
            &path,
            "projects:\n  - C:/one\nnested:\n  key: value\n  other:\n    - item\nflag: yes\n",
        )
        .unwrap();
        let mapping = read_yaml_mapping(&path).unwrap();
        assert_eq!(
            mapping.sequence("projects").unwrap(),
            &["C:/one".to_owned()]
        );
        assert_eq!(mapping.scalar("flag"), Some("yes"));
    }

    #[test]
    fn policy_reads_and_validates_the_shared_resources() {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shared = policy(&checkout).unwrap();
        assert_eq!(shared.max_projects, 3);
        assert_eq!(shared.idle_seconds, 300);

        let root = temp("policy");
        fs::create_dir_all(root.path().join("global")).unwrap();
        fs::write(
            root.path().join("global/tool-resources.json"),
            serde_json::to_vec(&json!({"serena": {"max_projects": 0, "idle_seconds": 300}}))
                .unwrap(),
        )
        .unwrap();
        assert!(policy(root.path()).is_err());
    }
}
