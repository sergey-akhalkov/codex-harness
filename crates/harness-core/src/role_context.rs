//! Resolve owned responsibility guidance without changing routing or shared state.
//! Native callers retain their applicable user/project configuration, then apply
//! only these developer-instruction and skill-selection overrides to one session.
use crate::{build_identity, inventory};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Lead,
    Executor,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Executor => "executor",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    schema: u32,
    shared_skills: Vec<String>,
    roles: BTreeMap<Role, RoleSource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleSource {
    instructions: String,
    skills: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ManagedSkill {
    pub name: String,
    pub path: PathBuf,
    /// Membership allows discovery; this never overrides an explicit disable.
    pub allowed: bool,
}

#[derive(Debug)]
pub struct ResolvedRole {
    pub role: Role,
    /// Identity of required common, owned developer and selected role text.
    pub revision: String,
    pub developer_instructions: String,
    pub skills: Vec<ManagedSkill>,
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!(
        "Role context: {message}; repair the kit role sources or update the installation."
    ))
}

/// Read only ordinary owned source paths. All memberships and role inputs are
/// validated, so incomplete delivery fails before starting any conversation.
pub fn resolve(source_root: &Path, role: Role) -> io::Result<ResolvedRole> {
    let root = source_root.canonicalize()?;
    let read = |relative: &str| -> io::Result<String> {
        inventory::text(&inventory::owned_source(&root, relative)?)
            .map_err(|error| invalid(format!("cannot read {relative}: {error}")))
    };
    let manifest: inventory::Manifest = serde_json::from_str(&read("global/kit.json")?)
        .map_err(|_| invalid("invalid kit manifest"))?;
    if manifest.schema != 1 {
        return Err(invalid("unsupported kit manifest schema"));
    }
    let descriptor_text = read("global/roles.toml")?;
    let descriptor: Descriptor = toml::from_str(descriptor_text.trim_start_matches('\u{feff}'))
        .map_err(|_| invalid("invalid roles.toml or unknown role"))?;
    if descriptor.schema != 1 || descriptor.roles.len() != 2 {
        return Err(invalid(
            "roles.toml must define schema 1 and lead/executor roles",
        ));
    }
    let common = read(&manifest.instructions)?;
    let common_path = inventory::owned_source(&root, &manifest.instructions)?.canonicalize()?;
    let profile: toml::Table =
        toml::from_str(read(&manifest.profile)?.trim_start_matches('\u{feff}'))
            .map_err(|_| invalid("invalid owned profile"))?;
    let developer = instruction(&profile)?;
    let mut bodies = BTreeMap::new();
    let mut role_paths = BTreeSet::new();
    for (key, value) in &descriptor.roles {
        let path = inventory::owned_source(&root, &value.instructions)?;
        let canonical = path.canonicalize()?;
        if canonical == common_path || !role_paths.insert(canonical) {
            return Err(invalid("each role needs its own instruction source"));
        }
        let body = read(&value.instructions)?;
        if body.trim().is_empty() {
            return Err(invalid(format!("{} instructions are empty", key.label())));
        }
        bodies.insert(*key, body);
    }
    let mut membership = BTreeMap::new();
    for (names, owner) in std::iter::once((&descriptor.shared_skills, None)).chain(
        descriptor
            .roles
            .iter()
            .map(|(key, value)| (&value.skills, Some(*key))),
    ) {
        for name in names {
            if membership.insert(name.clone(), owner).is_some() {
                return Err(invalid(format!("skill {name} has duplicate memberships")));
            }
        }
    }
    let mut skills = Vec::new();
    let skill_root = inventory::owned_source(&root, &manifest.skills)?;
    for entry in fs::read_dir(skill_root)? {
        let entry = entry?;
        let path = entry.path();
        build_identity::ordinary(&path)?;
        if !path.is_dir() {
            continue;
        }
        let path = path.join("SKILL.md");
        build_identity::ordinary(&path)?;
        let name = inventory::descriptor_name(&path, true)?;
        let owner = membership.remove(&name).ok_or_else(|| {
            invalid(format!(
                "managed skill {name} is unclassified or duplicated"
            ))
        })?;
        skills.push(ManagedSkill {
            name,
            path: path.canonicalize()?,
            allowed: owner.is_none() || owner == Some(role),
        });
    }
    if !membership.is_empty() {
        return Err(invalid(format!(
            "memberships name missing managed skills: {}",
            membership.keys().cloned().collect::<Vec<_>>().join(", ")
        )));
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    let body = &bodies[&role];
    let revision = build_identity::hash_bytes(&serde_json::to_vec(&(
        role,
        descriptor_text,
        common,
        developer,
        body,
    ))?);
    let developer_instructions = join_instructions(developer, body);
    Ok(ResolvedRole {
        role,
        revision,
        developer_instructions,
        skills,
    })
}

fn instruction(table: &toml::Table) -> io::Result<&str> {
    match table.get("developer_instructions") {
        None => Ok(""),
        Some(toml::Value::String(value)) => Ok(value),
        Some(_) => Err(invalid("developer_instructions must be text")),
    }
}

fn join_instructions(first: &str, second: &str) -> String {
    match (first.is_empty(), second.is_empty()) {
        (true, _) => second.to_owned(),
        (_, true) => first.to_owned(),
        _ => format!("{first}\n\n{second}"),
    }
}

fn names_path(entry: &toml::Value, path: &Path) -> bool {
    entry
        .get("path")
        .and_then(toml::Value::as_str)
        .and_then(|value| Path::new(value).canonicalize().ok())
        .is_some_and(|value| value == path)
}

impl ResolvedRole {
    /// `retained` is the applicable native user/project layer, excluding the
    /// owned portable defaults. Return only role-owned overrides. Foreign skill
    /// entries and their unknown fields survive; allowed skills are never enabled.
    /// Explicit later native CLI overrides retain their native precedence.
    pub fn native_overrides(&self, retained: &toml::Table) -> io::Result<toml::Table> {
        let configured = match retained.get("skills") {
            None => None,
            Some(toml::Value::Table(table)) => table.get("config"),
            Some(_) => return Err(invalid("skills must be a table")),
        };
        let mut entries = match configured {
            None => Vec::new(),
            Some(toml::Value::Array(values)) => values.clone(),
            Some(_) => return Err(invalid("skills.config must be an array")),
        };
        for skill in self.skills.iter().filter(|skill| !skill.allowed) {
            let mut present = false;
            for entry in entries
                .iter_mut()
                .filter(|entry| names_path(entry, &skill.path))
            {
                let table = entry
                    .as_table_mut()
                    .ok_or_else(|| invalid("skill configuration must be a table"))?;
                table.insert("enabled".into(), false.into());
                present = true;
            }
            if !present {
                // Native 0.162.1 identifies the SKILL.md file, not its directory.
                // Strip Windows' verbatim prefix for portable native path input.
                let path = skill.path.to_string_lossy();
                let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
                entries.push(toml::Value::Table(toml::Table::from_iter([
                    ("path".into(), path.into()),
                    ("enabled".into(), false.into()),
                ])));
            }
        }
        Ok(toml::Table::from_iter([
            (
                "developer_instructions".into(),
                join_instructions(&self.developer_instructions, instruction(retained)?).into(),
            ),
            (
                "skills".into(),
                toml::Value::Table(toml::Table::from_iter([("config".into(), entries.into())])),
            ),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("global/roles")).unwrap();
        fs::write(
            root.path().join("global/kit.json"),
            serde_json::json!({
                "schema": 1, "profile_name": "harness", "profile": "global/profile.toml",
                "instructions": "global/common.md", "skills": "skills", "agents": "agents",
                "hooks": "hooks.json", "token_hooks": "token-hooks.json"
            })
            .to_string(),
        )
        .unwrap();
        for (name, body) in [
            ("common.md", "Common safeguards"),
            (
                "profile.toml",
                "developer_instructions = 'Shared developer'\nmodel = 'unchanged'",
            ),
            ("roles/lead.md", "Lead responsibility"),
            ("roles/executor.md", "Executor responsibility"),
            (
                "roles.toml",
                "schema = 1\nshared_skills = ['verify']\n[roles.lead]\ninstructions = 'global/roles/lead.md'\nskills = ['coordinate']\n[roles.executor]\ninstructions = 'global/roles/executor.md'\nskills = []\n",
            ),
        ] {
            fs::write(root.path().join("global").join(name), body).unwrap();
        }
        for name in ["verify", "coordinate"] {
            let directory = root.path().join("skills").join(name);
            fs::create_dir_all(&directory).unwrap();
            fs::write(
                directory.join("SKILL.md"),
                format!("---\nname: {name}\n---\nbody"),
            )
            .unwrap();
        }
        root
    }

    #[test]
    fn role_resolution_is_independent_and_revision_tracks_its_inputs() {
        let root = fixture();
        let lead = resolve(root.path(), Role::Lead).unwrap();
        let executor = resolve(root.path(), Role::Executor).unwrap();
        assert!(lead.skills.iter().all(|skill| skill.allowed));
        assert_eq!(
            executor.skills.iter().filter(|skill| skill.allowed).count(),
            1
        );
        assert_eq!(
            executor.developer_instructions,
            "Shared developer\n\nExecutor responsibility"
        );
        fs::write(
            root.path().join("global/roles/executor.md"),
            "Changed executor",
        )
        .unwrap();
        assert_eq!(
            lead.revision,
            resolve(root.path(), Role::Lead).unwrap().revision
        );
        assert_ne!(
            executor.revision,
            resolve(root.path(), Role::Executor).unwrap().revision
        );
    }

    #[test]
    fn invalid_role_inputs_and_memberships_fail_preflight() {
        for (from, to) in [
            ("schema = 1", "schema = 99"),
            ("roles.executor", "roles.observer"),
            ("global/roles/executor.md", "../outside.md"),
            ("global/roles/executor.md", "global/missing.md"),
            ("global/roles/executor.md", "global/roles/lead.md"),
            ("['verify']", "[]"),
            ("['coordinate']", "['verify', 'coordinate']"),
            ("['verify']", "['verify', 'missing']"),
        ] {
            let root = fixture();
            let path = root.path().join("global/roles.toml");
            let original = fs::read_to_string(&path).unwrap();
            fs::write(&path, original.replace(from, to)).unwrap();
            assert!(
                resolve(root.path(), Role::Lead).is_err(),
                "accepted {from} => {to}"
            );
        }
    }

    #[test]
    fn invalid_retained_configuration_is_not_silently_replaced() {
        let root = fixture();
        let role = resolve(root.path(), Role::Executor).unwrap();
        for input in [
            "skills = 'invalid'",
            "skills.config = 'invalid'",
            "developer_instructions = false",
        ] {
            let config = toml::from_str(input).unwrap();
            assert!(role.native_overrides(&config).is_err(), "accepted {input}");
        }
    }

    #[test]
    fn native_overrides_preserve_foreign_settings_and_explicit_disables() {
        let root = fixture();
        let role = resolve(root.path(), Role::Executor).unwrap();
        let verify = role
            .skills
            .iter()
            .find(|skill| skill.name == "verify")
            .unwrap();
        let coordinate = role
            .skills
            .iter()
            .find(|skill| skill.name == "coordinate")
            .unwrap();
        let mut retained: toml::Table = toml::from_str(
            "developer_instructions = 'User guidance'\nmodel = 'user-model'\nmodel_reasoning_effort = 'user-effort'\n[[skills.config]]\npath = '/foreign/SKILL.md'\nenabled = true\ncustom = 'preserved'"
        ).unwrap();
        let entries = retained
            .get_mut("skills")
            .unwrap()
            .get_mut("config")
            .unwrap()
            .as_array_mut()
            .unwrap();
        for (skill, enabled) in [(verify, false), (coordinate, true)] {
            entries.push(toml::Value::Table(toml::Table::from_iter([
                (
                    "path".into(),
                    skill.path.to_string_lossy().into_owned().into(),
                ),
                ("enabled".into(), enabled.into()),
            ])));
        }
        let before = retained.clone();
        let overrides = role.native_overrides(&retained).unwrap();
        assert_eq!(retained, before);
        assert_eq!(overrides.len(), 2);
        assert_eq!(
            overrides["developer_instructions"].as_str().unwrap(),
            "Shared developer\n\nExecutor responsibility\n\nUser guidance"
        );
        let config = overrides["skills"]["config"].as_array().unwrap();
        assert_eq!(config.len(), 3);
        assert_eq!(config[0], before["skills"]["config"][0]);
        assert_eq!(config[1]["enabled"].as_bool(), Some(false));
        assert_eq!(config[2]["enabled"].as_bool(), Some(false));
        let empty = role.native_overrides(&toml::Table::new()).unwrap();
        let entries = empty["skills"]["config"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(names_path(&entries[0], &coordinate.path));
    }
}
