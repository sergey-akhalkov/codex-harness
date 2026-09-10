//! Explicit external-dependency observations; never provisions or executes a package.
#![cfg(windows)]

use crate::{dependency_package as package, wheel_record};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs, io,
    path::{Component, Path, PathBuf, Prefix},
};

#[derive(Default)]
pub struct Request {
    pub catalogue: PathBuf,
    pub user_home: PathBuf,
    pub npm_prefixes: Vec<PathBuf>,
    pub uv_tools_dir: Option<PathBuf>,
    pub serena_cache: Option<PathBuf>,
    pub rustup_home: Option<PathBuf>,
    pub path: Option<OsString>,
    pub graphify_manifest: Option<PathBuf>,
    pub nuphus_models: Option<PathBuf>,
    pub full_records: bool,
    pub probe_versions: bool,
    pub processes: bool,
}

pub fn local_path(path: &Path) -> io::Result<PathBuf> {
    let path = std::path::absolute(path)?;
    if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    {
        return Err(package::invalid());
    }
    let resolved = package::resolved(&path)?;
    if !matches!(resolved.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
    {
        return Err(package::invalid());
    }
    Ok(resolved)
}

fn unique(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !found.iter().any(|prior| package::same_path(prior, &path)) {
            found.push(path);
        }
    }
    found
}

fn directories(root: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error),
    };
    let mut found = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index == 4096 {
            return Err(package::invalid());
        }
        let entry = entry?;
        if entry.path().is_dir() {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}

fn distribution_name(name: &str) -> String {
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

fn metadata_headers(text: &str) -> io::Result<(String, String)> {
    let mut name = None;
    let mut version = None;
    for line in text.lines().take_while(|line| !line.is_empty()) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let target = if key.eq_ignore_ascii_case("Name") {
            &mut name
        } else if key.eq_ignore_ascii_case("Version") {
            &mut version
        } else {
            continue;
        };
        if target.is_some()
            || value.len() > 256
            || value.trim().is_empty()
            || value.chars().any(char::is_control)
        {
            return Err(package::invalid());
        }
        *target = Some(value.trim().to_owned());
    }
    Ok((
        name.ok_or_else(package::invalid)?,
        version.ok_or_else(package::invalid)?,
    ))
}

fn console_entry(text: &str, name: &str, module: &str) -> bool {
    let mut in_console = false;
    let mut matches = 0;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_console = line == "[console_scripts]";
        } else if in_console
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == name
        {
            if value.trim() != module {
                return false;
            }
            matches += 1;
        }
    }
    matches == 1
}

fn uv(spec: &Value, tools: &Path, full: bool) -> io::Result<Vec<Value>> {
    let package_name = spec["package"].as_str().ok_or_else(package::invalid)?;
    let root = local_path(&tools.join(package_name))?;
    let site = root.join("Lib/site-packages");
    let mut candidates = Vec::new();
    for dist in directories(&site)? {
        if dist
            .extension()
            .is_none_or(|extension| extension != "dist-info")
        {
            continue;
        }
        let dist = local_path(&dist)?;
        if !package::contained(&dist, &root) {
            return Err(package::invalid());
        }
        let metadata_path = package::package_file(&dist, "METADATA")?;
        let Some(metadata) = package::read_text(&metadata_path)? else {
            continue;
        };
        let (name, version) = metadata_headers(&metadata)?;
        if distribution_name(&name) != distribution_name(package_name) {
            continue;
        }
        if !candidates.is_empty() {
            return Err(package::invalid());
        }
        let serena = spec["id"] == "serena";
        let python = local_path(&root.join("Scripts/python.exe"))?;
        let module = local_path(&site.join(if serena {
            "serena/cli.py"
        } else {
            "graphify/serve.py"
        }))?;
        let entry = local_path(&root.join(if serena {
            "Scripts/serena.exe"
        } else {
            "Scripts/graphify-mcp.exe"
        }))?;
        let entry_points = package::read_text(&package::package_file(&dist, "entry_points.txt")?)?;
        let entry_identity = entry_points.is_some_and(|text| {
            console_entry(
                &text,
                if serena { "serena" } else { "graphify-mcp" },
                if serena {
                    "serena.cli:top_level"
                } else {
                    "graphify.serve:_main"
                },
            )
        });
        let installed = python.is_file()
            && entry.is_file()
            && module.is_file()
            && entry_identity
            && package::contained(&entry, &root)
            && package::contained(&module, &root);
        let package_dirs: &[&str] = if serena {
            &["serena", "solidlsp"]
        } else {
            &["graphify"]
        };
        let integrity = wheel_record::verify_record(&dist, &root, full, package_dirs);
        let receipt = package::package_file(&root, "uv-receipt.toml")?;
        let receipt_identity = package::read_text(&receipt)?
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .and_then(|value| value.get("tool")?.get("requirements")?.as_array().cloned())
            .is_some_and(|requirements| {
                requirements.iter().any(|requirement| {
                    requirement
                        .get("name")
                        .and_then(toml::Value::as_str)
                        .is_some_and(|name| {
                            distribution_name(name) == distribution_name(package_name)
                        })
                })
            });
        let status = if integrity["state"] == "modified" {
            "modified"
        } else if installed {
            "adopted"
        } else {
            "broken"
        };
        let mut evidence = vec![
            json!({"kind":"wheel-record","state":integrity["state"],"checked_files":integrity["checked_files"],"issues":integrity["issues"],"basis":integrity["basis"]}),
        ];
        if installed {
            evidence.push(json!({"kind":"console-entrypoint-fingerprint","path":entry,"sha256":package::fingerprint(&entry)?}));
        }
        candidates.push(json!({"manager":"uv","version":version,"executable":entry,
            "command":if installed {json!([entry])} else {json!([])},
            "paths":{"python":python,"module_root":site,"entrypoint":module,"console_entrypoint":entry},
            "installation_root":root,"status":status,"ownership":if receipt_identity {"adopted-shared"} else {"unconfirmed"},
            "health":{"installed":installed,"identity_verified":true,"integrity":integrity["state"],"callable":null,"checked_operations":[]},
            "update_safe":receipt_identity && integrity["state"] == "record-matches" && installed,
            "provenance":{"source":spec["source"],"metadata":metadata_path,"receipt":receipt,"receipt_identity_verified":receipt_identity,"module":module},"evidence":evidence}));
    }
    Ok(candidates)
}

