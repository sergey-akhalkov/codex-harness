//! Content identities for explicit native builds; checking never executes a tool.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Component, Path, PathBuf},
};

/// Test-only observation of which verification work a decision performed.
/// Thread-local so parallel test cases never observe another case's reads.
#[cfg(test)]
pub(crate) mod test_probe {
    use std::{cell::RefCell, path::Path};

    thread_local! {
        static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn record(event: impl Into<String>) {
        EVENTS.with(|events| events.borrow_mut().push(event.into()));
    }

    pub(crate) fn hash(path: &Path) {
        record(format!("hash:{}", path.to_string_lossy()));
    }

    pub(crate) fn source_identity() {
        record("source_identity");
    }

    pub(crate) fn take() -> Vec<String> {
        EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
    }
}

pub const SCHEMA: u32 = 1;
pub const BINARIES: &[&str] = &[
    "codex-harness.exe",
    "codex.exe",
    "harness-rtk.exe",
    "harness-inspect.exe",
    "harness-observe.exe",
    "harness-source-check.exe",
    "token-audit.exe",
];
// Live source data resolved by native consumers; never embedded in a binary.
pub const INSPECTION_SCHEMA: &str =
    ".agents/skills/structured-codex-run/assets/inspection.schema.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub sha256: String,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildRecord {
    pub schema: u32,
    pub source_root: PathBuf,
    pub source: SourceIdentity,
    pub rustc: String,
    pub cargo: String,
    pub target: String,
    pub profile: String,
    pub binaries: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Health {
    Healthy,
    SourceStale,
    SourceUnavailable,
    Missing,
    Altered,
    Incompatible,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildCheck {
    pub status: Health,
    pub management_allowed: bool,
    pub runtime_allowed: bool,
    pub serving_allowed: bool,
    pub action: String,
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn hash_file(path: &Path) -> io::Result<String> {
    #[cfg(test)]
    test_probe::hash(path);
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    io::copy(&mut file, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}

/// Do not walk junctions/symlinks into source or foreign caches.
pub fn ordinary(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(
                "reparse point is not an owned native input",
            ));
        }
    }
    if meta.file_type().is_symlink() {
        return Err(io::Error::other("linked native input is unsupported"));
    }
    Ok(())
}

fn collect(root: &Path, path: &Path, files: &mut BTreeMap<String, String>) -> io::Result<()> {
    ordinary(path)?;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            collect(root, &entry.path(), files)?;
        }
    } else {
        let relative = path.strip_prefix(root).map_err(io::Error::other)?;
        // Markdown below src may be an include_str!/include_bytes! input.
        // Non-source documentation stays live without recompilation. The builder
        // checks compiler dep-info and refuses any compiled input omitted here.
        if path.extension().is_some_and(|e| e == "md")
            && !relative.components().any(|part| part.as_os_str() == "src")
        {
            return Ok(());
        }
        // A member lockfile is superseded by the workspace lockfile.
        if path.file_name().is_some_and(|n| n == "Cargo.lock") && path != root.join("Cargo.lock") {
            return Ok(());
        }
        let key = relative
            .to_str()
            .ok_or_else(|| io::Error::other("non-Unicode source path"))?
            .replace('\\', "/");
        files.insert(key, hash_file(path)?);
    }
    Ok(())
}

pub fn source_identity(source: &Path) -> io::Result<SourceIdentity> {
    #[cfg(test)]
    test_probe::source_identity();
    let root = source.canonicalize()?;
    let mut files = BTreeMap::new();
    for required in ["Cargo.toml", "Cargo.lock", "crates"] {
        collect(&root, &root.join(required), &mut files)?;
    }
    for optional in [".cargo", "rust-toolchain", "rust-toolchain.toml"] {
        let path = root.join(optional);
        match fs::symlink_metadata(&path) {
            Ok(_) => collect(&root, &path, &mut files)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    // Cargo also reads ancestor and user configuration. Hash their contents,
    // never store potentially private config bodies in an installation receipt.
    let mut config_roots: Vec<_> = root
        .ancestors()
        .skip(1)
        .map(|path| path.join(".cargo"))
        .collect();
    if let Some(home) = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|path| PathBuf::from(path).join(".cargo")))
    {
        config_roots.push(if home.is_absolute() {
            home
        } else {
            root.join(home)
        });
    }
    for config_root in config_roots {
        for name in ["config", "config.toml"] {
            let path = config_root.join(name);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    ordinary(&path)?;
                    let canonical = path.canonicalize()?;
                    let key = format!(
                        "cargo-host-config/{}",
                        hash_bytes(canonical.to_string_lossy().as_bytes())
                    );
                    files.insert(key, hash_file(&path)?);
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
        }
    }
    let sha256 = hash_bytes(&serde_json::to_vec(&files)?);
    Ok(SourceIdentity { sha256, files })
}

