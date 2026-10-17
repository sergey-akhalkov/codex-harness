//! Resolve owned responsibility guidance without changing routing or shared state.
//! Native callers retain their applicable user/project configuration, then apply
//! only these developer-instruction and skill-selection overrides to one session.
//! A skill this role excludes or a user explicitly disables is also disabled at
//! every validated copy under the repository's other registered worktrees, so a
//! sibling checkout of this kit cannot leak the skill back into the session.
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
    /// Bounded, read-only analysis of an accepted executor conversation.
    ExecutorSelfReflection,
    /// Bounded synthesis over a retained executor reflection and the lead
    /// conversation it belongs to.
    LeadSelfReflection,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Executor => "executor",
            Self::ExecutorSelfReflection => "executor-self-reflection",
            Self::LeadSelfReflection => "lead-self-reflection",
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
    /// Validated copies of the same file at the same relative path under the
    /// repository's other registered worktrees; native discovery may report
    /// one of these instead of the owned source path.
    pub aliases: Vec<PathBuf>,
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
    let root = source_root
        .canonicalize()
        .map_err(|error| invalid(format!("cannot resolve the kit checkout: {error}")))?;
    let source = |relative: &str| -> io::Result<PathBuf> {
        inventory::owned_source(&root, relative)
            .map_err(|error| invalid(format!("cannot resolve {relative}: {error}")))
    };
    let read = |relative: &str| -> io::Result<String> {
        inventory::text(&source(relative)?)
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
    let ordinary = [Role::Lead, Role::Executor];
    let reflection = [Role::ExecutorSelfReflection, Role::LeadSelfReflection];
    let ordinary_present = ordinary
        .iter()
        .all(|role| descriptor.roles.contains_key(role));
    let reflection_present = reflection
        .iter()
        .all(|role| descriptor.roles.contains_key(role));
    let complete = (descriptor.roles.len() == 2 && ordinary_present)
        || (descriptor.roles.len() == 4 && ordinary_present && reflection_present);
    if descriptor.schema != 1 || !complete {
        return Err(invalid(
            "roles.toml must define schema 1 and exactly the lead/executor roles, or all four roles including executor-self-reflection and lead-self-reflection",
        ));
    }
    let common = read(&manifest.instructions)?;
    let common_path = source(&manifest.instructions)?.canonicalize()?;
    let profile: toml::Table =
        toml::from_str(read(&manifest.profile)?.trim_start_matches('\u{feff}'))
            .map_err(|_| invalid("invalid owned profile"))?;
    let developer = instruction(&profile)?;
    let mut bodies = BTreeMap::new();
    let mut role_paths = BTreeSet::new();
    for (key, value) in &descriptor.roles {
        let path = source(&value.instructions)?;
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
    let skill_root = source(&manifest.skills)?;
    let co_roots = co_worktree_roots(&root);
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
        let path = path.canonicalize()?;
        let aliases = skill_aliases(&root, &path, &co_roots);
        skills.push(ManagedSkill {
            name,
            path,
            aliases,
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

/// Other registered checkouts of `root`'s Git repository. A managed session's
/// case is often a sibling worktree of the kit source, and native discovery
/// then reports the same skill file at that checkout's path; role filtering
/// must reach those copies. Best-effort: a missing Git, a foreign source or
/// any failure contributes no aliases and never fails resolution.
fn co_worktree_roots(root: &Path) -> Vec<PathBuf> {
    let Ok(output) = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "list", "--porcelain"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut roots = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(path) = line.trim_end_matches('\r').strip_prefix("worktree ") else {
            continue;
        };
        let mut candidate = PathBuf::from(path);
        if candidate.is_relative() {
            candidate = root.join(candidate);
        }
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if canonical != root {
            roots.push(canonical);
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

/// The validated copies of one skill: the same relative path under each
/// sibling worktree, keeping only canonical ordinary files that stay inside
/// their checkout and differ from the owned source path.
fn skill_aliases(root: &Path, path: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    let Ok(relative) = path.strip_prefix(root) else {
        return Vec::new();
    };
    let mut aliases = Vec::new();
    for other in roots {
        let candidate = other.join(relative);
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if canonical == path || !canonical.starts_with(other) || aliases.contains(&canonical) {
            continue;
        }
        if build_identity::ordinary(&candidate).is_err() {
            continue;
        }
        aliases.push(canonical);
    }
    aliases
}

/// One native disable entry for a skill file, portable across hosts.
fn disabled_entry(path: &Path) -> toml::Value {
    // Native 0.162.1 identifies the SKILL.md file, not its directory.
    // Strip Windows' verbatim prefix for portable native path input.
    let path = path.to_string_lossy();
    let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
    toml::Value::Table(toml::Table::from_iter([
        ("path".into(), path.into()),
        ("enabled".into(), false.into()),
    ]))
}

impl ResolvedRole {
    /// `retained` is the applicable native user/project layer, excluding the
    /// owned portable defaults. Return only native dotted leaf overrides, so
    /// callers cannot replace the surrounding skills table. Foreign skill
    /// entries and their unknown fields survive; allowed skills are never enabled.
    /// Callers include explicit native context in `retained` and place these
    /// composed leaves after the original overrides, before any `--` separator.
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
        for skill in &self.skills {
            let paths = std::iter::once(&skill.path)
                .chain(skill.aliases.iter())
                .collect::<Vec<_>>();
            let explicitly_disabled = entries.iter().any(|entry| {
                entry.get("enabled").and_then(toml::Value::as_bool) == Some(false)
                    && paths.iter().any(|path| names_path(entry, path))
            });
            if skill.allowed && !explicitly_disabled {
                continue;
            }
            for path in paths {
                let mut present = false;
                for entry in entries.iter_mut().filter(|entry| names_path(entry, path)) {
                    let table = entry
                        .as_table_mut()
                        .ok_or_else(|| invalid("skill configuration must be a table"))?;
                    table.insert("enabled".into(), false.into());
                    present = true;
                }
                if !present {
                    entries.push(disabled_entry(path));
                }
            }
        }
        Ok(toml::Table::from_iter([
            (
                "developer_instructions".into(),
                join_instructions(&self.developer_instructions, instruction(retained)?).into(),
            ),
            ("skills.config".into(), entries.into()),
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
    fn missing_role_input_names_the_source_and_recovery_action() {
        let root = fixture();
        fs::remove_file(root.path().join("global/roles/executor.md")).unwrap();
        let error = resolve(root.path(), Role::Lead).unwrap_err().to_string();
        assert!(error.contains("global/roles/executor.md"), "{error}");
        assert!(
            error.contains("repair") || error.contains("update"),
            "{error}"
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
    fn role_selection_never_reenables_a_disable_retained_from_another_layer() {
        let root = fixture();
        let role = resolve(root.path(), Role::Lead).unwrap();
        let path = role.skills[0].path.to_string_lossy();
        let entries = [false, true]
            .into_iter()
            .map(|enabled| {
                toml::Value::Table(toml::Table::from_iter([
                    ("path".into(), path.as_ref().into()),
                    ("enabled".into(), enabled.into()),
                ]))
            })
            .collect::<Vec<_>>();
        let retained = toml::Table::from_iter([(
            "skills".into(),
            toml::Value::Table(toml::Table::from_iter([("config".into(), entries.into())])),
        )]);
        let overrides = role.native_overrides(&retained).unwrap();
        let entries = overrides["skills.config"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| entry["enabled"].as_bool() == Some(false))
        );
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
        let config = overrides["skills.config"].as_array().unwrap();
        assert_eq!(config.len(), 3);
        assert_eq!(config[0], before["skills"]["config"][0]);
        assert_eq!(config[1]["enabled"].as_bool(), Some(false));
        assert_eq!(config[2]["enabled"].as_bool(), Some(false));
        let empty = role.native_overrides(&toml::Table::new()).unwrap();
        let entries = empty["skills.config"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(names_path(&entries[0], &coordinate.path));
    }

    /// A repository-backed fixture: the owned source tree committed, plus one
    /// linked worktree of the same repository carrying identical copies.
    fn linked_fixture() -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
        let root = fixture();
        let parent = tempfile::tempdir().unwrap();
        let linked = parent.path().join("linked");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(root.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=role fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ]);
        let output = std::process::Command::new("git")
            .args(["worktree", "add", "-q", "--detach"])
            .arg(&linked)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git worktree add: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (root, parent, linked)
    }

    #[test]
    fn role_filtering_reaches_identical_copies_in_linked_worktrees() {
        let (root, parent, linked) = linked_fixture();
        let linked = linked.canonicalize().unwrap();
        let executor = resolve(root.path(), Role::Executor).unwrap();
        let coordinate = executor
            .skills
            .iter()
            .find(|skill| skill.name == "coordinate")
            .unwrap();
        let alias = linked
            .join("skills")
            .join("coordinate")
            .join("SKILL.md")
            .canonicalize()
            .unwrap();
        assert_eq!(coordinate.aliases, vec![alias.clone()]);
        let verify = executor
            .skills
            .iter()
            .find(|skill| skill.name == "verify")
            .unwrap();
        assert_eq!(
            verify.aliases,
            vec![
                linked
                    .join("skills")
                    .join("verify")
                    .join("SKILL.md")
                    .canonicalize()
                    .unwrap()
            ]
        );

        // An unrelated same-name tree outside the repository is never adopted.
        let foreign = parent.path().join("foreign");
        let foreign_skill = foreign.join("skills").join("coordinate").join("SKILL.md");
        fs::create_dir_all(foreign_skill.parent().unwrap()).unwrap();
        fs::write(&foreign_skill, "---\nname: coordinate\n---\nbody").unwrap();
        let foreign_skill = foreign_skill.canonicalize().unwrap();
        assert!(!coordinate.aliases.contains(&foreign_skill));

        let retained: toml::Table = toml::from_str(&format!(
            "[[skills.config]]\npath = {}\nenabled = true\ncustom = 'preserved'",
            serde_json::to_string(foreign_skill.to_string_lossy().as_ref()).unwrap()
        ))
        .unwrap();
        let overrides = executor.native_overrides(&retained).unwrap();
        let config = overrides["skills.config"].as_array().unwrap();
        assert_eq!(config.len(), 3);
        assert_eq!(config[0], retained["skills"]["config"][0]);
        assert!(
            config[1..]
                .iter()
                .all(|entry| entry["enabled"].as_bool() == Some(false))
        );
        assert!(
            config[1..]
                .iter()
                .any(|entry| names_path(entry, &coordinate.path))
        );
        assert!(config[1..].iter().any(|entry| names_path(entry, &alias)));
        assert!(
            config[1..]
                .iter()
                .all(|entry| !names_path(entry, &foreign_skill))
        );

        // The lead role keeps every copy enabled: the disablement is role
        // selection, not a blanket same-name or same-path filter.
        let lead = resolve(root.path(), Role::Lead).unwrap();
        let overrides = lead.native_overrides(&toml::Table::new()).unwrap();
        assert_eq!(overrides["skills.config"].as_array().unwrap().len(), 0);
    }
}
