//! Failure-injection probe for skill-package publication. This is the primitive
//! later tasks reuse: journaled Registration, live source links, recover.
use super::*;
use crate::{
    config_create::ConfigCreation, config_file::ConfigSnapshot, inventory::Connection,
    inventory::Link,
};
use std::fs;

fn skill_link(source: &Path, destination: &Path) -> Link {
    Link {
        kind: "skill".into(),
        name: "owned-probe".into(),
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
        connection: Connection::Missing,
    }
}

#[test]
fn whole_package_link_and_descriptor_switch_recover_to_complete_revisions() {
    let root = tempfile::Builder::new()
        .prefix("skill-package-publish-")
        .tempdir()
        .unwrap();
    let source = root.path().join("canonical/owned-probe");
    let destination = root.path().join("user/.agents/skills/owned-probe");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(
        source.join("SKILL.md"),
        b"---\nname: owned-probe\ndescription: v1\n---\nbody-v1\n",
    )
    .unwrap();
    fs::write(source.join("resource.txt"), b"immutable-v1").unwrap();
    fs::write(
        root.path().join("user/.agents/skills/foreign.txt"),
        b"keep-foreign",
    )
    .unwrap();
    let resource_path = source.join("resource.txt");
    let descriptor = ConfigSnapshot::read(&source.join("SKILL.md")).unwrap();
    let v2 = b"---\nname: owned-probe\ndescription: v2\n---\nbody-v2\n";
    let reg = Registration::open(&root.path().join("state")).unwrap();
    let link = skill_link(&source, &destination);
    let mut count = 0;
    let failed = reg.apply_inner(
        std::slice::from_ref(&link),
        &[descriptor.plan_replace(v2).unwrap()],
        &[],
        &[],
        || {
            count += 1;
            if count == 1 {
                Err(io::Error::other("injected failure after descriptor"))
            } else {
                Ok(())
            }
        },
    );
    assert!(failed.is_err());
    assert_eq!(fs::read(&resource_path).unwrap(), b"immutable-v1");
    assert_eq!(
        fs::read(root.path().join("user/.agents/skills/foreign.txt")).unwrap(),
        b"keep-foreign"
    );
    let undo = reg.recover().unwrap();
    assert!(
        !undo.restored.is_empty()
            || !destination.exists()
            || fs::read(source.join("SKILL.md")).unwrap()
                == b"---\nname: owned-probe\ndescription: v1\n---\nbody-v1\n".to_vec()
    );
    assert_eq!(fs::read(&resource_path).unwrap(), b"immutable-v1");
    assert_eq!(
        fs::read(root.path().join("user/.agents/skills/foreign.txt")).unwrap(),
        b"keep-foreign"
    );
    let descriptor = ConfigSnapshot::read(&source.join("SKILL.md")).unwrap();
    let creation = ConfigCreation::new(&source.join("extra.txt"), b"new-file").unwrap();
    reg.apply_with_files(
        &[skill_link(&source, &destination)],
        &[descriptor.plan_replace(v2).unwrap()],
        &[creation],
    )
    .unwrap();
    assert_eq!(fs::read(source.join("SKILL.md")).unwrap(), v2);
    assert_eq!(fs::read(&resource_path).unwrap(), b"immutable-v1");
    assert_eq!(fs::read(source.join("extra.txt")).unwrap(), b"new-file");
    assert_eq!(fs::read_link(&destination).unwrap(), source);
}

#[test]
fn scoped_new_skill_preserves_foreign_destination_and_busy_lock() {
    let root = tempfile::Builder::new()
        .prefix("skill-scoped-register-")
        .tempdir()
        .unwrap();
    let source = root.path().join("canonical/owned-probe");
    let destination = root.path().join("user/.agents/skills/owned-probe");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(
        source.join("SKILL.md"),
        b"---\nname: owned-probe\ndescription: v1\n---\n",
    )
    .unwrap();
    fs::write(destination.join("foreign.md"), b"keep").unwrap();
    let unrelated = root.path().join("unrelated.toml");
    fs::write(&unrelated, b"keep-config").unwrap();
    let reg = Registration::open(&root.path().join("state")).unwrap();
    let result = reg.apply_with_files(&[skill_link(&source, &destination)], &[], &[]);
    assert!(result.is_err() || fs::read(destination.join("foreign.md")).unwrap() == b"keep");
    assert_eq!(fs::read(&unrelated).unwrap(), b"keep-config");
    drop(reg);
    let lock_path = root.path().join("state/registration.lock");
    if lock_path.exists() {
        let _busy = crate::process::ExclusiveFileLock::try_acquire(&lock_path).unwrap();
        assert!(Registration::open(&root.path().join("state")).is_err());
    }
}

#[test]
fn isolated_lifecycle_links_two_kit_skills_and_keeps_foreign_records() {
    let root = tempfile::Builder::new()
        .prefix("skill-lifecycle-")
        .tempdir()
        .unwrap();
    let source = root.path().join("canonical");
    let user_skills = root.path().join("user/.agents/skills");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&user_skills).unwrap();
    for name in ["skill-evolution", "skills-usage-analysis"] {
        fs::create_dir_all(source.join(name)).unwrap();
        fs::write(
            source.join(name).join("SKILL.md"),
            format!("---\nname: {name}\ndescription: lifecycle\n---\n").as_bytes(),
        )
        .unwrap();
    }
    fs::write(user_skills.join("foreign-project.md"), b"keep-project").unwrap();
    let links = ["skill-evolution", "skills-usage-analysis"].map(|name| Link {
        kind: "skill".into(),
        name: name.into(),
        source: source.join(name),
        destination: user_skills.join(name),
        connection: Connection::Missing,
    });
    let reg = Registration::open(&root.path().join("state")).unwrap();
    if let Err(error) = reg.apply_with_files(&links, &[], &[]) {
        panic!(
            "apply failed: {error}; src={} dest_parent={}",
            source.join("skill-evolution").is_dir(),
            user_skills.is_dir()
        );
    }
    for name in ["skill-evolution", "skills-usage-analysis"] {
        let dest = user_skills.join(name);
        assert!(
            fs::symlink_metadata(&dest)
                .unwrap()
                .file_type()
                .is_symlink(),
            "{name}"
        );
    }
    assert_eq!(
        fs::read(user_skills.join("foreign-project.md")).unwrap(),
        b"keep-project"
    );
    let _ = reg.disconnect();
    assert_eq!(
        fs::read(user_skills.join("foreign-project.md")).unwrap(),
        b"keep-project"
    );
}
