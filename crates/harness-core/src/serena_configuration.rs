//! Harness-owned Serena home and generated configuration.
//!
//! Every adopted language backend is pinned with an explicit launch command, so
//! a worker never provisions or updates a package, and Serena's own
//! configuration writes stay inside the owned home instead of a shared user
//! file. A project whose detected language has no verified adopted backend is
//! refused before a worker starts, matching the retired compatibility seam.
#![cfg(windows)]

use crate::{dependency_discovery::local_path, registration_native::StagedFile};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

pub const CONFIG_NAME: &str = "serena_config.yml";
const SCAN_ENTRIES: usize = 4000;
const SCAN_DEPTH: usize = 6;
const ADOPTED: &[&str] = &["adopted", "present", "installed", "ready", "verified"];

fn unavailable(detail: &str) -> io::Error {
    io::Error::other(format!(
        "Harness Serena: {detail}; installation and updates are disabled during MCP sessions"
    ))
}

fn adopted(record: &Value) -> bool {
    record["status"]
        .as_str()
        .is_some_and(|status| ADOPTED.contains(&status))
}

fn required_file(value: Option<&str>, label: &str) -> io::Result<PathBuf> {
    let Some(text) = value else {
        return Err(unavailable(&format!("missing adopted {label}")));
    };
    let resolved = local_path(Path::new(text))?;
    if !resolved.is_file() {
        return Err(unavailable(&format!("missing adopted {label}")));
    }
    Ok(resolved)
}

/// Explicit launch commands for every adopted backend, keyed by Serena's
/// language-server id. The registry's `serena_id` is the single mapping owner.
pub fn ls_specific_settings(registry: &Value) -> io::Result<BTreeMap<String, Value>> {
    let mut settings = BTreeMap::new();
    let Some(languages) = registry["languages"].as_array() else {
        return Ok(settings);
    };
    for record in languages {
        let Some(serena_id) = record["serena_id"].as_str() else {
            continue;
        };
        if !adopted(record) {
            continue;
        }
        let paths = &record["paths"];
        let (base, args) = match (paths["node"].as_str(), paths["entrypoint"].as_str()) {
            (Some(node), Some(entry)) => (
                vec![
                    required_file(Some(node), "Node runtime")?,
                    required_file(Some(entry), "language server entrypoint")?,
                ],
                vec!["--stdio".to_owned()],
            ),
            _ => {
                let executable = paths["executable"]
                    .as_str()
                    .or_else(|| record["executable"].as_str());
                (
                    vec![required_file(executable, "language server executable")?],
                    Vec::new(),
                )
            }
        };
        settings.insert(serena_id.to_owned(), launch_setting(&base, &args));
        // The retired seam mapped several Serena ids onto one adopted backend
        // (for example both `python` and `python_basedpyright`).
        if let Some((_, aliases)) = ALIASES.iter().find(|(owner, _)| *owner == serena_id) {
            for alias in *aliases {
                settings.insert((*alias).to_owned(), launch_setting(&base, &args));
            }
        }
    }
    Ok(settings)
}

/// Serena ids that select the same adopted backend as their registry owner.
const ALIASES: &[(&str, &[&str])] = &[
    ("python_basedpyright", &["python"]),
    ("delphi", &["pascal"]),
];

fn launch_setting(base: &[PathBuf], args: &[String]) -> Value {
    json!({
        "ls_base_cmd": base
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        "ls_args": args,
    })
}

/// Project markers mapped to Serena's language-server ids. Only project-shaped
/// declarations count: a stray foreign-language sample in an otherwise adopted
/// project must not refuse the whole session. A detected language without an
/// adopted backend is refused instead of letting the tool consult a release API
/// or run a package manager for it.
const MARKERS: &[(&str, Option<&str>)] = &[
    ("Cargo.toml", Some("rust")),
    ("pyproject.toml", Some("python_basedpyright")),
    ("setup.py", Some("python_basedpyright")),
    ("requirements.txt", Some("python_basedpyright")),
    ("tsconfig.json", Some("typescript")),
    ("go.mod", Some("go")),
    ("pom.xml", Some("java")),
    ("build.gradle", Some("java")),
    ("build.gradle.kts", Some("java")),
    ("composer.json", Some("php")),
    ("Gemfile", Some("ruby")),
    ("pubspec.yaml", Some("dart")),
    ("mix.exs", Some("elixir")),
    ("CMakeLists.txt", Some("clangd")),
];

