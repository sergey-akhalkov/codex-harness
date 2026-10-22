//! A synthetic installed kit build as the managed role preflight reads it:
//! the recorded build resolves its owned role inputs from the kit checkout,
//! which is a different concept from the consuming checkout a dispatch
//! targets. Checks reuse it to refuse or compose responsibility roles without
//! building the kit or calling a model.
//!
//! The kit classifies one shared skill and one lead-owned skill, so a
//! composed role's memberships are observable: the executor role receives the
//! lead skill as an explicit disablement and the shared skill stays allowed.

use harness_core::build_identity::{self, BuildRecord};
use harness_core::native_launcher::{Registration, Upstream};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// The owned kit checkout: manifest, profile, common instructions, both role
/// bodies, the role descriptor and every managed skill classified.
pub fn kit_source(root: &Path) -> PathBuf {
    let source = root.join("kit");
    for dir in [
        source.join("global/agents"),
        source.join("skills/shared"),
        source.join("skills/coordinate"),
        source.join("crates/one/src"),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    for file in ["Cargo.toml", "Cargo.lock", "crates/one/src/lib.rs"] {
        fs::write(source.join(file), b"fixture").unwrap();
    }
    fs::write(
        source.join("global/profile.toml"),
        "approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\n",
    )
    .unwrap();
    fs::write(
        source.join("global/kit.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "profile_name": "harness",
            "profile": "global/profile.toml",
            "instructions": "global/instructions.md",
            "skills": "skills",
            "agents": "global/agents",
            "hooks": "global/hooks.json",
            "token_hooks": "global/token-hooks.json"
        }))
        .unwrap(),
    )
    .unwrap();
    for (path, body) in [
        ("global/instructions.md", "Common fixture safeguards"),
        ("global/lead.md", "Fixture lead responsibility"),
        ("global/executor.md", "Fixture executor responsibility"),
        (
            "skills/shared/SKILL.md",
            "---\nname: shared\ndescription: fixture\n---\nFixture shared skill",
        ),
        (
            "skills/coordinate/SKILL.md",
            "---\nname: coordinate\ndescription: fixture\n---\nFixture lead skill",
        ),
        (
            "global/roles.toml",
            "schema = 1\nshared_skills = ['shared']\n[roles.lead]\ninstructions = 'global/lead.md'\nskills = ['coordinate']\n[roles.executor]\ninstructions = 'global/executor.md'\nskills = []\n",
        ),
    ] {
        fs::write(source.join(path), body).unwrap();
    }
    source
}

/// One recorded native build directory whose `source_root` is `source`, as an
/// explicit installation records it. The stub binaries satisfy the recorded
/// artifact integrity pass; no launcher process runs in these checks.
pub fn recorded_build(root: &Path, source: &Path) -> PathBuf {
    let build = root.join("native-build");
    fs::create_dir_all(&build).unwrap();
    for binary in build_identity::BINARIES {
        fs::write(build.join(binary), binary.as_bytes()).unwrap();
    }
    let record = BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.to_path_buf(),
        source: build_identity::source_identity(source).unwrap(),
        rustc: "fixture".into(),
        cargo: "fixture".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries: build_identity::BINARIES
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    build_identity::hash_file(&build.join(name)).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    };
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    build
}

/// The launch registration a Codex home carries after installing `build`.
/// The upstream stub is never launched by the role preflight these checks
/// exercise.
pub fn register(home: &Path, root: &Path, build: &Path) {
    let upstream = root.join("upstream-stub.exe");
    fs::write(&upstream, b"fixture").unwrap();
    fs::create_dir_all(home.join("harness")).unwrap();
    fs::write(
        home.join("harness/native-launch.json"),
        serde_json::to_vec_pretty(&Registration {
            schema: 2,
            state: None,
            build: Some(build.to_path_buf()),
            task_control: false,
            goal_recovery: false,
            upstream: Upstream {
                executable: upstream.clone(),
                sha256: build_identity::hash_file(&upstream).unwrap(),
                package: None,
            },
        })
        .unwrap(),
    )
    .unwrap();
}
