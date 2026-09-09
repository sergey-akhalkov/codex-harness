//! Native TOML name-collision preflight; no profiles, hooks or commands execute.
#![cfg(windows)]

use crate::{
    config_file::ConfigSnapshot,
    inventory::{Agent, ordinary_parents},
};
use std::{fs, io, path::Path};

const CONFIG_LIMIT: usize = 1024 * 1024;

pub fn check(codex_home: &Path, agents: &[Agent]) -> io::Result<()> {
    let path = codex_home.join("config.toml");
    ordinary_parents(&path)?;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    let snapshot = ConfigSnapshot::read(&path)?;
    inspect(snapshot.contents(), agents)?;
    snapshot
        .plan_replace(snapshot.contents())?
        .record
        .check_before()
}

fn inspect(bytes: &[u8], agents: &[Agent]) -> io::Result<()> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "base configuration cannot be inspected; preserving it",
        )
    };
    if bytes.len() > CONFIG_LIMIT {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    // Keep parser diagnostics private: their snippets can contain credentials.
    let document: toml::Table = toml::from_str(text).map_err(|_| invalid())?;
    if let Some(value) = document.get("agents") {
        let roles = value.as_table().ok_or_else(invalid)?;
        if agents.iter().any(|agent| roles.contains_key(&agent.name)) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "agent name collision in base configuration; preserving it",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agents() -> Vec<Agent> {
        vec![Agent {
            name: "principal".into(),
            source: "unused-source".into(),
        }]
    }

    #[test]
    fn full_toml_structure_detects_quoted_dotted_and_inline_role_names() {
        for text in [
            "[agents.principal]\nconfig_file = 'private'",
            "['agents'.'principal']\nconfig_file = 'private'",
            "[agents]\nprincipal = { config_file = 'private' }",
            "agents.principal.config_file = 'private'",
            "agents = { principal = { config_file = 'private' } }",
            "agents = {\n principal = { config_file = 'private', },\n }",
            "agents = { \"princi\\u0070al\" = { config_file = 'private' } }",
        ] {
            assert_eq!(
                inspect(text.as_bytes(), &agents()).unwrap_err().kind(),
                io::ErrorKind::AlreadyExists
            );
        }
    }

    #[test]
    fn comments_strings_unrelated_roles_and_agent_settings_are_not_collisions() {
        for text in [
            "# [agents.principal]\nmodel = 'principal'",
            "note = '''\n[agents.principal]\nconfig_file = 'private'\n'''",
            "[agents]\nmax_threads = 4\n[agents.principal_other]\nconfig_file = 'private'",
            "[metadata.agents.principal]\nvalue = 'ordinary data'",
            "agents = {\n max_threads = 4,\n other = { config_file = 'private', },\n }",
        ] {
            inspect(text.as_bytes(), &agents()).unwrap();
        }
    }

    #[test]
    fn malformed_large_and_deep_configurations_fail_privately_with_bounded_parser() {
        for text in [
            "private-secret = {".into(),
            "agents = 12".into(),
            format!("private-secret = '{}'", "x".repeat(CONFIG_LIMIT)),
            format!("private-secret = {}0{}", "[".repeat(200), "]".repeat(200)),
        ] {
            let error = inspect(text.as_bytes(), &agents()).unwrap_err();
            assert!(!error.to_string().contains("private-secret"));
        }
        assert!(inspect(&[0xff], &agents()).is_err());
    }
}