/// Extension-shaped project declarations (for example `*.csproj` or `*.sln`).
const PROJECT_EXTENSIONS: &[(&str, &str)] = &[
    ("csproj", "csharp"),
    ("sln", "csharp"),
    ("dpr", "pascal"),
    ("lpr", "pascal"),
    ("vcxproj", "clangd"),
];

fn skipped(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | "target" | ".venv" | "venv" | "__pycache__" | "dist" | "build"
    )
}

fn collect_languages(
    root: &Path,
    depth: usize,
    remaining: &mut usize,
    found: &mut BTreeMap<String, ()>,
) -> io::Result<()> {
    if depth > SCAN_DEPTH || *remaining == 0 {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        if *remaining == 0 {
            return Ok(());
        }
        *remaining -= 1;
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if skipped(&name) {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_languages(&entry.path(), depth + 1, remaining, found)?;
            continue;
        }
        if let Some((_, language)) = MARKERS.iter().find(|(marker, _)| *marker == name) {
            if let Some(language) = language {
                found.insert((*language).to_owned(), ());
            }
            continue;
        }
        let path = entry.path();
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        let extension = extension.to_ascii_lowercase();
        if let Some((_, language)) = PROJECT_EXTENSIONS
            .iter()
            .find(|(known, _)| *known == extension)
        {
            found.insert((*language).to_owned(), ());
        }
    }
    Ok(())
}

/// Refuse a project whose detected language has no verified adopted backend.
pub fn ensure_supported_languages(registry: &Value, project: &Path) -> io::Result<()> {
    let supported = ls_specific_settings(registry)?;
    let mut remaining = SCAN_ENTRIES;
    let mut found = BTreeMap::new();
    collect_languages(project, 0, &mut remaining, &mut found)?;
    for language in found.keys() {
        if !supported.contains_key(language) {
            return Err(unavailable(&format!(
                "{language} has no verified reusable dependency"
            )));
        }
    }
    Ok(())
}

fn render(settings: &BTreeMap<String, Value>) -> String {
    let mut text = String::from(
        "# Generated by coding-agents-harness; regenerated from the adopted\n\
         # dependency registry. Explicit launch commands keep a Serena worker from\n\
         # provisioning or updating any package during an MCP session.\n\
         projects: []\n\
         gui_log_window: false\n\
         web_dashboard: false\n\
         ls_specific_settings:\n",
    );
    for (id, value) in settings {
        text.push_str(&format!("  {id}:\n    ls_base_cmd:\n"));
        for item in value["ls_base_cmd"].as_array().into_iter().flatten() {
            text.push_str(&format!(
                "      - '{}'\n",
                item.as_str().unwrap_or_default()
            ));
        }
        let args = value["ls_args"].as_array().cloned().unwrap_or_default();
        if args.is_empty() {
            text.push_str("    ls_args: []\n");
        } else {
            text.push_str("    ls_args:\n");
            for item in args {
                text.push_str(&format!(
                    "      - '{}'\n",
                    item.as_str().unwrap_or_default()
                ));
            }
        }
    }
    text
}

/// Materialize the owned Serena home: `<CODEX_HOME>/harness/serena-home` with a
/// generated configuration. Unchanged content is left untouched so the broker's
/// configuration identity stays stable.
pub fn prepare(registry_path: &Path, codex_home: &Path) -> io::Result<PathBuf> {
    prepare_in(registry_path, &codex_home.join("harness/serena-home"))
}

/// A worker-private Serena home. Each worker serves one project, so its own
/// configuration keeps project registration and configuration writes from
/// crossing between concurrently served projects.
pub fn prepare_worker(
    registry_path: &Path,
    codex_home: &Path,
    project: &Path,
) -> io::Result<PathBuf> {
    let key = crate::build_identity::hash_bytes(
        project
            .to_string_lossy()
            .to_lowercase()
            .replace('/', "\\")
            .as_bytes(),
    );
    prepare_in(
        registry_path,
        &codex_home
            .join("harness/serena-home/workers")
            .join(&key[..16]),
    )
}