fn result(status: Health, management: bool, action: &str) -> BuildCheck {
    BuildCheck {
        // Launch admission follows recorded binary integrity: a stale or
        // unreachable checkout reports status and the deploy action while the
        // delivered build keeps serving ordinary launches. Only damaged or
        // unsupported inputs refuse runtime with their corrective action.
        runtime_allowed: matches!(
            status,
            Health::Healthy | Health::SourceStale | Health::SourceUnavailable
        ),
        serving_allowed: matches!(
            status,
            Health::Healthy | Health::SourceStale | Health::SourceUnavailable
        ),
        status,
        management_allowed: management,
        action: action.into(),
    }
}

pub fn read_record(build: &Path) -> io::Result<BuildRecord> {
    ordinary(build)?;
    ordinary(&build.join("build.json"))?;
    let file = fs::File::open(build.join("build.json"))?;
    if file.metadata()?.len() > 4 * 1024 * 1024 {
        return Err(io::Error::other("build record too large"));
    }
    serde_json::from_reader(file).map_err(io::Error::other)
}

pub(crate) fn verify_record_metadata(record: &BuildRecord) -> io::Result<()> {
    if record.schema != 1
        || record.profile != "release"
        || record.target != "x86_64-pc-windows-msvc"
        || !record.source_root.is_absolute()
        || !record.binaries.contains_key("codex-harness.exe")
        || record.binaries.len() > 64
        || record.source.files.is_empty()
        || record.source.sha256 != hash_bytes(&serde_json::to_vec(&record.source.files)?)
        || record.source.files.keys().any(|p| {
            p.is_empty()
                || Path::new(p)
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
        })
        || record.binaries.keys().any(|name| {
            !name.ends_with(".exe")
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                || Path::new(name).components().count() != 1
        })
    {
        return Err(io::Error::other(
            "Unsupported native v1 transport metadata; explicit current Cargo bootstrap is required.",
        ));
    }
    Ok(())
}

/// Stable v1 transport integrity, independent of this consumer's binary set.
/// This does not grant runtime permission: the actual consumer must also check
/// its own compiled contract before reuse or activation.
pub fn verify_record_integrity(build: &Path) -> io::Result<BuildRecord> {
    crate::native_build::ordinary_ancestors(build)?;
    let record = read_record(build)?;
    verify_record_metadata(&record)?;
    for (name, expected) in &record.binaries {
        let path = build.join(name);
        ordinary(&path)?;
        if hash_file(&path)? != *expected {
            return Err(io::Error::other(
                "Native transport binary integrity mismatch.",
            ));
        }
    }
    Ok(record)
}

/// A recorded binary name that may be opened inside the build directory:
/// a single plain `.exe` file name, never a relative path or link input.
fn binary_name_is_safe(name: &str) -> bool {
    name.ends_with(".exe")
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        && Path::new(name).components().count() == 1
}

/// This consumer's required binary set is recorded exactly.
pub(crate) fn required_binaries_present(record: &BuildRecord) -> bool {
    record.binaries.len() == BINARIES.len()
        && BINARIES
            .iter()
            .all(|name| record.binaries.contains_key(*name))
}

/// One coherent verification pass over a build's recorded artifacts.
///
/// Every recorded binary with a safe name is hashed exactly once; no
/// compiled-source input is traversed. `check` adds the explicit source
/// freshness diagnosis on top of this result. The pass intentionally keeps
/// no state beyond its own return value: a later decision verifies again,
/// so a changed or ambiguous artifact can never ride on earlier evidence.
pub struct Artifacts {
    check: BuildCheck,
    record: Option<BuildRecord>,
    /// `Some(digest)` only when this pass verified the recorded binary intact.
    digests: BTreeMap<String, Option<String>>,
}

