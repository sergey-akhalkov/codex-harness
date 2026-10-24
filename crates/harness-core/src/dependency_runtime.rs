//! Pinned, integrity-checked runtime acquisition for the independent Serena
//! plugin. Ordinary discovery and startup never call this module.
#![cfg(windows)]
use crate::{
    build_identity, dependency_archive, native_build,
    process::{CommandSpec, StopReason},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};
use ureq::ResponseExt;

pub(crate) const UV_VERSION: &str = "0.12.24";
pub(crate) const PYTHON_VERSION: &str = "3.13.16";
pub(crate) const NODE_VERSION: &str = "24.21.0";
pub(crate) const SERENA_VERSION: &str = "1.7.0";
pub(crate) const BASEDPYRIGHT_VERSION: &str = "1.40.2";
const SERENA_URL: &str = "https://files.pythonhosted.org/packages/95/c1/edd38220ce54fe37999d5b4a0790e6cfcc3557f71a8f47cf850fc079769b/serena_agent-1.7.0-py3-none-any.whl";
const SERENA_SHA256: &str = "6dbf1459670d96fb0595f84932adef34260a6fe14ba5135b901fdb3c8c76e891";

const UV_ARCHIVE: &str = "uv-x86_64-pc-windows-msvc.zip";
const PYTHON_ARCHIVE: &str = "python-3.13.16-embed-amd64.zip";
const NODE_ARCHIVE: &str = "node-v24.21.0-win-x64.zip";
const BASEDPYRIGHT_ARCHIVE: &str = "basedpyright-1.40.2.tgz";
const REQUIREMENTS: &str = include_str!("serena_requirements.txt");
const BUILD_REQUIREMENTS: &str = include_str!("serena_build_requirements.txt");
const DOWNLOAD_LIMIT: u64 = 128 * 1024 * 1024;

struct Asset {
    id: &'static str,
    archive: &'static str,
    url: &'static str,
    sha256: &'static str,
    integrity: Option<&'static str>,
    hosts: &'static [&'static str],
}

const UV: Asset = Asset {
    id: "uv",
    archive: UV_ARCHIVE,
    url: "https://github.com/astral-sh/uv/releases/download/0.12.24/uv-x86_64-pc-windows-msvc.zip",
    sha256: "7c38608c8a18ee137d748a1773053b07ec8f3a30fab49aebaa6f4e4efeceb019",
    integrity: None,
    hosts: &["github.com", "release-assets.githubusercontent.com"],
};
const PYTHON: Asset = Asset {
    id: "python",
    archive: PYTHON_ARCHIVE,
    url: "https://www.python.org/ftp/python/3.13.16/python-3.13.16-embed-amd64.zip",
    sha256: "97dae5274cc54867065e8d5a3226e48c35017ed332a0fdb0e27d5b5821961297",
    integrity: None,
    hosts: &["www.python.org"],
};
const NODE: Asset = Asset {
    id: "node",
    archive: NODE_ARCHIVE,
    url: "https://nodejs.org/dist/v24.21.0/node-v24.21.0-win-x64.zip",
    sha256: "158f7685b44de51f6c0df1d153526cbcd3e1bc739a8dfc607721cef75de9e541",
    integrity: None,
    hosts: &["nodejs.org"],
};
const BASEDPYRIGHT: Asset = Asset {
    id: "basedpyright",
    archive: BASEDPYRIGHT_ARCHIVE,
    url: "https://registry.npmjs.org/basedpyright/-/basedpyright-1.40.2.tgz",
    sha256: "5d1073a0e61a1adc6b40b2bed4489017aa5c1336aa8e6fdcb7157b9bf26f3fab",
    integrity: Some(
        "sha512-9uld8+5tZC2Gxf8nIynyunK0N4xn01W1ApzLMXDAcuA8uJq2QSfldYn9gSbxn9MWR8wmHDo7GaEHLhlnLFHaJA==",
    ),
    hosts: &["registry.npmjs.org"],
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Serena runtime acquisition input or artifact was rejected",
    )
}