fn prepare_in(registry_path: &Path, home: &Path) -> io::Result<PathBuf> {
    let registry: Value = serde_json::from_slice(&fs::read(registry_path)?)
        .map_err(|_| io::Error::other("Serena registry is not JSON"))?;
    let settings = ls_specific_settings(&registry)?;
    let home = local_path(home)?;
    fs::create_dir_all(&home)?;
    let config = home.join(CONFIG_NAME);
    let after = render(&settings).into_bytes();
    match fs::read(&config) {
        Ok(current) if current == after => {}
        Ok(_) => {
            let snapshot = crate::config_file::ConfigSnapshot::read(&config)?;
            snapshot.replace(&after)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            StagedFile::create(&config, &after)?.commit()?;
        }
        Err(error) => return Err(error),
    }
    Ok(home)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(root: &Path) -> Value {
        let node = root.join("node.exe");
        let entry = root.join("langserver.js");
        let rust = root.join("rust-analyzer.exe");
        for path in [&node, &entry, &rust] {
            fs::write(path, b"fixture").unwrap();
        }
        json!({"languages": [
            {"id": "python", "serena_id": "python_basedpyright", "status": "adopted",
             "paths": {"node": node, "entrypoint": entry}},
            {"id": "rust", "serena_id": "rust", "status": "adopted",
             "paths": {"executable": rust}},
            {"id": "graphify", "serena_id": "graphify", "status": "missing",
             "paths": {"executable": root.join("absent.exe")}},
        ]})
    }

    #[test]
    fn generated_configuration_pins_every_adopted_backend() {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(root.path());
        let settings = ls_specific_settings(&registry).unwrap();
        assert_eq!(
            settings.keys().cloned().collect::<Vec<_>>(),
            vec![
                "python".to_owned(),
                "python_basedpyright".to_owned(),
                "rust".to_owned()
            ]
        );
        assert_eq!(settings["rust"]["ls_args"], json!([]));
        assert_eq!(
            settings["python_basedpyright"]["ls_args"],
            json!(["--stdio"])
        );
        // The alias selects the same adopted backend.
        assert_eq!(
            settings["python"]["ls_base_cmd"],
            settings["python_basedpyright"]["ls_base_cmd"]
        );
        let rendered = render(&settings);
        assert!(rendered.contains("ls_base_cmd"));
        assert!(rendered.contains("langserver.js"));
        assert!(!rendered.contains("absent.exe"));
    }

    #[test]
    fn prepare_is_idempotent_and_keeps_the_home_inside_codex_home() {
        let root = tempfile::tempdir().unwrap();
        let registry_path = root.path().join("code-tools.json");
        let codex_home = root.path().join("codex");
        fs::create_dir_all(&codex_home).unwrap();
        fs::write(
            &registry_path,
            serde_json::to_vec(&registry(root.path())).unwrap(),
        )
        .unwrap();
        let home = prepare(&registry_path, &codex_home).unwrap();
        assert_eq!(home, codex_home.join("harness/serena-home"));
        let first = fs::read(home.join(CONFIG_NAME)).unwrap();
        let again = prepare(&registry_path, &codex_home).unwrap();
        assert_eq!(again, home);
        assert_eq!(fs::read(home.join(CONFIG_NAME)).unwrap(), first);
    }

    #[test]
    fn unadopted_language_is_refused_before_a_worker_starts() {
        let root = tempfile::tempdir().unwrap();
        let registry = registry(root.path());
        let project = root.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("Cargo.toml"), b"[package]").unwrap();
        ensure_supported_languages(&registry, &project).unwrap();
        // Stray foreign-language source does not refuse an adopted project.
        fs::write(project.join("sample.cs"), b"class Sample {}").unwrap();
        ensure_supported_languages(&registry, &project).unwrap();
        // A foreign project declaration does.
        fs::write(project.join("App.csproj"), b"<Project/>").unwrap();
        let error = ensure_supported_languages(&registry, &project).unwrap_err();
        assert!(error.to_string().contains("csharp"), "{error}");
        assert!(!error.to_string().contains("absent.exe"));
    }

    #[test]
    fn missing_adopted_path_is_refused_without_echoing_it() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = registry(root.path());
        registry["languages"][0]["paths"]["node"] = json!(root.path().join("gone/node.exe"));
        let error = ls_specific_settings(&registry).unwrap_err();
        assert!(error.to_string().contains("missing adopted Node runtime"));
        assert!(!error.to_string().contains("gone"));
    }
}
