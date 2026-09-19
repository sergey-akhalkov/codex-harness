//! Next-action observation before delivery is complete. Tokens are not refunded.

use crate::identity::Revision;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Awareness {
    pub identity: Revision,
    pub observed: bool,
    pub stale: bool,
    pub tokens_refunded: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

pub fn after_publish(identity: Revision) -> Awareness {
    Awareness {
        identity,
        observed: false,
        stale: false,
        tokens_refunded: false,
        enabled: true,
    }
}

pub fn observe(awareness: &mut Awareness, current_revision: &str, retired: bool) {
    awareness.stale = retired || current_revision != awareness.identity.revision;
    awareness.observed = !awareness.stale;
    awareness.tokens_refunded = false;
}

pub fn live_read(identity: Revision, retired: bool) -> Awareness {
    live_read_enabled(identity, retired, true)
}

pub fn live_read_enabled(identity: Revision, retired: bool, enabled: bool) -> Awareness {
    let current = identity.revision.clone();
    let mut awareness = after_publish(identity);
    observe(&mut awareness, &current, retired || !enabled);
    awareness.enabled = enabled;
    awareness
}

pub fn delivery_complete(awareness: &Awareness) -> bool {
    awareness.observed && !awareness.stale && !awareness.tokens_refunded
}

pub fn config_disables(config: &Path, skill: &Path) -> bool {
    let Ok(text) = fs::read_to_string(config) else {
        return false;
    };
    let skill = skill.to_string_lossy();
    let skill_md = Path::new(skill.as_ref()).join("SKILL.md");
    let skill_md = skill_md.to_string_lossy();
    text.split("[[skills.config]]").skip(1).any(|block| {
        let path_hit = block.lines().any(|line| {
            let line = line.trim();
            if !line.starts_with("path") {
                return false;
            }
            let hay = line.replace("\\\\", "\\").replace('/', "\\");
            let dir = skill.replace('/', "\\");
            let file = skill_md.replace('/', "\\");
            let hay = hay.to_ascii_lowercase();
            hay.contains(&dir.to_ascii_lowercase()) || hay.contains(&file.to_ascii_lowercase())
        });
        let disabled = block.lines().any(|line| {
            let line = line.trim();
            line.starts_with("enabled") && line.contains("false")
        });
        path_hit && disabled
    })
}

pub fn child_attribution(used_revision: &str, current_revision: &str) -> &'static str {
    if used_revision == current_revision {
        "current"
    } else {
        "prior"
    }
}

pub fn save(path: &Path, awareness: &Awareness) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(awareness).map_err(io::Error::other)?,
    )
}

pub fn load(path: &Path) -> io::Result<Awareness> {
    serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
}

pub fn report(awareness: &Awareness) -> serde_json::Value {
    serde_json::json!({
        "name": awareness.identity.name,
        "path": awareness.identity.path,
        "revision": awareness.identity.revision,
        "operation": awareness.identity.operation,
        "observed": awareness.observed,
        "stale": awareness.stale,
        "tokens_refunded": awareness.tokens_refunded,
        "delivery_complete": delivery_complete(awareness),
        "enabled": awareness.enabled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(rev: &str, op: &str) -> Revision {
        Revision {
            name: "demo".into(),
            path: "skills/demo".into(),
            revision: rev.into(),
            operation: op.into(),
        }
    }

    #[test]
    fn publish_is_not_delivery_until_the_current_revision_is_observed() {
        let mut a = after_publish(id("v2", "update"));
        assert!(!delivery_complete(&a));
        observe(&mut a, "v1", false);
        assert!(a.stale);
        assert!(!delivery_complete(&a));
        observe(&mut a, "v2", false);
        assert!(delivery_complete(&a));
        assert!(!a.tokens_refunded);
        observe(&mut a, "v2", true);
        assert!(a.stale);
        assert!(!delivery_complete(&a));
    }

    #[test]
    fn identity_read_observes_the_live_revision_and_never_refunds_tokens() {
        let awareness = live_read(id("v2", "update"), false);
        assert!(delivery_complete(&awareness));
        assert!(!awareness.tokens_refunded);
        let retired = live_read(id("v2", "retire"), true);
        assert!(retired.stale);
        assert!(!delivery_complete(&retired));
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join("awareness.json");
        let pending = after_publish(id("v3", "publish"));
        save(&journal, &pending).unwrap();
        let loaded = load(&journal).unwrap();
        assert!(!delivery_complete(&loaded));
        assert_eq!(report(&loaded)["tokens_refunded"], false);
    }

    #[test]
    fn running_child_keeps_prior_attribution_when_the_revision_changes() {
        assert_eq!(child_attribution("v1", "v1"), "current");
        assert_eq!(child_attribution("v1", "v2"), "prior");
    }

    #[test]
    fn disabled_package_is_not_delivery_and_does_not_refund_tokens() {
        let a = live_read_enabled(id("v2", "observe"), false, false);
        assert!(!a.enabled);
        assert!(a.stale);
        assert!(!delivery_complete(&a));
        assert!(!a.tokens_refunded);
    }
}
