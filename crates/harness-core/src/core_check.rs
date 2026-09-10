//! Read-only native schema-2 core check. No publication, build, download or
//! feature edit is performed; missing and legacy state is preserved.
#![cfg(windows)]

use crate::{
    agent_config, build_identity,
    config_file::ConfigSnapshot,
    core_runtime,
    installation_lock::InstallationLocks,
    installation_metadata::{InstallationMetadata, InstalledLink},
    installation_path::PathChange,
    installation_state::{PathScope, normal},
    inventory, native_launcher,
    registration::LinkType,
    registration_native as native,
};
use serde::Serialize;
use std::{collections::BTreeSet, fs, io, io::Read, path::Path, time::Duration};

#[derive(Serialize)]
pub struct CheckReport {
    pub status: &'static str,
    pub model_calls: u32,
    pub links: usize,
    pub runtime: core_runtime::RuntimeReceipt,
}

/// Observe an existing native schema-2 core installation. Pending recovery is
/// refused before any original CLI launch. User PATH presence is registry text
/// only; it does not prove a PowerShell function, alias or fresh-terminal lookup.
pub fn check(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
    timeout: Duration,
) -> io::Result<CheckReport> {
    for path in [codex_home, user_home, dependency_user_home] {
        normal(path)?;
    }
    if timeout.is_zero() || timeout > Duration::from_secs(300) {
        return Err(io::Error::other(
            "native core check requires a finite timeout of at most 300 seconds",
        ));
    }
    let _locks = InstallationLocks::acquire(user_home, dependency_user_home)?;
    refuse_pending(codex_home)?;
    let metadata = read_native(codex_home, user_home, dependency_user_home)?;
    let settings = metadata.settings();
    inspect_links(metadata.links())?;
    let (launch, launch_snapshot) = inspect_native_launch(&metadata)?;
    let build = launch
        .build
        .as_ref()
        .ok_or_else(|| disconnected("native-launch registration is not schema 2"))?;
    normal(build)?;
    require_native_commands(metadata.links(), build)?;
    build_identity::ordinary(&settings.codex_command)?;
    if !settings
        .codex_command
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        return Err(disconnected(
            "registered command is not a native executable",
        ));
    }
    if !same_path(&launch.upstream.executable, &settings.codex_command)? {
        return Err(disconnected(
            "native-launch upstream does not match the registered command",
        ));
    }
    let upstream_hash = build_identity::hash_file(&settings.codex_command)?;
    if launch.upstream.sha256 != upstream_hash {
        return Err(disconnected(
            "native-launch upstream hash does not match the original executable",
        ));
    }
    if settings.codex_command.canonicalize()? == build.join("codex.exe").canonicalize()? {
        return Err(disconnected(
            "native-launch registration is not schema 2 core config",
        ));
    }
    require_path(
        settings.path_scope,
        &settings.codex_home.join("harness/bin"),
    )?;
    require_current_source(build, &settings.source_root)?;
    let inventory = inventory::read(&settings.source_root, codex_home, user_home)?;
    require_data_links(metadata.links(), &inventory.links)?;
    agent_config::check(codex_home, &inventory.agents)?;
    let instructions =
        read_instructions(&settings.source_root.join(&inventory.manifest.instructions))?;
    let runtime = core_runtime::verify(
        &settings.codex_command,
        &codex_home.join("harness/bin/codex.exe"),
        codex_home,
        &instructions,
        timeout,
    )?;
    metadata.verify_unchanged()?;
    inspect_links(metadata.links())?;
    launch_snapshot.verify_unchanged()?;
    require_path(
        settings.path_scope,
        &settings.codex_home.join("harness/bin"),
    )?;
    require_current_source(build, &settings.source_root)?;
    let current = inventory::read(&settings.source_root, codex_home, user_home)?;
    require_data_links(metadata.links(), &current.links)?;
    if read_instructions(&settings.source_root.join(&current.manifest.instructions))?
        != instructions
    {
        return Err(disconnected(
            "instruction source changed during runtime observation",
        ));
    }
    let (launch_after, _) = inspect_native_launch(&metadata)?;
    if launch_after.schema != launch.schema
        || launch_after.build != launch.build
        || launch_after.upstream.executable != launch.upstream.executable
        || launch_after.upstream.sha256 != launch.upstream.sha256
    {
        return Err(disconnected(
            "native-launch registration changed during runtime observation",
        ));
    }
    Ok(CheckReport {
        status: "connected",
        model_calls: 0,
        links: metadata.links().len(),
        runtime,
    })
}

