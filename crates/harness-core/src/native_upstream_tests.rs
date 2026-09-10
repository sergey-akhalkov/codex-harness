use super::*;
use serde_json::json;

fn resolve(
    explicit: Option<&Path>,
    path: Option<&OsStr>,
    excluded: &[PathBuf],
) -> io::Result<Upstream> {
    super::resolve(explicit, path, excluded, &ManagerHints::default())
}

struct Fixture {
    _root: tempfile::TempDir,
    prefix: PathBuf,
    package: PathBuf,
    executable: PathBuf,
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

impl Fixture {
    fn new(layout: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("prefix");
        let package = prefix.join("node_modules/@openai/codex");
        write(&package.join("package.json"), &serde_json::to_vec(&json!({"name":"@openai/codex","version":"0.153.4","bin":{"codex":"bin/codex.js"},"packageManager":"pnpm@10"})).unwrap());
        write(
            &package.join("bin/codex.js"),
            b"// Inert external package-description fixture; never executed.\n",
        );
        let platform = match layout {
            "nested" => package.join("node_modules").join(PLATFORM),
            "hoisted" => prefix.join("node_modules").join(PLATFORM),
            "vendor" => package.clone(),
            _ => unreachable!(),
        };
        if layout != "vendor" {
            write(
                &platform.join("package.json"),
                &serde_json::to_vec(&json!({"name":"@openai/codex","version":"0.153.4-win32-x64"}))
                    .unwrap(),
            );
        }
        let executable = platform.join(VENDOR_EXE);
        write(
            &executable,
            b"inert native executable fixture; never executed",
        );
        Self {
            _root: root,
            prefix,
            package,
            executable,
        }
    }
}

#[test]
fn nested_hoisted_and_vendor_packages_resolve_without_execution_or_mutation() {
    for layout in ["nested", "hoisted", "vendor"] {
        let fixture = Fixture::new(layout);
        let manifest = fs::read(fixture.package.join("package.json")).unwrap();
        for input in [
            &fixture.package,
            &fixture.package.join("bin/codex.js"),
            &fixture.executable,
        ] {
            let upstream = resolve(Some(input), None, &[]).unwrap();
            assert_eq!(
                key(&upstream.executable).unwrap(),
                key(&canonical(&fixture.executable).unwrap()).unwrap()
            );
            let package = upstream.package.unwrap();
            assert_eq!(
                package.manifest_sha256,
                build_identity::hash_bytes(&manifest)
            );
            assert_eq!(serde_json::to_value(package.manager).unwrap(), "npm");
            assert_eq!(
                key(&package.root).unwrap(),
                key(&canonical(&fixture.package).unwrap()).unwrap()
            );
        }
        assert_eq!(
            fs::read(fixture.package.join("package.json")).unwrap(),
            manifest
        );
        assert!(!fixture.prefix.join("executed").exists());
    }
}

#[test]
fn resolved_broken_or_wrong_version_platform_does_not_use_vendor_fallback() {
    let fixture = Fixture::new("nested");
    write(&fixture.package.join(VENDOR_EXE), b"wrong fallback");
    fs::remove_file(&fixture.executable).unwrap();
    assert!(resolve(Some(&fixture.package), None, &[]).is_err());
    write(&fixture.executable, b"restored fixture");
    write(
        &fixture
            .executable
            .ancestors()
            .nth(4)
            .unwrap()
            .join("package.json"),
        &serde_json::to_vec(&json!({"name":"@openai/codex","version":"0.999.0-win32-x64"}))
            .unwrap(),
    );
    assert!(resolve(Some(&fixture.package), None, &[]).is_err());
}

#[test]
fn path_is_absolute_ordered_excludes_harness_copies_and_refuses_custom_shims() {
    let root = tempfile::tempdir().unwrap();
    let harness = root.path().join("harness/codex.exe");
    let copy = root.path().join("copy/codex.exe");
    let upstream = root.path().join("original/codex.exe");
    write(&harness, b"harness fixture");
    write(&copy, b"harness fixture");
    write(&upstream, b"original fixture");
    let path = std::env::join_paths([
        Path::new(""),
        Path::new("relative"),
        harness.parent().unwrap(),
        copy.parent().unwrap(),
        upstream.parent().unwrap(),
    ])
    .unwrap();
    let found = resolve(None, Some(&path), std::slice::from_ref(&harness)).unwrap();
    assert_eq!(found.executable, canonical(&upstream).unwrap());
    assert!(found.package.is_none());
    assert!(resolve(Some(&copy), None, std::slice::from_ref(&harness)).is_err());
    let custom = root.path().join("custom/codex.ps1");
    write(
        &custom,
        b"# Inert unknown shim. node_modules/@openai/codex/bin/codex.js\n",
    );
    let path =
        std::env::join_paths([custom.parent().unwrap(), upstream.parent().unwrap()]).unwrap();
    let before = fs::read(&custom).unwrap();
    assert!(resolve(None, Some(&path), &[]).is_err());
    assert_eq!(fs::read(custom).unwrap(), before);
    let dangling = root.path().join("dangling/codex.exe");
    fs::create_dir_all(dangling.parent().unwrap()).unwrap();
    std::os::windows::fs::symlink_file(root.path().join("missing.exe"), &dangling).unwrap();
    let path = std::env::join_paths([
        dangling.parent().unwrap(),
        harness.parent().unwrap(),
        upstream.parent().unwrap(),
    ])
    .unwrap();
    let found = resolve(None, Some(&path), &[dangling, harness]).unwrap();
    assert_eq!(found.executable, canonical(&upstream).unwrap());
}

#[test]
fn metadata_and_path_bounds_refuse_selection() {
    let fixture = Fixture::new("nested");
    write(
        &fixture.package.join("package.json"),
        &vec![b' '; LIMIT + 1],
    );
    assert!(resolve(Some(&fixture.package), None, &[]).is_err());
    let path = std::env::join_paths(std::iter::repeat_n(fixture.prefix.as_path(), 257)).unwrap();
    assert!(resolve(None, Some(&path), &[]).is_err());
}

#[test]
fn manager_ownership_is_bound_to_the_selected_package() {
    for kind in ["pnpm", "vite-plus"] {
        let fixture = Fixture::new("nested");
        if kind == "pnpm" {
            write(
                &fixture.prefix.join("node_modules/.modules.yaml"),
                b"inert ownership marker",
            );
            let found = resolve(Some(&fixture.package), None, &[]).unwrap();
            assert_eq!(
                serde_json::to_value(found.package.unwrap().manager).unwrap(),
                "pnpm"
            );
        } else {
            // Vite+ ownership uses the lexical entrypoint ancestry, with both
            // the old #-ID and current subdirectory shapes.
            for id in ["", "#owned", "owned"] {
                let packages = fixture._root.path().join("packages");
                write(
                    &packages.join("@openai/codex.json"),
                    &serde_json::to_vec(&json!({"name":"@openai/codex","installId":id})).unwrap(),
                );
                let install = if id.starts_with('#') {
                    packages.join(format!("@openai/codex{id}"))
                } else {
                    packages.join("@openai/codex").join(id)
                };
                let link = install.join("lib/node_modules/@openai/codex");
                fs::create_dir_all(link.parent().unwrap()).unwrap();
                std::os::windows::fs::symlink_dir(&fixture.package, &link).unwrap();
                let selected = resolve(Some(&link.join("bin/codex.js")), None, &[]).unwrap();
                assert_eq!(
                    serde_json::to_value(selected.package.unwrap().manager).unwrap(),
                    "vite-plus"
                );
                fs::remove_dir(&link).unwrap();
            }
        }
    }
    let fixture = Fixture::new("vendor");
    let bun = fixture
        ._root
        .path()
        .join(".bun/install/global/node_modules/@openai/codex");
    fs::create_dir_all(bun.parent().unwrap()).unwrap();
    fs::rename(&fixture.package, &bun).unwrap();
    let selected = resolve(Some(&bun), None, &[]).unwrap();
    assert_eq!(
        serde_json::to_value(selected.package.unwrap().manager).unwrap(),
        "bun"
    );
}

#[test]
fn same_path_directory_refuses_distinct_eligible_originals() {
    let first = Fixture::new("vendor");
    let second = Fixture::new("vendor");
    write(&second.executable, b"other inert native executable fixture");
    let directory = first._root.path().join("commands");
    fs::create_dir_all(&directory).unwrap();
    std::os::windows::fs::symlink_file(&first.executable, directory.join("codex.exe")).unwrap();
    std::os::windows::fs::symlink_file(&second.executable, directory.join("codex.cmd")).unwrap();
    let path = std::env::join_paths([directory.as_path()]).unwrap();
    let error = resolve(None, Some(&path), &[]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ambiguous Codex commands in one PATH directory")
    );
}

#[test]
fn manager_hints_use_bun_word_boundaries_and_yield_to_install_ownership() {
    let bun = Fixture::new("nested");
    for (user_agent, exec_path, manager) in [
        ("bun/1.1.0 npm/? node/v22.0.0", "", "bun"),
        ("npm/10.0.0 node/v22.0.0", "C:/tools/bun.exe", "bun"),
        ("foo bun/1.1.0 npm/?", "", "bun"),
        ("npm/10.0.0 bun/? node/v22.0.0", "", "bun"),
        ("abun/1.0 npm/? node/v22.0.0", "", "npm"),
        ("_bun/1.0 npm/? node/v22.0.0", "", "npm"),
    ] {
        let selected = super::resolve(
            Some(&bun.package),
            None,
            &[],
            &ManagerHints {
                user_agent: user_agent.into(),
                exec_path: exec_path.into(),
            },
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(selected.package.unwrap().manager).unwrap(),
            manager
        );
    }

    let pnpm = Fixture::new("nested");
    write(
        &pnpm.prefix.join("node_modules/.modules.yaml"),
        b"inert ownership marker",
    );
    let selected = super::resolve(
        Some(&pnpm.package),
        None,
        &[],
        &ManagerHints {
            user_agent: "bun/1.1.0 npm/? node/v22.0.0".into(),
            exec_path: "C:/tools/bun.exe".into(),
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(selected.package.unwrap().manager).unwrap(),
        "pnpm"
    );

    let vite = Fixture::new("nested");
    let packages = vite._root.path().join("packages");
    write(
        &packages.join("@openai/codex.json"),
        &serde_json::to_vec(&json!({"name":"@openai/codex","installId":""})).unwrap(),
    );
    let link = packages
        .join("@openai/codex")
        .join("lib/node_modules/@openai/codex");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::windows::fs::symlink_dir(&vite.package, &link).unwrap();
    let selected = super::resolve(
        Some(&link.join("bin/codex.js")),
        None,
        &[],
        &ManagerHints {
            user_agent: "bun/1.1.0 npm/? node/v22.0.0".into(),
            exec_path: "C:/tools/bun.exe".into(),
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(selected.package.unwrap().manager).unwrap(),
        "vite-plus"
    );
}

#[test]
fn multiple_packages_owning_one_executable_are_refused() {
    let fixture = Fixture::new("nested");
    write(
        &fixture.prefix.join("package.json"),
        &fs::read(fixture.package.join("package.json")).unwrap(),
    );
    write(
        &fixture.prefix.join("bin/codex.js"),
        b"// Inert external package-description fixture; never executed.\n",
    );
    std::os::windows::fs::symlink_dir(
        fixture.package.join("node_modules").join(PLATFORM),
        fixture.prefix.join("node_modules").join(PLATFORM),
    )
    .unwrap();
    let error = resolve(Some(&fixture.executable), None, &[]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("multiple upstream packages own this executable")
    );
    for root in [&fixture.package, &fixture.prefix] {
        let selected = resolve(Some(root), None, &[]).unwrap();
        assert_eq!(
            key(&selected.executable).unwrap(),
            key(&canonical(&fixture.executable).unwrap()).unwrap()
        );
        assert_eq!(
            key(&selected.package.unwrap().root).unwrap(),
            key(&canonical(root).unwrap()).unwrap()
        );
    }
}

#[test]
#[ignore = "explicit installed CLI package; read-only discovery with no subprocesses"]
fn actual_original_package_and_npm_shims_agree() {
    let root = PathBuf::from(
        std::env::var_os("HARNESS_UPSTREAM_REAL_PACKAGE").expect("explicit installed package root"),
    );
    let prefix = root.parent().unwrap().parent().unwrap().parent().unwrap();
    let direct = resolve(Some(&root), None, &[]).unwrap();
    for path in [
        prefix.join("codex.ps1"),
        prefix.join("codex.cmd"),
        root.join("bin/codex.js"),
        direct.executable.clone(),
    ] {
        let selected = resolve(Some(&path), None, &[]).unwrap();
        assert_eq!(selected.executable, direct.executable);
        assert_eq!(selected.sha256, direct.sha256);
        let package = selected.package.unwrap();
        assert_eq!(
            package.manifest_sha256,
            direct.package.as_ref().unwrap().manifest_sha256
        );
        assert_eq!(serde_json::to_value(package.manager).unwrap(), "npm");
    }
    println!(
        "installed upstream discovery passed: {}",
        direct.executable.display()
    );
}