struct Discovery {
    catalogue: PathBuf,
    home: PathBuf,
    spec: Value,
    paths: Vec<PathBuf>,
    npm_roots: Vec<PathBuf>,
    uv: PathBuf,
    serena: PathBuf,
    rustup: PathBuf,
    node: Option<PathBuf>,
    models: PathBuf,
    graphify_manifest: Option<PathBuf>,
    full: bool,
    probe_versions: bool,
}

impl Discovery {
    fn new(request: &Request, spec: Value) -> io::Result<Self> {
        let catalogue = local_path(&request.catalogue)?;
        if spec["schema_version"] != 1 || spec["runtime_downloads"] != false {
            return Err(package::invalid());
        }
        let mut ids = BTreeSet::new();
        for group in ["mcp", "languages"] {
            for entry in spec[group].as_array().ok_or_else(package::invalid)? {
                let id = entry["id"].as_str().ok_or_else(package::invalid)?;
                let expected = match (group, id) {
                    ("mcp", "serena") => ("serena-agent", "uv"),
                    ("mcp", "graphify") => ("graphifyy", "uv"),
                    ("mcp", "codebase-memory") => ("codebase-memory-mcp", "npm"),
                    ("mcp", "nuphus") => ("@nuphus/nuphus-mcp", "npm"),
                    ("languages", "python") => ("basedpyright", "npm"),
                    ("languages", "rust") => ("rust-analyzer", "rustup"),
                    _ => return Err(package::invalid()),
                };
                if !ids.insert(id)
                    || entry["package"] != expected.0
                    || entry["manager"] != expected.1
                    || package::text(&entry["source"]).is_none()
                {
                    return Err(package::invalid());
                }
            }
        }
        if ids.len() != 6 {
            return Err(package::invalid());
        }
        let home = local_path(&request.user_home)?;
        let mut paths = Vec::new();
        if let Some(path) = &request.path {
            for (index, path) in std::env::split_paths(path).enumerate() {
                if index == 256 {
                    return Err(package::invalid());
                }
                if path.is_absolute() {
                    paths.push(local_path(&path)?);
                }
            }
        }
        let paths = unique(paths);
        let node = paths
            .iter()
            .map(|path| path.join("node.exe"))
            .find(|path| path.is_file())
            .map(|path| local_path(&path))
            .transpose()?;
        if request.npm_prefixes.len() > 256 {
            return Err(package::invalid());
        }
        let mut npm_roots = Vec::new();
        for prefix in request
            .npm_prefixes
            .iter()
            .chain([home.join("AppData/Roaming/npm")].iter())
            .chain(paths.iter())
        {
            let root = local_path(&prefix.join("node_modules"))?;
            if root.is_dir() {
                npm_roots.push(root);
            }
        }
        Ok(Self {
            catalogue,
            spec,
            node,
            paths,
            npm_roots: unique(npm_roots),
            uv: local_path(
                request
                    .uv_tools_dir
                    .as_deref()
                    .unwrap_or(&home.join("AppData/Roaming/uv/tools")),
            )?,
            serena: local_path(
                request
                    .serena_cache
                    .as_deref()
                    .unwrap_or(&home.join(".serena/language_servers/static")),
            )?,
            rustup: local_path(
                request
                    .rustup_home
                    .as_deref()
                    .unwrap_or(&home.join(".rustup")),
            )?,
            models: local_path(
                request
                    .nuphus_models
                    .as_deref()
                    .unwrap_or(&home.join("AppData/Roaming/Nuphus/models")),
            )?,
            graphify_manifest: request
                .graphify_manifest
                .as_deref()
                .map(local_path)
                .transpose()?,
            home,
            full: request.full_records,
            probe_versions: request.probe_versions,
        })
    }