fn disconnected(message: &str) -> io::Error {
    io::Error::other(format!("{message}; preserving it"))
}

fn refuse_pending(codex_home: &Path) -> io::Result<()> {
    for relative in [
        "harness/native-registration/journal.json",
        "harness/native-registration/commit.json",
        "harness/native-registration/complete.json",
        "harness/token-workflow-pending.json",
        "harness/pending.json",
        "AGENTS.override.md",
    ] {
        let path = codex_home.join(relative);
        inventory::ordinary_parents(&path)?;
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(_) => {
                return Err(disconnected(
                    "pending journal, commit, completion or legacy recovery is present",
                ));
            }
        }
    }
    Ok(())
}

fn read_native(
    codex_home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> io::Result<InstallationMetadata> {
    let path = codex_home.join("harness/installation.json");
    inventory::ordinary_parents(&path)?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(disconnected(
                "native schema-2 core installation is not connected",
            ));
        }
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let snapshot = ConfigSnapshot::read(&path)?;
    let header: serde_json::Value = serde_json::from_slice(snapshot.contents())
        .map_err(|_| disconnected("native core installation metadata is not connected"))?;
    match header.get("schemaVersion").and_then(|value| value.as_u64()) {
        Some(1) => Err(disconnected(
            "legacy installation needs an explicit native upgrade",
        )),
        Some(2) => InstallationMetadata::read(codex_home, user_home, dependency_user_home)?
            .ok_or_else(|| disconnected("native schema-2 core installation is not connected")),
        _ => Err(disconnected(
            "native core installation metadata is not a connected schema-2 record",
        )),
    }
}

fn inspect_links(links: &[InstalledLink]) -> io::Result<()> {
    for link in links {
        inspect_link(link)?;
    }
    Ok(())
}

fn inspect_link(link: &InstalledLink) -> io::Result<()> {
    let directory = matches!(link.object.link_type, LinkType::Directory);
    let guard = match native::verified_link(&link.object.path, &link.object.target, directory) {
        Ok(guard) => guard,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(disconnected("recorded core link is missing"));
        }
        Err(error) => return Err(error),
    };
    if guard.object_identity()? != link.object.identity {
        return Err(disconnected(
            "recorded core link identity, type or target does not match the live object",
        ));
    }
    Ok(())
}

fn inspect_native_launch(
    metadata: &InstallationMetadata,
) -> io::Result<(native_launcher::Registration, ConfigSnapshot)> {
    let link = metadata
        .links()
        .iter()
        .find(|link| link.kind == "native-launch" && link.name == "codex")
        .ok_or_else(|| disconnected("native-launch registration is missing"))?;
    inspect_link(link)?;
    let snapshot = ConfigSnapshot::read(&link.object.target)?;
    let launch: native_launcher::Registration = serde_json::from_slice(snapshot.contents())
        .map_err(|_| disconnected("native-launch registration is invalid"))?;
    if launch.schema != 2 || launch.state.is_some() || launch.build.is_none() {
        return Err(disconnected("native-launch registration is not schema 2"));
    }
    Ok((launch, snapshot))
}

fn require_native_commands(links: &[InstalledLink], build: &Path) -> io::Result<()> {
    let expected: BTreeSet<_> = build_identity::BINARIES
        .iter()
        .map(|name| name.trim_end_matches(".exe").to_owned())
        .collect();
    let mut found = BTreeSet::new();
    for link in links.iter().filter(|link| link.kind == "native-command") {
        if !found.insert(link.name.clone()) {
            return Err(disconnected("native-command records are duplicated"));
        }
        let expected_target = build.join(format!("{}.exe", link.name));
        if !same_path(&link.object.target, &expected_target)? {
            return Err(disconnected(
                "native-command target does not match the registered build",
            ));
        }
    }
    if found != expected {
        return Err(disconnected(
            "native core installation is missing recorded native commands",
        ));
    }
    let diagnostic: Vec<_> = links
        .iter()
        .filter(|link| link.kind == "native-diagnostic")
        .collect();
    if diagnostic.len() != 1
        || diagnostic[0].name != "codex-harness-check"
        || !same_path(
            &diagnostic[0].object.target,
            &build.join("codex-harness.exe"),
        )?
    {
        return Err(disconnected(
            "native diagnostic command is missing or selects another manager",
        ));
    }
    Ok(())
}

