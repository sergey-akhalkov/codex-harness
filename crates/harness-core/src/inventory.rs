//! Live declarative core inventory. No mutation, interpreter, hooks or models.
//! Bounded preflight refuses foreign global skill/agent descriptors that reuse a
//! managed capability name. Matching intended destinations are reused, not refused.
//! Read-only foreign discovery may follow ordinary symlink roots and descriptors;
//! source traversal and mutation inputs stay on ordinary non-reparse paths.
use crate::build_identity;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

const TEXT_LIMIT: u64 = 262144;

const SCAN_ENTRIES: usize = 4096;
const SCAN_DEPTH: usize = 8;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub profile_name: String,
    pub profile: String,
    pub instructions: String,
    pub skills: String,
    pub agents: String,
    pub hooks: String,
    pub token_hooks: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Connection {
    Missing,
    Linked,
    Conflict,
}

#[derive(Clone, Debug, Serialize)]
pub struct Link {
    pub kind: String,
    pub name: String,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub connection: Connection,
}

#[derive(Debug, Serialize)]
pub struct Agent {
    pub name: String,
    pub source: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Inventory {
    pub manifest: Manifest,
    pub source_root: PathBuf,
    pub codex_home: PathBuf,
    pub user_home: PathBuf,
    pub links: Vec<Link>,
    pub agents: Vec<Agent>,
    pub native_binaries: &'static [&'static str],
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

fn text(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(TEXT_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > TEXT_LIMIT {
        return Err(invalid("kit descriptor exceeds its size bound"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("kit descriptor is not UTF-8"))
}

fn simple_name(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(|c| c.is_ascii_lowercase())
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
}

fn descriptor_name(path: &Path, skill: bool) -> io::Result<String> {
    let body = text(path)?;
    if !skill {
        let document: toml::Table = toml::from_str(body.trim_start_matches('\u{feff}'))
            .map_err(|_| invalid("agent descriptor is not valid TOML"))?;
        return document
            .get("name")
            .and_then(toml::Value::as_str)
            .filter(|name| simple_name(name))
            .map(str::to_owned)
            .ok_or_else(|| invalid("descriptor requires one unique top-level name"));
    }
    let mut lines = body.trim_start_matches('\u{feff}').lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err(invalid("skill requires YAML frontmatter"));
    }
    let mut names = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let line = line.trim();
        if line == "---" {
            closed = true;
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() != "name" {
            continue;
        }
        let value = value.trim();
        let value = if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        if !simple_name(value) {
            return Err(invalid("descriptor requires a simple portable name"));
        }
        names.push(value.to_owned());
    }
    if !closed || names.len() != 1 {
        return Err(invalid("descriptor requires one unique top-level name"));
    }
    Ok(names.remove(0))
}

fn owned_source(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(invalid(
            "manifest source must be a relative path inside the checkout",
        ));
    }
    let mut path = root.to_owned();
    for part in relative.components() {
        path.push(part);
        build_identity::ordinary(&path)?;
    }
    Ok(path)
}

/// Mutating consumers also use this guard before touching a target. It examines
/// ancestors without following a junction into source or a shared cache.
pub fn ordinary_parents(path: &Path) -> io::Result<()> {
    for parent in path.ancestors().skip(1) {
        match fs::symlink_metadata(parent) {
            Ok(_) => build_identity::ordinary(parent)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn target_root(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(invalid(
            "installation roots must be absolute without parent traversal",
        ));
    }
    ordinary_parents(&path.join("probe"))?;
    Ok(path.to_owned())
}

fn link(kind: &str, name: &str, source: PathBuf, destination: PathBuf) -> io::Result<Link> {
    ordinary_parents(&destination)?;
    let connection = match fs::symlink_metadata(&destination) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Connection::Missing,
        Err(e) => return Err(e),
        Ok(meta) if meta.file_type().is_symlink() => {
            let target = fs::read_link(&destination)?;
            let target = if target.is_absolute() {
                target
            } else {
                destination.parent().unwrap().join(target)
            };
            if target.canonicalize().ok().as_deref() == Some(source.canonicalize()?.as_path()) {
                Connection::Linked
            } else {
                Connection::Conflict
            }
        }
        Ok(_) => Connection::Conflict,
    };
    Ok(Link {
        kind: kind.into(),
        name: name.into(),
        source,
        destination,
        connection,
    })
}

fn agents(root: &Path, names: &mut BTreeSet<String>, found: &mut Vec<Agent>) -> io::Result<()> {
    let mut remaining = SCAN_ENTRIES;
    walk_agents(root, names, found, &mut remaining)
}

fn walk_agents(
    root: &Path,
    names: &mut BTreeSet<String>,
    found: &mut Vec<Agent>,
    remaining: &mut usize,
) -> io::Result<()> {
    if *remaining == 0 {
        return Err(invalid("agent scan exceeds its bound"));
    }
    *remaining -= 1;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        build_identity::ordinary(&path)?;
        if path.is_dir() {
            walk_agents(&path, names, found, remaining)?;
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        {
            let name = descriptor_name(&path, false)?;
            if !names.insert(name.clone()) {
                return Err(invalid("duplicate agent name in checkout"));
            }
            found.push(Agent { name, source: path });
        }
    }
    Ok(())
}

pub fn read(source: &Path, codex_home: &Path, user_home: &Path) -> io::Result<Inventory> {
    let source = source.canonicalize()?;
    let codex_home = target_root(codex_home)?;
    let user_home = target_root(user_home)?;
    let manifest_path = owned_source(&source, "global/kit.json")?;
    let manifest: Manifest = serde_json::from_str(&text(&manifest_path)?)
        .map_err(|_| invalid("invalid native kit manifest"))?;
    if manifest.schema != 1 || manifest.profile_name != "harness" {
        return Err(invalid("unsupported native kit manifest"));
    }
    for relative in [
        &manifest.profile,
        &manifest.instructions,
        &manifest.hooks,
        &manifest.token_hooks,
    ] {
        if !owned_source(&source, relative)?.is_file() {
            return Err(invalid("required kit data file is missing"));
        }
    }
    let mut links = vec![
        link(
            "instructions",
            "AGENTS",
            source.join(&manifest.instructions),
            codex_home.join("AGENTS.md"),
        )?,
        link(
            "profile",
            "harness",
            source.join(&manifest.profile),
            codex_home.join("harness.config.toml"),
        )?,
        link(
            "agents",
            "codex-harness",
            owned_source(&source, &manifest.agents)?,
            codex_home.join("agents/codex-harness"),
        )?,
    ];
    let skills = owned_source(&source, &manifest.skills)?;
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(skills)? {
        let entry = entry?;
        let path = entry.path();
        build_identity::ordinary(&path)?;
        if !path.is_dir() {
            continue;
        }
        let descriptor = path.join("SKILL.md");
        build_identity::ordinary(&descriptor)?;
        let name = descriptor_name(&descriptor, true)?;
        if !names.insert(name.clone()) {
            return Err(invalid("duplicate skill name in checkout"));
        }
        let destination = user_home.join(".agents/skills").join(entry.file_name());
        links.push(link("skill", &name, path, destination)?);
    }
    links.sort_by(|a, b| a.destination.cmp(&b.destination));
    let mut found = Vec::new();
    agents(
        &source.join(&manifest.agents),
        &mut BTreeSet::new(),
        &mut found,
    )?;
    found.sort_by(|a, b| a.name.cmp(&b.name));
    refuse_foreign(&links, &found, &codex_home, &user_home)?;
    Ok(Inventory {
        manifest,
        source_root: source,
        codex_home,
        user_home,
        links,
        agents: found,
        native_binaries: build_identity::BINARIES,
    })
}

fn refuse_foreign(
    links: &[Link],
    agents: &[Agent],
    codex_home: &Path,
    user_home: &Path,
) -> io::Result<()> {
    refuse_foreign_skills(links, user_home)?;
    refuse_foreign_agents(links, agents, codex_home)
}

fn refuse_foreign_skills(links: &[Link], user_home: &Path) -> io::Result<()> {
    let root = user_home.join(".agents/skills");
    let Some(entries) = existing_dir(&root)? else {
        return Ok(());
    };
    let mut remaining = SCAN_ENTRIES;
    let mut seen = BTreeSet::new();
    for entry in entries {
        let path = entry?.path();
        if intended_destination(&path, links) {
            continue;
        }
        let kind = inspect_foreign(&path, 0, &mut remaining, &mut seen)?;
        if kind != ForeignKind::Directory {
            continue;
        }
        let descriptor = path.join("SKILL.md");
        if present_foreign_file(&descriptor, 1, &mut remaining, &mut seen)?.is_none() {
            continue;
        }
        let name = descriptor_name(&descriptor, true)?;
        if links
            .iter()
            .any(|link| link.kind == "skill" && link.name == name)
        {
            return Err(invalid("capability name collision"));
        }
    }
    Ok(())
}

fn refuse_foreign_agents(links: &[Link], agents: &[Agent], codex_home: &Path) -> io::Result<()> {
    let global = codex_home.join("agents");
    let Some(entries) = existing_dir(&global)? else {
        return Ok(());
    };
    let mut remaining = SCAN_ENTRIES;
    let mut seen = BTreeSet::new();
    for entry in entries {
        let path = entry?.path();
        if intended_destination(&path, links) {
            continue;
        }
        let kind = inspect_foreign(&path, 0, &mut remaining, &mut seen)?;
        if kind == ForeignKind::Directory {
            scan_foreign_agents(&path, agents, 1, &mut remaining, &mut seen)?;
        } else if toml_file(&path) {
            refuse_agent(&path, agents)?;
        }
    }
    Ok(())
}

fn scan_foreign_agents(
    root: &Path,
    agents: &[Agent],
    depth: usize,
    remaining: &mut usize,
    seen: &mut BTreeSet<Vec<u8>>,
) -> io::Result<()> {
    if depth > SCAN_DEPTH {
        return Err(invalid("agent scan exceeds its bound"));
    }
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        let kind = inspect_foreign(&path, depth, remaining, seen)?;
        if kind == ForeignKind::Directory {
            scan_foreign_agents(&path, agents, depth + 1, remaining, seen)?;
        } else if toml_file(&path) {
            refuse_agent(&path, agents)?;
        }
    }
    Ok(())
}

fn refuse_agent(path: &Path, agents: &[Agent]) -> io::Result<()> {
    let name = descriptor_name(path, false)?;
    if agents.iter().any(|agent| agent.name == name) {
        return Err(invalid("capability name collision"));
    }
    Ok(())
}

fn intended_destination(path: &Path, links: &[Link]) -> bool {
    links.iter().any(|link| same_place(path, &link.destination))
}

fn existing_dir(path: &Path) -> io::Result<Option<fs::ReadDir>> {
    match fs::read_dir(path) {
        Ok(entries) => Ok(Some(entries)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ForeignKind {
    Directory,
    File,
    Other,
}

fn present_foreign_file(
    path: &Path,
    depth: usize,
    remaining: &mut usize,
    seen: &mut BTreeSet<Vec<u8>>,
) -> io::Result<Option<ForeignKind>> {
    match inspect_foreign(path, depth, remaining, seen) {
        Ok(kind) if kind == ForeignKind::File => Ok(Some(kind)),
        Ok(_) => Ok(None),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn inspect_foreign(
    path: &Path,
    depth: usize,
    remaining: &mut usize,
    seen: &mut BTreeSet<Vec<u8>>,
) -> io::Result<ForeignKind> {
    if *remaining == 0 {
        return Err(invalid("global descriptor scan exceeds its bound"));
    }
    *remaining -= 1;
    if depth > SCAN_DEPTH {
        return Err(invalid("global descriptor scan exceeds its bound"));
    }
    let meta = fs::symlink_metadata(path)?;
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        return follow_foreign_link(path, depth, remaining, seen);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(invalid("malformed foreign descriptor link"));
        }
    }
    if file_type.is_dir() {
        Ok(ForeignKind::Directory)
    } else if file_type.is_file() {
        Ok(ForeignKind::File)
    } else {
        Ok(ForeignKind::Other)
    }
}

fn follow_foreign_link(
    path: &Path,
    depth: usize,
    remaining: &mut usize,
    seen: &mut BTreeSet<Vec<u8>>,
) -> io::Result<ForeignKind> {
    let raw = fs::read_link(path).map_err(|_| invalid("unreadable foreign descriptor link"))?;
    if raw.as_os_str().is_empty() {
        return Err(invalid("incomplete foreign descriptor link"));
    }
    let target = if raw.is_absolute() {
        raw
    } else {
        path.parent()
            .ok_or_else(|| invalid("malformed foreign descriptor link"))?
            .join(raw)
    };
    let resolved = fs::canonicalize(&target).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            invalid("dangling foreign descriptor link")
        } else {
            invalid("unreadable foreign descriptor link")
        }
    })?;
    if !seen.insert(path_key(path)) || !seen.insert(path_key(&resolved)) {
        return Err(invalid("cyclic foreign descriptor scan"));
    }
    if *remaining == 0 {
        return Err(invalid("global descriptor scan exceeds its bound"));
    }
    *remaining -= 1;
    if depth + 1 > SCAN_DEPTH {
        return Err(invalid("global descriptor scan exceeds its bound"));
    }
    let meta = fs::metadata(&resolved).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            invalid("dangling foreign descriptor link")
        } else {
            invalid("unreadable foreign descriptor link")
        }
    })?;
    let file_type = meta.file_type();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let raw_meta = fs::symlink_metadata(&resolved)?;
        if raw_meta.file_attributes() & 0x400 != 0 && !raw_meta.file_type().is_symlink() {
            return Err(invalid("malformed foreign descriptor link"));
        }
    }
    if file_type.is_dir() {
        Ok(ForeignKind::Directory)
    } else if file_type.is_file() {
        Ok(ForeignKind::File)
    } else {
        Ok(ForeignKind::Other)
    }
}

fn toml_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
}

fn same_place(left: &Path, right: &Path) -> bool {
    path_key(left) == path_key(right)
}

fn path_key(path: &Path) -> Vec<u8> {
    path.as_os_str()
        .as_encoded_bytes()
        .iter()
        .map(|b| match b {
            b'/' | b'\\' => b'\\',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}