    fn npm(&self, name: &str, command: &str, cache_class: Option<&str>) -> io::Result<Vec<Value>> {
        let mut roots = self
            .npm_roots
            .iter()
            .cloned()
            .map(|path| (path, "npm-global"))
            .collect::<Vec<_>>();
        if let Some(class) = cache_class {
            let cache = self.serena.join(class);
            roots.push((cache.join("node_modules"), "serena-cache"));
            for directory in directories(&cache)? {
                roots.push((directory.join("node_modules"), "serena-cache"));
            }
        }
        let mut candidates = Vec::new();
        for (root, owner) in roots {
            if let Some(candidate) =
                package::npm(name, &root, self.node.as_deref(), Some(command), owner)?
            {
                candidates.push(candidate);
            }
        }
        Ok(candidates)
    }

    fn rust(&self) -> io::Result<Vec<Value>> {
        let settings = package::read_text(&self.rustup.join("settings.toml"))?
            .map(|text| toml::from_str::<toml::Value>(&text).map_err(|_| package::invalid()))
            .transpose()?;
        let toolchain = settings
            .as_ref()
            .and_then(|settings| settings.get("default_toolchain"))
            .and_then(toml::Value::as_str);
        let mut native = None;
        if let Some(toolchain) = toolchain {
            if toolchain.is_empty()
                || toolchain.len() > 256
                || !toolchain
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
            {
                return Err(package::invalid());
            }
            let path = local_path(
                &self
                    .rustup
                    .join("toolchains")
                    .join(toolchain)
                    .join("bin/rust-analyzer.exe"),
            )?;
            if path.is_file() {
                native = Some((path, "rustup"));
            }
        }
        if native.is_none() {
            for root in &self.paths {
                let path = local_path(&root.join("rust-analyzer.exe"))?;
                if path.is_file()
                    && !path
                        .components()
                        .any(|component| component.as_os_str().eq_ignore_ascii_case(".cargo"))
                {
                    native = Some((path, "path"));
                    break;
                }
            }
        }
        let Some((path, manager)) = native else {
            return Ok(vec![]);
        };
        let hash = package::fingerprint(&path)?;
        let receipt_path = path.parent().unwrap().join(".harness-provisioning.json");
        let receipt = package::read_json(&receipt_path)?;
        let valid_receipt = receipt.as_ref().filter(|receipt| {
            receipt["owner"] == "codex-harness-dependencies" && receipt["executable_sha256"] == hash
        });
        let mut version = valid_receipt
            .and_then(|receipt| package::text(&receipt["version"]))
            .map(str::to_owned);
        let mut evidence = vec![json!({"kind":"executable-fingerprint","path":path,"sha256":hash})];
        if let Some(receipt) = valid_receipt {
            evidence.push(json!({"kind":"verified-provisioning-record","source":package::text(&receipt["source"]),"path":receipt_path}));
        }
        if self.probe_versions {
            match crate::dependency_probe::rust_analyzer(&path, &hash) {
                Ok(observed) => {
                    evidence.push(json!({"kind":"version-command","exit_code":0,"value":observed}));
                    version = Some(observed);
                }
                Err(_) => evidence
                    .push(json!({"kind":"version-command","result":"failed-or-incompatible"})),
            }
        }
        Ok(vec![
            json!({"manager":manager,"version":version,"executable":path,"command":[path],
            "paths":{"executable":path},"installation_root":path.parent(),"status":"adopted","ownership":"adopted-shared",
            "health":{"installed":true,"identity_verified":version.is_some(),"integrity":"unknown","callable":null,"checked_operations":[]},
            "update_safe":false,"provenance":{"resolved_path":path},"evidence":evidence}),
        ])
    }