fn require_data_links(links: &[InstalledLink], desired: &[inventory::Link]) -> io::Result<()> {
    use crate::installation_state::key;
    for expected in desired {
        let path = key(&expected.destination)?;
        let recorded = links
            .iter()
            .find(|link| key(&link.object.path).ok().as_deref() == Some(path.as_str()))
            .ok_or_else(|| disconnected("current manifest data link is not registered"))?;
        if recorded.kind != expected.kind
            || recorded.name != expected.name
            || !same_path(&recorded.object.target, &expected.source)?
        {
            return Err(disconnected(
                "current manifest data link differs from the installed record",
            ));
        }
    }
    Ok(())
}

fn require_path(scope: PathScope, bin: &Path) -> io::Result<()> {
    let (_, missing) = PathChange::prepend(scope, bin)?;
    if missing {
        return Err(disconnected(match scope {
            PathScope::User => "User PATH does not contain harness/bin",
            PathScope::Process => "Process PATH does not contain harness/bin",
        }));
    }
    Ok(())
}

fn require_current_source(build: &Path, source: &Path) -> io::Result<()> {
    let check = build_identity::check(build, Some(source));
    if !check.runtime_allowed {
        return Err(disconnected(
            "current source is unavailable or the native build is not healthy",
        ));
    }
    let record = build_identity::read_record(build)?;
    if !same_path(&record.source_root, source)? {
        return Err(disconnected(
            "registered source does not match the native build record",
        ));
    }
    Ok(())
}

fn read_instructions(path: &Path) -> io::Result<Vec<u8>> {
    build_identity::ordinary(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err(disconnected(
            "current instruction source is missing or too large",
        ));
    }
    Ok(bytes)
}