fn failed(code: &str) -> io::Error {
    io::Error::other(format!("Serena runtime preparation: {code}"))
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Needs {
    pub serena: bool,
    pub python: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Runtime {
    pub uv: Option<PathBuf>,
    pub python: Option<PathBuf>,
    pub serena: Option<PathBuf>,
    pub serena_console: Option<PathBuf>,
    pub node: Option<PathBuf>,
    pub basedpyright: Option<PathBuf>,
    pub acquired: Vec<&'static str>,
    pub reused: Vec<&'static str>,
}

impl Runtime {
    pub(crate) fn uv_tools(&self) -> Option<&Path> {
        self.serena.as_deref()
    }

    pub(crate) fn npm_prefix(&self) -> Option<&Path> {
        self.basedpyright
            .as_deref()
            .and_then(Path::parent)
            .and_then(Path::parent)
    }
}

fn runtime_root(state: &Path) -> PathBuf {
    state.join("runtime")
}

fn uv_dir(state: &Path) -> PathBuf {
    runtime_root(state).join(format!("uv-{UV_VERSION}"))
}

fn python_dir(state: &Path) -> PathBuf {
    runtime_root(state).join(format!("python-{PYTHON_VERSION}"))
}

fn node_dir(state: &Path) -> PathBuf {
    runtime_root(state).join(format!("node-{NODE_VERSION}"))
}

fn serena_tools_dir(state: &Path) -> PathBuf {
    runtime_root(state).join(format!("uv-tools-serena-{SERENA_VERSION}"))
}

fn serena_dir(state: &Path) -> PathBuf {
    serena_tools_dir(state).join("serena-agent")
}

fn basedpyright_dir(state: &Path) -> PathBuf {
    runtime_root(state)
        .join(format!("npm-basedpyright-{BASEDPYRIGHT_VERSION}"))
        .join("node_modules")
        .join("basedpyright")
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    schema: u32,
    id: String,
    version: String,
    files: BTreeMap<String, String>,
}

fn hash_file(path: &Path) -> io::Result<String> {
    build_identity::hash_file(path)
}

fn marker_valid(dir: &Path, id: &str, version: &str) -> io::Result<bool> {
    let path = dir.join("ready.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if bytes.len() > 64 * 1024 {
        return Ok(false);
    }
    let marker: Marker = match serde_json::from_slice(&bytes) {
        Ok(marker) => marker,
        Err(_) => return Ok(false),
    };
    if marker.schema != 1 || marker.id != id || marker.version != version || marker.files.is_empty()
    {
        return Ok(false);
    }
    for (relative, expected) in marker.files {
        let relative = Path::new(&relative);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Ok(false);
        }
        let file = dir.join(relative);
        if !file.is_file() || hash_file(&file)? != expected {
            return Ok(false);
        }
    }
    Ok(true)
}

fn write_marker(
    dir: &Path,
    id: &str,
    version: &str,
    files: impl IntoIterator<Item = (&'static str, PathBuf)>,
) -> io::Result<()> {
    let mut hashes = BTreeMap::new();
    for (relative, path) in files {
        hashes.insert(relative.to_owned(), hash_file(&path)?);
    }
    if hashes.is_empty() {
        return Err(invalid());
    }
    let marker = Marker {
        schema: 1,
        id: id.to_owned(),
        version: version.to_owned(),
        files: hashes,
    };
    fs::write(dir.join("ready.json"), serde_json::to_vec_pretty(&marker)?)
}

fn within_state(state: &Path, path: &Path) -> bool {
    path.starts_with(state) && path != state
}

fn remove_owned_tree(state: &Path, path: &Path) -> io::Result<()> {
    if !within_state(state, path) {
        return Err(invalid());
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(failed("owned staging cleanup failed")),
    }
}

fn clean_staging(state: &Path) -> io::Result<()> {
    let root = runtime_root(state);
    if !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("staging-") && entry.file_type()?.is_dir() {
            remove_owned_tree(state, &entry.path())?;
        }
    }
    Ok(())
}

fn staging(state: &Path, label: &str) -> io::Result<tempfile::TempDir> {
    let root = runtime_root(state);
    fs::create_dir_all(&root)?;
    tempfile::Builder::new()
        .prefix(&format!("staging-{label}-"))
        .tempdir_in(&root)
}

fn publish(state: &Path, stage: tempfile::TempDir, destination: &Path) -> io::Result<()> {
    if !within_state(state, destination) {
        return Err(invalid());
    }
    let stage = stage.keep();
    if destination.exists() {
        return remove_owned_tree(state, &stage);
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(&stage, destination) {
        Ok(()) => Ok(()),
        Err(error) if destination.exists() => {
            let _ = remove_owned_tree(state, &stage);
            Err(error)
        }
        Err(error) => Err(error),
    }
}

fn local_asset(asset: &Asset) -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let root = std::env::var_os("HARNESS_SERENA_SETUP_ASSETS")?;
    let path = PathBuf::from(root).join(asset.archive);
    path.is_file().then_some(path)
}

fn redirect_allowed(asset: &Asset, url: &str) -> bool {
    let Ok(uri) = url.parse::<ureq::http::Uri>() else {
        return false;
    };
    uri.scheme_str() == Some("https")
        && uri
            .authority()
            .is_some_and(|authority| !authority.as_str().contains('@'))
        && uri.port_u16().is_none_or(|port| port == 443)
        && uri.host().is_some_and(|host| asset.hosts.contains(&host))
}

fn download(asset: &Asset) -> io::Result<Vec<u8>> {
    if let Some(path) = local_asset(asset) {
        let bytes = fs::read(path)?;
        if bytes.len() as u64 > DOWNLOAD_LIMIT {
            return Err(failed("artifact-size-limit"));
        }
        return verify(asset, bytes);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .max_redirects(2)
        .save_redirect_history(true)
        .build()
        .into();
    let mut response = agent
        .get(asset.url)
        .header("User-Agent", "codex-harness-serena-setup")
        .call()
        .map_err(|_| failed("artifact-transfer-failed"))?;
    if response.status() != 200 {
        return Err(failed("artifact-http-status-rejected"));
    }
    let history = response
        .get_redirect_history()
        .ok_or_else(|| failed("artifact-redirect-history-unavailable"))?;
    let expected = asset
        .url
        .parse::<ureq::http::Uri>()
        .map_err(|_| failed("artifact-url-invalid"))?;
    if history.first().is_none_or(|uri| uri != &expected)
        || history
            .iter()
            .any(|uri| !redirect_allowed(asset, &uri.to_string()))
    {
        return Err(failed("artifact-redirect-rejected"));
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(DOWNLOAD_LIMIT)
        .read_to_vec()
        .map_err(|_| failed("artifact-size-limit-or-transfer-failed"))?;
    verify(asset, bytes)
}

fn verify(asset: &Asset, bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    if bytes.len() as u64 > DOWNLOAD_LIMIT {
        return Err(failed("artifact-size-limit"));
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if digest != asset.sha256 {
        return Err(failed(&format!("artifact-integrity-mismatch:{}", asset.id)));
    }
    if let Some(integrity) = asset.integrity {
        dependency_archive::verify_sri(&bytes, integrity).map_err(|_| {
            failed(&format!(
                "artifact-published-integrity-mismatch:{}",
                asset.id
            ))
        })?;
    }
    Ok(bytes)
}

fn acquire(state: &Path, asset: &Asset) -> io::Result<Vec<u8>> {
    let root = state.join("downloads");
    fs::create_dir_all(&root)?;
    let path = root.join(asset.archive);
    match fs::read(&path) {
        Ok(bytes) => {
            if format!("{:x}", Sha256::digest(&bytes)) == asset.sha256 {
                return Ok(bytes);
            }
            fs::remove_file(&path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let bytes = download(asset)?;
    let part = root.join(format!("{}.part", asset.archive));
    match fs::remove_file(&part) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::write(&part, &bytes)?;
    fs::rename(&part, &path)?;
    Ok(bytes)
}

fn extract_zip(
    bytes: &[u8],
    root: &Path,
    strip: Option<&str>,
) -> io::Result<dependency_archive::Summary> {
    dependency_archive::visit_zip(bytes, |path, _size, reader| {
        let relative = match strip {
            Some(prefix) => path.strip_prefix(prefix).ok_or_else(invalid)?,
            None => path,
        };
        if relative.is_empty() {
            return Ok(());
        }
        let target = root.join(relative);
        let parent = target.parent().ok_or_else(invalid)?;
        fs::create_dir_all(parent)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        io::copy(reader, &mut file)?;
        Ok(())
    })
}

fn extract_npm(bytes: &[u8], root: &Path) -> io::Result<dependency_archive::Summary> {
    dependency_archive::visit_npm_tar(bytes, |path, _size, reader| {
        let target = root.join(path);
        let parent = target.parent().ok_or_else(invalid)?;
        fs::create_dir_all(parent)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        io::copy(reader, &mut file)?;
        Ok(())
    })
}

fn ensure_uv(state: &Path, runtime: &mut Runtime) -> io::Result<()> {
    let dir = uv_dir(state);
    if marker_valid(&dir, "uv", UV_VERSION)? {
        runtime.uv = Some(dir.join("uv.exe"));
        runtime.reused.push("uv");
        return Ok(());
    }
    let stage = staging(state, "uv")?;
    let bytes = acquire(state, &UV)?;
    extract_zip(&bytes, stage.path(), None)?;
    let executable = stage.path().join("uv.exe");
    if !executable.is_file() {
        return Err(failed("uv-executable-missing"));
    }
    write_marker(stage.path(), "uv", UV_VERSION, [("uv.exe", executable)])?;
    if dir.exists() {
        return Err(failed("prepared-uv-state-changed; preserve it and retry"));
    }
    publish(state, stage, &dir)?;
    runtime.uv = Some(dir.join("uv.exe"));
    runtime.acquired.push("uv");
    Ok(())
}

fn ensure_python(state: &Path, runtime: &mut Runtime) -> io::Result<()> {
    let dir = python_dir(state);
    if marker_valid(&dir, "python", PYTHON_VERSION)? {
        runtime.python = Some(dir.join("python.exe"));
        runtime.reused.push("python");
        return Ok(());
    }
    let stage = staging(state, "python")?;
    let bytes = acquire(state, &PYTHON)?;
    extract_zip(&bytes, stage.path(), None)?;
    let executable = stage.path().join("python.exe");
    if !executable.is_file() {
        return Err(failed("python-executable-missing"));
    }
    write_marker(
        stage.path(),
        "python",
        PYTHON_VERSION,
        [("python.exe", executable)],
    )?;
    if dir.exists() {
        return Err(failed(
            "prepared-python-state-changed; preserve it and retry",
        ));
    }
    publish(state, stage, &dir)?;
    runtime.python = Some(dir.join("python.exe"));
    runtime.acquired.push("python");
    Ok(())
}

fn ensure_node(state: &Path, runtime: &mut Runtime) -> io::Result<PathBuf> {
    let dir = node_dir(state);
    let executable = dir.join("node.exe");
    if marker_valid(&dir, "node", NODE_VERSION)? {
        runtime.node = Some(executable.clone());
        runtime.reused.push("node");
        return Ok(executable);
    }
    let stage = staging(state, "node")?;
    let bytes = acquire(state, &NODE)?;
    let prefix = format!("node-v{NODE_VERSION}-win-x64/");
    extract_zip(&bytes, stage.path(), Some(&prefix))?;
    let staged = stage.path().join("node.exe");
    if !staged.is_file() {
        return Err(failed("node-executable-missing"));
    }
    write_marker(stage.path(), "node", NODE_VERSION, [("node.exe", staged)])?;
    if dir.exists() {
        return Err(failed("prepared-node-state-changed; preserve it and retry"));
    }
    publish(state, stage, &dir)?;
    runtime.node = Some(executable.clone());
    runtime.acquired.push("node");
    Ok(executable)
}

fn isolated_command(executable: &Path, private: &Path) -> io::Result<CommandSpec> {
    let mut command = CommandSpec::new(executable);
    for (key, _) in std::env::vars_os() {
        command.env.insert(key, None);
    }
    for key in [
        "SystemRoot",
        "WINDIR",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env.insert(key.into(), Some(value));
        }
    }
    for (key, folder) in [
        ("HOME", "home"),
        ("USERPROFILE", "home"),
        ("APPDATA", "roaming"),
        ("LOCALAPPDATA", "local"),
        ("TEMP", "temp"),
        ("TMP", "temp"),
        ("UV_CACHE_DIR", "cache"),
    ] {
        let path = private.join(folder);
        fs::create_dir_all(&path)?;
        command.env.insert(key.into(), Some(path.into_os_string()));
    }
    command.env.insert("UV_NO_CONFIG".into(), Some("1".into()));
    command
        .env
        .insert("UV_PYTHON_DOWNLOADS".into(), Some("never".into()));
    command
        .env
        .insert("UV_NO_PROGRESS".into(), Some("1".into()));
    command.current_dir = Some(private.to_owned());
    Ok(command)
}

fn run_tool(
    command: CommandSpec,
    private: &Path,
    label: &str,
    timeout: Duration,
) -> io::Result<()> {
    let stdout = private.join(format!("{label}-stdout"));
    let stderr = private.join(format!("{label}-stderr"));
    let outcome = native_build::invoke_management(command, &stderr, Some(&stdout), timeout)?;
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        let bytes = fs::read(&stderr).unwrap_or_default();
        let detail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(1024)..])
            .replace(['\r', '\n'], " ");
        return Err(failed(&format!("{label}-failed: {detail}")));
    }
    Ok(())
}

fn ensure_serena(state: &Path, runtime: &mut Runtime) -> io::Result<()> {
    let dir = serena_dir(state);
    if marker_valid(&dir, "serena-agent", SERENA_VERSION)? {
        runtime.serena = Some(serena_tools_dir(state));
        runtime.serena_console = Some(dir.join("Scripts/serena.exe"));
        runtime.reused.push("serena-agent");
        return Ok(());
    }
    let uv = runtime
        .uv
        .as_deref()
        .ok_or_else(|| failed("uv-unavailable"))?;
    let python = runtime
        .python
        .as_deref()
        .ok_or_else(|| failed("python-unavailable"))?;
    let stage = staging(state, "serena")?;
    let venv = stage.path().join("serena-agent");
    let private = tempfile::Builder::new()
        .prefix("harness-serena-install-")
        .tempdir()?;
    let mut command = isolated_command(uv, private.path())?;
    command.args = vec![
        "venv".into(),
        venv.as_os_str().to_owned(),
        "--python".into(),
        python.as_os_str().to_owned(),
        "--no-project".into(),
        "--no-python-downloads".into(),
        "--offline".into(),
        "--no-config".into(),
        "--no-progress".into(),
    ];
    run_tool(command, private.path(), "uv-venv", Duration::from_secs(120))?;
    let venv_python = venv.join("Scripts/python.exe");
    if !venv_python.is_file() {
        return Err(failed("serena-venv-python-missing"));
    }
    let build_requirements = private.path().join("serena-build-requirements.txt");
    fs::write(&build_requirements, BUILD_REQUIREMENTS)?;
    let mut command = isolated_command(uv, private.path())?;
    command.args = vec![
        "pip".into(),
        "install".into(),
        "--python".into(),
        venv_python.as_os_str().to_owned(),
        "--requirement".into(),
        build_requirements.as_os_str().to_owned(),
        "--require-hashes".into(),
        "--index-url".into(),
        "https://pypi.org/simple".into(),
        "--no-config".into(),
        "--no-progress".into(),
    ];
    run_tool(
        command,
        private.path(),
        "uv-build-tools-install",
        Duration::from_secs(120),
    )?;
    let requirements = private.path().join("serena-requirements.txt");
    fs::write(&requirements, REQUIREMENTS)?;
    let mut command = isolated_command(uv, private.path())?;
    command.args = vec![
        "pip".into(),
        "install".into(),
        "--python".into(),
        venv_python.as_os_str().to_owned(),
        "--requirement".into(),
        requirements.as_os_str().to_owned(),
        "--require-hashes".into(),
        "--no-build-isolation".into(),
        "--index-url".into(),
        "https://pypi.org/simple".into(),
        "--no-config".into(),
        "--no-progress".into(),
    ];
    run_tool(
        command,
        private.path(),
        "uv-pip-install",
        Duration::from_secs(600),
    )?;
    let console = venv.join("Scripts/serena.exe");
    let module = venv.join("Lib/site-packages/serena/cli.py");
    if !console.is_file() || !module.is_file() || !venv_python.is_file() {
        return Err(failed("serena-installation-incomplete"));
    }
    fs::write(venv.join("serena-requirements.txt"), REQUIREMENTS)?;
    write_marker(
        &venv,
        "serena-agent",
        SERENA_VERSION,
        [
            ("Scripts/serena.exe", console.clone()),
            ("Scripts/python.exe", venv_python),
            ("Lib/site-packages/serena/cli.py", module),
        ],
    )?;
    if serena_tools_dir(state).exists() {
        return Err(failed(
            "prepared-serena-state-changed; preserve it and retry",
        ));
    }
    publish(state, stage, &serena_tools_dir(state))?;
    runtime.serena = Some(serena_tools_dir(state));
    runtime.serena_console = Some(dir.join("Scripts/serena.exe"));
    runtime.acquired.push("serena-agent");
    Ok(())
}

fn ensure_basedpyright(
    state: &Path,
    runtime: &mut Runtime,
    ambient_node: Option<&Path>,
) -> io::Result<()> {
    let node = match ambient_node {
        Some(node) if node.is_file() => {
            runtime.node = Some(node.to_owned());
            runtime.reused.push("node");
            node.to_owned()
        }
        _ => ensure_node(state, runtime)?,
    };
    let _ = node;
    let dir = basedpyright_dir(state);
    if marker_valid(&dir, "basedpyright", BASEDPYRIGHT_VERSION)? {
        runtime.basedpyright = Some(dir);
        runtime.reused.push("basedpyright");
        return Ok(());
    }
    let stage = staging(state, "basedpyright")?;
    let bytes = acquire(state, &BASEDPYRIGHT)?;
    extract_npm(&bytes, stage.path())?;
    let manifest_path = stage.path().join("package.json");
    let entry = stage.path().join("langserver.index.js");
    let manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    if manifest["name"] != "basedpyright"
        || manifest["version"] != BASEDPYRIGHT_VERSION
        || !entry.is_file()
    {
        return Err(failed("basedpyright-identity-mismatch"));
    }
    write_marker(
        stage.path(),
        "basedpyright",
        BASEDPYRIGHT_VERSION,
        [
            ("package.json", manifest_path),
            ("langserver.index.js", entry),
        ],
    )?;
    if basedpyright_dir(state).exists() {
        return Err(failed(
            "prepared-basedpyright-state-changed; preserve it and retry",
        ));
    }
    publish(state, stage, &dir)?;
    runtime.basedpyright = Some(dir);
    runtime.acquired.push("basedpyright");
    Ok(())
}

/// Install only missing runtime pieces into versioned owned directories.
/// The caller holds the owned-state lock and must not hold another one.
pub(crate) fn provision(
    state: &Path,
    needs: Needs,
    ambient_node: Option<&Path>,
) -> io::Result<Runtime> {
    let mut runtime = Runtime::default();
    if !needs.serena && !needs.python {
        return Ok(runtime);
    }
    fs::create_dir_all(runtime_root(state))?;
    clean_staging(state)?;
    if needs.serena {
        ensure_uv(state, &mut runtime)?;
        ensure_python(state, &mut runtime)?;
        ensure_serena(state, &mut runtime)?;
    }
    if needs.python {
        ensure_basedpyright(state, &mut runtime, ambient_node)?;
    }
    Ok(runtime)
}

/// Reuse only complete, verified piece directories.
pub(crate) fn prepared(state: &Path, needs: Needs) -> io::Result<Option<Runtime>> {
    let mut runtime = Runtime::default();
    if needs.serena {
        let uv = uv_dir(state);
        let python = python_dir(state);
        let tools = serena_tools_dir(state);
        if !marker_valid(&uv, "uv", UV_VERSION)?
            || !marker_valid(&python, "python", PYTHON_VERSION)?
            || !marker_valid(&serena_dir(state), "serena-agent", SERENA_VERSION)?
        {
            return Ok(None);
        }
        runtime.uv = Some(uv.join("uv.exe"));
        runtime.python = Some(python.join("python.exe"));
        runtime.serena = Some(tools);
        runtime.serena_console = Some(serena_dir(state).join("Scripts/serena.exe"));
        runtime.reused.extend(["uv", "python", "serena-agent"]);
    }
    if needs.python {
        if !marker_valid(
            &basedpyright_dir(state),
            "basedpyright",
            BASEDPYRIGHT_VERSION,
        )? {
            return Ok(None);
        }
        runtime.basedpyright = Some(basedpyright_dir(state));
        runtime.reused.push("basedpyright");
    }
    Ok(Some(runtime))
}

pub(crate) fn pinned() -> Value {
    serde_json::json!({
        "uv": {"version": UV_VERSION, "url": UV.url, "sha256": UV.sha256},
        "python": {"version": PYTHON_VERSION, "url": PYTHON.url, "sha256": PYTHON.sha256},
        "node": {"version": NODE_VERSION, "url": NODE.url, "sha256": NODE.sha256},
        "serena_agent": {"version": SERENA_VERSION, "url": SERENA_URL, "sha256": SERENA_SHA256, "requirements": "embedded-pypi-hashes"},
        "basedpyright": {"version": BASEDPYRIGHT_VERSION, "url": BASEDPYRIGHT.url, "sha256": BASEDPYRIGHT.sha256}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_rejects_changed_artifact() {
        let error = verify(&UV, b"changed".to_vec()).unwrap_err();
        assert!(error.to_string().contains("artifact-integrity-mismatch"));
    }

    #[test]
    fn marker_requires_matching_hashes_and_contained_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("uv.exe"), b"inert").unwrap();
        write_marker(
            root.path(),
            "uv",
            UV_VERSION,
            [("uv.exe", root.path().join("uv.exe"))],
        )
        .unwrap();
        assert!(marker_valid(root.path(), "uv", UV_VERSION).unwrap());
        fs::write(root.path().join("uv.exe"), b"changed").unwrap();
        assert!(!marker_valid(root.path(), "uv", UV_VERSION).unwrap());
        let marker = Marker {
            schema: 1,
            id: "uv".into(),
            version: UV_VERSION.into(),
            files: BTreeMap::from([("../escape".into(), "0".repeat(64))]),
        };
        fs::write(
            root.path().join("ready.json"),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
        assert!(!marker_valid(root.path(), "uv", UV_VERSION).unwrap());
    }
}