    fn record(&self, spec: &Value, group: &str) -> Value {
        let result = match spec["id"].as_str().unwrap() {
            "serena" | "graphify" => uv(spec, &self.uv, self.full),
            "codebase-memory" => self.npm("codebase-memory-mcp", "codebase-memory-mcp", None),
            "nuphus" => self.npm("@nuphus/nuphus-mcp", "nuphus-mcp", None),
            "python" => self
                .npm(
                    "basedpyright",
                    "basedpyright-langserver",
                    Some("BasedPyrightLanguageServer"),
                )
                .and_then(|mut candidates| {
                    candidates.extend(self.npm(
                        "pyright",
                        "pyright-langserver",
                        Some("PyrightServer"),
                    )?);
                    Ok(candidates)
                }),
            "rust" => self.rust(),
            _ => unreachable!(),
        };
        let mut record = match result {
            Ok(candidates) => package::select(package::base(spec), candidates),
            Err(_) => {
                let mut record = package::base(spec);
                record["status"] = json!("incomplete");
                record["evidence"] = json!([{"kind":"unavailable-observation","detail":"A dependency input is unreadable, malformed or exceeds the observation limit; no package was executed or changed."}]);
                record
            }
        };
        if group == "languages" {
            for (key, value) in [
                ("required", spec["required"].clone()),
                ("backend", spec["backend"].clone()),
                ("serena_id", spec["serena_id"].clone()),
                ("project_inputs", spec["project_inputs"].clone()),
                ("runtime_prerequisites", spec["runtime"].clone()),
                ("automatic_diagnostics", json!("unverified")),
                ("exposed_operations", json!([])),
            ] {
                record[key] = value;
            }
        }
        if spec["id"] == "nuphus" {
            record["models"] = json!(
                [
                    "ch_PP-OCRv4_det.onnx",
                    "ch_PP-OCRv4_rec.onnx",
                    "ch_PP-OCR_keys_v1.txt"
                ]
                .map(|name| {
                    let path = self.models.join(name);
                    json!({"path":path,"exists":path.is_file()})
                })
            );
            if let Some(original) = package::text(&record["paths"]["original_native_executable"]) {
                let runtime = Path::new(original)
                    .parent()
                    .unwrap()
                    .join("onnxruntime.dll");
                record["health"]["onnxruntime_exists"] = json!(runtime.is_file());
                record["paths"]["onnxruntime"] = json!(runtime);
            }
        } else if spec["id"] == "graphify" {
            record["excluded_installations"] = match self.npm("@dreamtree-org/graphify", "graphify", None) {
                Ok(candidates) => json!(candidates.iter().map(|candidate| json!({"identity":"@dreamtree-org/graphify","installation_root":candidate["installation_root"],"version":candidate["version"],"reason":"Unrelated package; command name is not identity."})).collect::<Vec<_>>()),
                Err(_) => json!([{"identity":"@dreamtree-org/graphify","status":"incomplete"}]),
            };
            record["shared_service"] = self.graphify();
        }
        record
    }

    fn graphify(&self) -> Value {
        let mut report = json!({"state":"unverified","manifest":self.graphify_manifest});
        let Some(path) = &self.graphify_manifest else {
            return report;
        };
        let Ok(Some(data)) = package::read_json(path) else {
            return report;
        };
        let config = &data["graphify"]["configuration"];
        for key in ["python", "graph"] {
            if let Some(path) = package::text(&config[key]["path"]) {
                report[format!("{key}_path")] = json!(path);
                report[format!("{key}_exists")] = json!(Path::new(path).is_file());
            }
        }
        report["module_name"] = json!(package::text(&config["module"]["name"]));
        report["package_version"] = json!(package::text(&config["module"]["packageVersion"]));
        report["authentication"] = json!("not-read; connection layer must resolve securely");
        report
    }
}

pub fn discover(request: &Request) -> io::Result<Value> {
    let catalogue = local_path(&request.catalogue)?;
    let spec = package::read_json(&catalogue)?.ok_or_else(package::invalid)?;
    discover_with_catalogue(request, spec)
}

pub(crate) fn discover_with_catalogue(request: &Request, spec: Value) -> io::Result<Value> {
    let discovery = Discovery::new(request, spec)?;
    let records = |group: &str| {
        discovery.spec[group]
            .as_array()
            .unwrap()
            .iter()
            .map(|spec| discovery.record(spec, group))
            .collect::<Vec<_>>()
    };
    let mut all = records("mcp");
    let mcp_count = all.len();
    all.extend(records("languages"));
    if request.processes {
        crate::dependency_process::observe(&mut all)?;
    }
    let languages = all.split_off(mcp_count);
    Ok(
        json!({"schema_version":1,"catalogue":discovery.catalogue,"user_home":discovery.home,
        "read_only":true,"model_calls":0,"processes_started":if request.probe_versions {Value::Null} else {json!(0)},
        "version_probes_requested":request.probe_versions,
        "process_inspection_requested":request.processes,
        "release_checks":"not-requested; explicit lifecycle operation required",
        "mcp":all,"languages":languages}),
    )
}