fn same_path(left: &Path, right: &Path) -> io::Result<bool> {
    Ok(left.canonicalize()? == right.canonicalize()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        environment_path::UserPathSnapshot,
        installation_metadata::{Previous, Settings, encode},
        installation_state::PathScope,
        inventory::{Connection, Link},
        registration::{Registration, metadata::MetadataDestination},
    };
    use serde_json::json;
    use std::{collections::BTreeMap, path::PathBuf};

    struct Fixture {
        root: PathBuf,
        settings: Settings,
        links: Vec<Link>,
        launch_source: PathBuf,
    }

    impl Fixture {
        fn new(path_scope: PathScope) -> Self {
            let root = tempfile::Builder::new()
                .prefix("native-core-check-")
                .tempdir()
                .unwrap()
                .keep();
            let source = root.join("source");
            let build = root.join("build");
            let home = root.join("codex");
            let user = root.join("user");
            let dependency = root.join("dependency");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir_all(&build).unwrap();
            fs::write(
                source.join("instructions.md"),
                b"Owned native core check.\n",
            )
            .unwrap();
            fs::write(source.join("profile.toml"), b"approval_policy = 'never'\n").unwrap();
            for binary in build_identity::BINARIES {
                fs::write(build.join(binary), binary.as_bytes()).unwrap();
            }
            let upstream = root.join("upstream.exe");
            fs::write(&upstream, b"explicit fixture upstream").unwrap();
            let launch_source = root.join("launch.json");
            fs::write(
                &launch_source,
                serde_json::to_vec_pretty(&native_launcher::Registration {
                    schema: 2,
                    state: None,
                    build: Some(build.clone()),
                    upstream: native_launcher::Upstream {
                        executable: upstream.clone(),
                        sha256: build_identity::hash_file(&upstream).unwrap(),
                        package: None,
                    },
                })
                .unwrap(),
            )
            .unwrap();
            let mut links = vec![
                Link {
                    kind: "instructions".into(),
                    name: "AGENTS".into(),
                    source: source.join("instructions.md"),
                    destination: home.join("AGENTS.md"),
                    connection: Connection::Missing,
                },
                Link {
                    kind: "profile".into(),
                    name: "harness".into(),
                    source: source.join("profile.toml"),
                    destination: home.join("harness.config.toml"),
                    connection: Connection::Missing,
                },
                Link {
                    kind: "native-launch".into(),
                    name: "codex".into(),
                    source: launch_source.clone(),
                    destination: home.join("harness/native-launch.json"),
                    connection: Connection::Missing,
                },
                Link {
                    kind: "native-diagnostic".into(),
                    name: "codex-harness-check".into(),
                    source: build.join("codex-harness.exe"),
                    destination: home.join("harness/bin/codex-harness-check.exe"),
                    connection: Connection::Missing,
                },
            ];
            for binary in build_identity::BINARIES {
                links.push(Link {
                    kind: "native-command".into(),
                    name: binary.trim_end_matches(".exe").into(),
                    source: build.join(binary),
                    destination: home.join("harness/bin").join(binary),
                    connection: Connection::Missing,
                });
            }
            Self {
                settings: Settings {
                    source_root: source,
                    codex_home: home,
                    user_home: user,
                    dependency_user_home: dependency,
                    codex_command: upstream,
                    path_scope,
                    path_added: false,
                    versions: BTreeMap::new(),
                },
                launch_source,
                links,
                root,
            }
        }

        fn metadata_path(&self) -> PathBuf {
            self.settings.codex_home.join("harness/installation.json")
        }

        fn publish(&self, path: bool) {
            let reg = Registration::open(&self.root.join("journal-state")).unwrap();
            let destination = MetadataDestination::absent(&self.metadata_path()).unwrap();
            let settings = self.settings.clone();
            let links = self.links.clone();
            if path {
                let change = UserPathSnapshot::for_registration()
                    .unwrap()
                    .prepend(&self.settings.codex_home.join("harness/bin"))
                    .unwrap()
                    .0;
                reg.apply_installation(
                    &links,
                    &[],
                    &[],
                    &[],
                    destination,
                    |view| encode(&settings, &links, view, Previous::Fresh),
                    Some(change.into()),
                    || Ok(()),
                )
                .unwrap();
            } else {
                reg.apply_with_metadata(&links, &[], &[], &[], destination, |view| {
                    encode(&settings, &links, view, Previous::Fresh)
                })
                .unwrap();
            }
            assert!(reg.finish(&self.metadata_path()).unwrap().committed);
        }

        fn check(&self) -> io::Result<CheckReport> {
            check(
                &self.settings.codex_home,
                &self.settings.user_home,
                &self.settings.dependency_user_home,
                Duration::from_secs(45),
            )
        }
    }

    #[test]
    fn changed_or_unregistered_manifest_data_requires_update() {
        let fixture = Fixture::new(PathScope::User);
        fixture.publish(false);
        let metadata = read_native(
            &fixture.settings.codex_home,
            &fixture.settings.user_home,
            &fixture.settings.dependency_user_home,
        )
        .unwrap();
        require_data_links(metadata.links(), &fixture.links).unwrap();
        let mut changed = fixture.links[0].clone();
        changed.source = fixture.settings.source_root.join("new-instructions.md");
        fs::write(&changed.source, b"new instructions").unwrap();
        assert!(require_data_links(metadata.links(), &[changed]).is_err());
        let mut missing = fixture.links[0].clone();
        missing.destination = fixture.settings.codex_home.join("new-data");
        assert!(require_data_links(metadata.links(), &[missing]).is_err());
        metadata.verify_unchanged().unwrap();
    }

    #[test]
    fn missing_installation_is_not_connected_and_creates_no_homes() {
        let root = tempfile::Builder::new()
            .prefix("native-core-check-missing-")
            .tempdir()
            .unwrap()
            .keep();
        let home = root.join("codex");
        let user = root.join("user");
        let dependency = root.join("dependency");
        let error = check(&home, &user, &dependency, Duration::from_secs(45))
            .err()
            .expect("must reject missing installation");
        assert!(error.to_string().contains("not connected"));
        assert!(!home.exists());
        assert!(!user.exists());
        assert!(!dependency.exists());
    }

    #[test]
    fn pending_recovery_is_refused_before_cli_and_preserved() {
        let fixture = Fixture::new(PathScope::User);
        fixture.publish(false);
        let journal = fixture
            .settings
            .codex_home
            .join("harness/native-registration/journal.json");
        fs::create_dir_all(journal.parent().unwrap()).unwrap();
        fs::write(&journal, b"pending-owned-journal").unwrap();
        let before = fs::read(fixture.metadata_path()).unwrap();
        let error = fixture.check().err().expect("must reject pending state");
        assert!(error.to_string().contains("pending"));
        assert_eq!(fs::read(&journal).unwrap(), b"pending-owned-journal");
        assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
    }

    #[test]
    fn legacy_metadata_is_not_connected_and_is_preserved() {
        let root = tempfile::Builder::new()
            .prefix("native-core-check-legacy-")
            .tempdir()
            .unwrap()
            .keep();
        let home = root.join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        let metadata = home.join("harness/installation.json");
        let bytes =
            serde_json::to_vec(&json!({"schemaVersion":1,"sourceRoot":root.join("source")}))
                .unwrap();
        fs::write(&metadata, &bytes).unwrap();
        let error = check(
            &home,
            &root.join("user"),
            &root.join("dependency"),
            Duration::from_secs(45),
        )
        .err()
        .expect("must reject legacy state");
        assert!(error.to_string().contains("legacy"));
        assert_eq!(fs::read(&metadata).unwrap(), bytes);
    }

    #[test]
    fn missing_process_path_preserves_metadata() {
        crate::environment_path::with_test_registry(|| {
            let fixture = Fixture::new(PathScope::Process);
            fixture.publish(false);
            let before = fs::read(fixture.metadata_path()).unwrap();
            let error = fixture.check().err().expect("must reject Process PATH");
            assert!(error.to_string().contains("Process PATH"));
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
        });
    }

    #[test]
    fn user_path_presence_uses_test_registry_not_shell_resolution() {
        crate::environment_path::with_test_registry(|| {
            let fixture = Fixture::new(PathScope::User);
            fixture.publish(false);
            let before = fs::read(fixture.metadata_path()).unwrap();
            let error = fixture.check().err().expect("must reject missing PATH");
            assert!(error.to_string().contains("User PATH"));
            assert!(!error.to_string().contains("alias"));
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
            let fixture = Fixture::new(PathScope::User);
            fixture.publish(true);
            let after_path = fs::read(fixture.metadata_path()).unwrap();
            let error = fixture.check().err().expect("must reject missing build");
            assert!(!error.to_string().contains("User PATH"));
            assert!(error.to_string().contains("current source"));
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), after_path);
        });
    }

    #[test]
    fn native_launch_json_must_match_the_original_upstream_hash() {
        crate::environment_path::with_test_registry(|| {
            let fixture = Fixture::new(PathScope::User);
            fixture.publish(false);
            let before = fs::read(fixture.metadata_path()).unwrap();
            let original = fs::read(&fixture.launch_source).unwrap();
            let mut launch: native_launcher::Registration =
                serde_json::from_slice(&original).unwrap();
            launch.upstream.sha256 = "0".repeat(64);
            fs::write(
                &fixture.launch_source,
                serde_json::to_vec_pretty(&launch).unwrap(),
            )
            .unwrap();
            let error = fixture.check().err().expect("must reject altered launch");
            assert!(error.to_string().contains("upstream hash"));
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
            assert_ne!(fs::read(&fixture.launch_source).unwrap(), original);
        });
    }

    #[test]
    fn recorded_link_absence_is_not_connected() {
        crate::environment_path::with_test_registry(|| {
            let fixture = Fixture::new(PathScope::User);
            fixture.publish(false);
            let before = fs::read(fixture.metadata_path()).unwrap();
            fs::remove_file(fixture.settings.codex_home.join("AGENTS.md")).unwrap();
            let error = fixture.check().err().expect("must reject missing link");
            assert!(
                error.to_string().contains("missing")
                    || error.to_string().contains("changed")
                    || error.to_string().contains("not")
            );
            assert_eq!(fs::read(fixture.metadata_path()).unwrap(), before);
            assert!(!fixture.settings.codex_home.join("AGENTS.md").exists());
        });
    }
}