impl Artifacts {
    /// Integrity and metadata-compatibility verdict of this pass.
    /// `Healthy` here means the required binaries and metadata are intact; it
    /// makes no claim about source freshness.
    pub fn check(&self) -> &BuildCheck {
        &self.check
    }

    /// Actual digest of a recorded binary verified intact in this pass.
    pub fn digest(&self, name: &str) -> Option<&str> {
        self.digests.get(name).and_then(Option::as_deref)
    }
}

/// Verify a build's recorded artifacts without touching the checkout.
pub fn artifacts(build: &Path) -> Artifacts {
    let record = match read_record(build) {
        Ok(record) => record,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Artifacts {
                check: result(
                    Health::Missing,
                    false,
                    "Explicit Cargo bootstrap is required; build metadata is missing.",
                ),
                record: None,
                digests: BTreeMap::new(),
            };
        }
        Err(_) => {
            return Artifacts {
                check: result(
                    Health::Incompatible,
                    false,
                    "Explicit Cargo bootstrap is required; build metadata is invalid.",
                ),
                record: None,
                digests: BTreeMap::new(),
            };
        }
    };
    let names_are_safe = record.binaries.keys().all(|name| binary_name_is_safe(name));
    let compatible = record.schema == SCHEMA
        && record.profile == "release"
        && record.target == "x86_64-pc-windows-msvc"
        && required_binaries_present(&record)
        && record.source.sha256
            == hash_bytes(&serde_json::to_vec(&record.source.files).unwrap_or_default())
        && !record.source.files.keys().any(|p| {
            Path::new(p)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        });
    // Each safely named recorded binary is opened and hashed once here; a
    // malformed name is never resolved against the filesystem.
    let mut digests = BTreeMap::new();
    for (name, expected) in &record.binaries {
        if !binary_name_is_safe(name) {
            continue;
        }
        let path = build.join(name);
        let actual = match ordinary(&path) {
            Ok(()) => hash_file(&path).ok(),
            Err(_) => None,
        };
        digests.insert(name.clone(), actual.filter(|digest| digest == expected));
    }
    let check = if !names_are_safe || !compatible {
        result(
            Health::Incompatible,
            false,
            "Explicit Cargo bootstrap is required; metadata schema or inputs are incompatible.",
        )
    } else if digests.values().any(Option::is_none) {
        let manager_ok = digests
            .get("codex-harness.exe")
            .is_some_and(Option::is_some);
        result(
            Health::Altered,
            manager_ok,
            if manager_ok {
                "Run explicit native build/update to repair the missing or altered dependent binary; the manager still passes integrity checks."
            } else {
                "Explicit Cargo bootstrap is required; the manager is missing or altered."
            },
        )
    } else {
        result(
            Health::Healthy,
            true,
            "Verified native build integrity; run explicit check or diagnose for source freshness.",
        )
    };
    Artifacts {
        check,
        record: Some(record),
        digests,
    }
}

/// Required runtime integrity and compatibility of one build, without any
/// compiled-source traversal. Launch and selection decisions use this;
/// `check` adds the explicit source-freshness diagnosis.
pub fn integrity(build: &Path) -> BuildCheck {
    artifacts(build).check
}

/// Explicit full diagnosis: required artifact integrity plus the freshness of
/// the recorded checkout. Fresh, stale and unavailable source states are
/// distinguished from executable damage.
pub fn check(build: &Path, source_override: Option<&Path>) -> BuildCheck {
    let artifacts = artifacts(build);
    if artifacts.check.status != Health::Healthy {
        return artifacts.check;
    }
    let record = artifacts
        .record
        .as_ref()
        .expect("a healthy artifact pass carries its record");
    let source = source_override.unwrap_or(&record.source_root);
    match source_identity(source) {
        Ok(current) if current == record.source => result(
            Health::Healthy,
            true,
            "Verified native build; source-linked data remains authoritative.",
        ),
        Ok(_) => result(
            Health::SourceStale,
            true,
            "The checkout differs from the delivered build; launches continue on the integrity-verified delivered build. Run explicit native build/update (deploy) to move new processes to the newer source.",
        ),
        Err(_) => result(
            Health::SourceUnavailable,
            true,
            "The recorded checkout is unavailable; launches continue on the integrity-verified delivered build. Select the accessible checkout for explicit recovery or deploy.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(root: &Path) {
        fs::create_dir_all(root.join(INSPECTION_SCHEMA).parent().unwrap()).unwrap();
        fs::write(root.join(INSPECTION_SCHEMA), "{}").unwrap();
        fs::create_dir_all(root.join("crates/test/src")).unwrap();
        fs::create_dir_all(root.join("crates/harness-rtk/src")).unwrap();
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "crates/test/Cargo.toml",
            "crates/test/src/lib.rs",
            "crates/harness-rtk/src/main.rs",
        ] {
            fs::write(root.join(path), path).unwrap();
        }
    }
    #[test]
    fn identity_tracks_added_removed_and_locked_inputs_but_not_docs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        source(root);
        let original = source_identity(root).unwrap();
        fs::write(root.join("crates/test/README.md"), "documentation").unwrap();
        assert_eq!(source_identity(root).unwrap(), original);
        fs::write(root.join("crates/test/src/new.rs"), "new").unwrap();
        assert_ne!(source_identity(root).unwrap(), original);
        fs::remove_file(root.join("crates/test/src/new.rs")).unwrap();
        assert_eq!(source_identity(root).unwrap(), original);
        fs::write(root.join("Cargo.lock"), "changed dependency").unwrap();
        assert_ne!(source_identity(root).unwrap(), original);
    }

    #[test]
    fn nested_module_names_and_compiled_text_are_native_inputs() {
        let temp = tempfile::tempdir().unwrap();
        source(temp.path());
        let initial = source_identity(temp.path()).unwrap();
        for name in ["examples", "tests", "target", "benches"] {
            let directory = temp.path().join("crates/test/src").join(name);
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("mod.rs"), "pub fn run() {}\n").unwrap();
        }
        fs::write(
            temp.path().join("crates/test/src/banner.md"),
            "compiled resource",
        )
        .unwrap();
        let changed = source_identity(temp.path()).unwrap();
        assert_ne!(changed, initial);
        assert!(
            changed
                .files
                .contains_key("crates/test/src/examples/mod.rs")
        );
        assert!(changed.files.contains_key("crates/test/src/banner.md"));
        fs::write(temp.path().join(INSPECTION_SCHEMA), "{\"type\":\"object\"}").unwrap();
        let schema_changed = source_identity(temp.path()).unwrap();
        assert_eq!(schema_changed, changed);
        assert!(!schema_changed.files.contains_key(INSPECTION_SCHEMA));
    }
    #[test]
    fn stale_source_allows_repair_but_altered_manager_does_not() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        source(&root);
        let build = temp.path().join("build");
        fs::create_dir(&build).unwrap();
        let mut binaries = BTreeMap::new();
        for name in BINARIES {
            fs::write(build.join(name), name).unwrap();
            binaries.insert(name.to_string(), hash_bytes(name.as_bytes()));
        }
        let record = BuildRecord {
            schema: SCHEMA,
            source_root: root.clone(),
            source: source_identity(&root).unwrap(),
            rustc: "test".into(),
            cargo: "test".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            binaries,
        };
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(check(&build, None).status, Health::Healthy);
        fs::write(root.join("crates/test/src/lib.rs"), "changed").unwrap();
        let stale = check(&build, None);
        assert_eq!(stale.status, Health::SourceStale);
        assert!(stale.management_allowed);
        assert!(stale.serving_allowed);
        assert!(
            stale.runtime_allowed,
            "a stale checkout keeps launches on the verified delivered build"
        );
        assert!(
            stale.action.contains("delivered build") && stale.action.contains("deploy"),
            "{}",
            stale.action
        );
        drop(fs::rename(&root, temp.path().join("moved")));
        let unavailable = check(&build, None);
        assert_eq!(unavailable.status, Health::SourceUnavailable);
        assert!(unavailable.runtime_allowed);
        assert!(unavailable.action.contains("delivered build"));
        drop(fs::rename(temp.path().join("moved"), &root));
        fs::write(build.join("codex-harness.exe"), "altered").unwrap();
        let altered = check(&build, None);
        assert_eq!(altered.status, Health::Altered);
        assert!(!altered.management_allowed);
    }

    /// Owned build fixture: an intact record and every required binary.
    fn build_fixture(temp: &Path) -> (PathBuf, PathBuf) {
        let root = temp.join("source");
        source(&root);
        let build = temp.join("build");
        fs::create_dir(&build).unwrap();
        let mut binaries = BTreeMap::new();
        for name in BINARIES {
            fs::write(build.join(name), name).unwrap();
            binaries.insert(name.to_string(), hash_bytes(name.as_bytes()));
        }
        let record = BuildRecord {
            schema: SCHEMA,
            source_root: root.clone(),
            source: source_identity(&root).unwrap(),
            rustc: "test".into(),
            cargo: "test".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            binaries,
        };
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        (root, build)
    }

    fn hashes_of(events: &[String], path: &Path) -> usize {
        let needle = format!("hash:{}", path.to_string_lossy());
        events.iter().filter(|event| **event == needle).count()
    }

    #[test]
    fn integrity_hashes_each_required_binary_once_without_source_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let (_root, build) = build_fixture(temp.path());
        let _ = test_probe::take();
        let verified = integrity(&build);
        assert_eq!(verified.status, Health::Healthy);
        assert!(verified.runtime_allowed && verified.serving_allowed);
        let events = test_probe::take();
        assert!(
            !events.iter().any(|event| event == "source_identity"),
            "integrity traversed compiled sources: {events:?}"
        );
        for name in BINARIES {
            assert_eq!(
                hashes_of(&events, &build.join(name)),
                1,
                "{name} verified more than once: {events:?}"
            );
        }
    }

    #[test]
    fn check_reuses_one_artifact_pass_and_still_reports_freshness() {
        let temp = tempfile::tempdir().unwrap();
        let (root, build) = build_fixture(temp.path());
        let _ = test_probe::take();
        assert_eq!(check(&build, None).status, Health::Healthy);
        let events = test_probe::take();
        assert_eq!(
            events
                .iter()
                .filter(|event| *event == "source_identity")
                .count(),
            1,
            "explicit check must diagnose sources exactly once: {events:?}"
        );
        for name in BINARIES {
            assert_eq!(
                hashes_of(&events, &build.join(name)),
                1,
                "{name} hashed more than once in one check: {events:?}"
            );
        }
        // Stale and unavailable checkouts stay distinguishable from damage.
        fs::write(root.join("crates/test/src/lib.rs"), "changed").unwrap();
        assert_eq!(check(&build, None).status, Health::SourceStale);
        fs::rename(&root, temp.path().join("moved")).unwrap();
        assert_eq!(check(&build, None).status, Health::SourceUnavailable);
    }

    #[test]
    fn changed_bytes_with_restored_timestamp_invalidate_verified_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let (_root, build) = build_fixture(temp.path());
        let original = integrity(&build);
        assert_eq!(original.status, Health::Healthy);
        let dependent = build.join("harness-observe.exe");
        let timestamp = fs::metadata(&dependent).unwrap().modified().unwrap();
        let mut altered = b"harness-observe.exe".to_vec();
        altered[0] ^= 0xff;
        fs::write(&dependent, &altered).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&dependent)
            .unwrap()
            .set_modified(timestamp)
            .unwrap();
        let changed = integrity(&build);
        assert_eq!(
            changed.status,
            Health::Altered,
            "a same-size mutation with a restored timestamp must not reuse verification"
        );
        assert!(changed.management_allowed, "the manager itself is intact");
        fs::write(&dependent, "harness-observe.exe").unwrap();
        assert_eq!(integrity(&build).status, Health::Healthy);
        drop(fs::remove_file(build.join("harness-inspect.exe")));
        assert_eq!(
            integrity(&build).status,
            Health::Altered,
            "every required binary stays checked"
        );
    }
}
