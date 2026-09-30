//! Native owned temporary Git cases for experiment preparation: frozen task
//! copies with independent history, candidate checkout allocation and reuse
//! verdicts, and prepared-runtime identity selection. All repositories are
//! synthetic fixtures inside the test's temporary directory.
use harness_core::improvement_experiment::{
    Arm, ArmBinding, ExperimentBindings, prepare_home, prepare_variant, select_variant,
};
use harness_core::task_worktree::{
    ReuseBlock, WorktreeReuse, allocate_candidate_checkout, frozen_copy, verify_candidate_checkout,
    verify_frozen, verify_frozen_pristine, worktree_reuse,
};
use harness_core::{build_identity, build_selection};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git is available");
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn git_result(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git is available")
        .status
        .success()
}

fn git_stdin(cwd: &Path, args: &[&str], input: &str) -> String {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git is available");
    child
        .stdin
        .take()
        .expect("stdin captured")
        .write_all(input.as_bytes())
        .expect("stdin write");
    let out = child.wait_with_output().expect("git output");
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn rev(cwd: &Path) -> String {
    git(cwd, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn configure(cwd: &Path) {
    git(cwd, &["config", "user.email", "fixture@example.test"]);
    git(cwd, &["config", "user.name", "Fixture"]);
}

fn fixture_repo(root: &Path, name: &str) -> PathBuf {
    let repo = root.join(name);
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    configure(&repo);
    fs::write(repo.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    fs::write(repo.join("main.rs"), "fn main() {}\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "seed"]);
    repo
}

#[test]
fn frozen_copies_are_identical_and_independent() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture_repo(temp.path(), "source");
    git(&source, &["checkout", "-q", "-b", "solution"]);
    fs::write(source.join("solution.txt"), "sibling solution\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-qm", "sibling solution"]);
    let solution = rev(&source);
    git(&source, &["checkout", "-q", "main"]);
    let revision = rev(&source);

    let baseline = frozen_copy(&source, &revision, &temp.path().join("workload/baseline")).unwrap();
    let candidate =
        frozen_copy(&source, &revision, &temp.path().join("workload/candidate")).unwrap();
    assert_eq!(baseline.source_revision, revision);
    assert_eq!(baseline.source_revision, candidate.source_revision);
    assert_eq!(
        baseline.tree_sha256, candidate.tree_sha256,
        "both arms freeze the same committed snapshot"
    );
    assert_eq!(baseline.revision, candidate.revision);
    verify_frozen(&baseline).unwrap();
    verify_frozen(&candidate).unwrap();

    // Independent minimal history: one root commit, no remote, no sibling.
    assert!(
        baseline.path.join(".git").is_dir(),
        ".git must be a real directory"
    );
    assert_eq!(
        git(&baseline.path, &["log", "--all", "--oneline"])
            .lines()
            .count(),
        1
    );
    assert!(git(&baseline.path, &["remote"]).trim().is_empty());
    assert!(!git_result(&baseline.path, &["cat-file", "-e", &solution]));
    assert!(!git_result(&candidate.path, &["cat-file", "-e", &solution]));
    assert!(!baseline.path.join("solution.txt").exists());
    assert!(!candidate.path.join("solution.txt").exists());

    // A treatment edit in one copy cannot change the other arm's input and
    // does not alter the frozen revision.
    fs::write(
        baseline.path.join("main.rs"),
        "fn main() { /* arm work */ }\n",
    )
    .unwrap();
    git(&baseline.path, &["add", "."]);
    git(&baseline.path, &["commit", "-qm", "arm baseline work"]);
    let arm_commit = rev(&baseline.path);
    verify_frozen(&baseline).unwrap();
    verify_frozen(&candidate).unwrap();
    assert!(
        !git_result(&candidate.path, &["cat-file", "-e", &arm_commit]),
        "an arm commit is not discoverable from the other copy"
    );
    assert_eq!(
        fs::read_to_string(candidate.path.join("main.rs")).unwrap(),
        "fn main() {}\n"
    );

    // Creation refuses existing state and unresolvable revisions.
    assert!(frozen_copy(&source, &revision, &baseline.path).is_err());
    assert!(
        frozen_copy(
            &source,
            "no-such-revision",
            &temp.path().join("workload/third")
        )
        .is_err()
    );
}

#[test]
fn candidate_checkout_binds_branch_and_revision_and_reuse_preserves_work() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture_repo(temp.path(), "harness");
    let base = rev(&source);
    let checkout = allocate_candidate_checkout(
        &source,
        &temp.path().join("alloc/candidate"),
        "hypothesis-1",
        &base,
    )
    .unwrap();
    assert_eq!(checkout.base, base);
    assert_eq!(checkout.revision, base);
    assert_eq!(checkout.branch, "hypothesis-1");
    verify_candidate_checkout(&checkout).unwrap();

    let elsewhere = temp.path().join("session-checkout");
    let verdict = worktree_reuse(&source, &checkout.path, &elsewhere, &base, false).unwrap();
    assert!(
        matches!(verdict, WorktreeReuse::Eligible { .. }),
        "{verdict:?}"
    );

    // Dirty state is preserved, never reset.
    fs::write(checkout.path.join("scratch.txt"), "wip\n").unwrap();
    let verdict = worktree_reuse(&source, &checkout.path, &elsewhere, &base, false).unwrap();
    assert!(
        matches!(
            verdict,
            WorktreeReuse::Blocked {
                kind: ReuseBlock::Unpreserved,
                ..
            }
        ),
        "{verdict:?}"
    );
    assert!(checkout.path.join("scratch.txt").is_file());

    // Committed but unmerged work is preserved the same way.
    fs::remove_file(checkout.path.join("scratch.txt")).unwrap();
    fs::write(checkout.path.join("candidate.txt"), "candidate work\n").unwrap();
    git(&checkout.path, &["add", "."]);
    git(&checkout.path, &["commit", "-qm", "candidate work"]);
    let candidate_commit = rev(&checkout.path);
    let verdict = worktree_reuse(&source, &checkout.path, &elsewhere, &base, false).unwrap();
    assert!(
        matches!(
            verdict,
            WorktreeReuse::Blocked {
                kind: ReuseBlock::Unpreserved,
                ..
            }
        ),
        "{verdict:?}"
    );
    assert!(checkout.path.join("candidate.txt").is_file());
    assert_eq!(rev(&checkout.path), candidate_commit);

    // An active attempt and a running Git operation both make the allocation busy.
    let verdict =
        worktree_reuse(&source, &checkout.path, &elsewhere, &candidate_commit, true).unwrap();
    assert!(
        matches!(
            verdict,
            WorktreeReuse::Blocked {
                kind: ReuseBlock::Busy,
                ..
            }
        ),
        "{verdict:?}"
    );
    let lock =
        PathBuf::from(git(&checkout.path, &["rev-parse", "--git-path", "index.lock"]).trim());
    let lock = if lock.is_absolute() {
        lock
    } else {
        checkout.path.join(lock)
    };
    fs::write(&lock, "").unwrap();
    let verdict = worktree_reuse(
        &source,
        &checkout.path,
        &elsewhere,
        &candidate_commit,
        false,
    )
    .unwrap();
    assert!(
        matches!(
            verdict,
            WorktreeReuse::Blocked {
                kind: ReuseBlock::Busy,
                ..
            }
        ),
        "{verdict:?}"
    );
    fs::remove_file(&lock).unwrap();

    // The current checkout, a foreign directory and a missing path are refused.
    let verdict = worktree_reuse(&source, &source, &source, &base, false).unwrap();
    assert!(
        matches!(
            verdict,
            WorktreeReuse::Blocked {
                kind: ReuseBlock::CurrentCheckout,
                ..
            }
        ),
        "{verdict:?}"
    );
    let foreign = temp.path().join("foreign");
    fs::create_dir_all(&foreign).unwrap();
    let verdict = worktree_reuse(&source, &foreign, &elsewhere, &base, false).unwrap();
    assert!(
        matches!(
            verdict,
            WorktreeReuse::Blocked {
                kind: ReuseBlock::Foreign,
                ..
            }
        ),
        "{verdict:?}"
    );
    let verdict = worktree_reuse(
        &source,
        &temp.path().join("missing"),
        &elsewhere,
        &base,
        false,
    )
    .unwrap();
    assert!(
        matches!(
            verdict,
            WorktreeReuse::Blocked {
                kind: ReuseBlock::Missing,
                ..
            }
        ),
        "{verdict:?}"
    );

    // Allocation refuses an existing path or branch.
    assert!(allocate_candidate_checkout(&source, &checkout.path, "hypothesis-2", &base).is_err());
    assert!(
        allocate_candidate_checkout(
            &source,
            &temp.path().join("alloc/other"),
            "hypothesis-1",
            &base
        )
        .is_err()
    );

    // The accepted mainline is untouched: no candidate commit is reachable
    // from the source checkout.
    assert_eq!(rev(&source), base);
    assert!(!source.join("candidate.txt").exists());
    assert!(!git_result(
        &source,
        &["merge-base", "--is-ancestor", &candidate_commit, "HEAD"]
    ));
}

fn native_state(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let state = root.join("state");
    let source = root.join("source");
    let schema = source.join(build_identity::INSPECTION_SCHEMA);
    fs::create_dir_all(schema.parent().unwrap()).unwrap();
    fs::write(&schema, "{}").unwrap();
    fs::create_dir_all(source.join("crates/one/src")).unwrap();
    fs::create_dir_all(source.join("crates/harness-rtk/src")).unwrap();
    for name in ["Cargo.toml", "Cargo.lock", "crates/one/src/lib.rs"] {
        fs::write(source.join(name), "fixture").unwrap();
    }
    fs::create_dir_all(state.join("builds")).unwrap();
    fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
    let a = state.join("builds/a");
    let b = state.join("builds/b");
    for build in [&a, &b] {
        fs::create_dir(build).unwrap();
        let mut binaries = BTreeMap::new();
        for name in build_identity::BINARIES {
            fs::write(build.join(name), name).unwrap();
            binaries.insert(
                name.to_string(),
                build_identity::hash_bytes(name.as_bytes()),
            );
        }
        let record = build_identity::BuildRecord {
            schema: build_identity::SCHEMA,
            source_root: source.clone(),
            source: build_identity::source_identity(&source).unwrap(),
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
    }
    (state, source, a, b)
}

#[test]
fn prepared_variant_selection_reports_identity_and_refuses_stale_or_busy() {
    let temp = tempfile::tempdir().unwrap();
    let (state, source, a, b) = native_state(temp.path());
    let baseline = prepare_variant(&state, Arm::Baseline, "H", &a).unwrap();
    let candidate = prepare_variant(&state, Arm::Candidate, "H+A", &b).unwrap();
    assert_eq!(baseline.source_sha256, candidate.source_sha256);

    let source_files: BTreeMap<String, Vec<u8>> =
        ["Cargo.toml", "Cargo.lock", "crates/one/src/lib.rs"]
            .into_iter()
            .map(|name| (name.to_owned(), fs::read(source.join(name)).unwrap()))
            .collect();
    let build_names = || {
        let mut names: Vec<String> = fs::read_dir(state.join("builds"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let before = build_names();

    let consumed = select_variant(&state, &baseline, false).unwrap();
    assert_eq!(consumed.build, a.canonicalize().unwrap());
    assert_eq!(consumed.record_sha256, baseline.record_sha256);
    assert_eq!(
        consumed.manager_sha256.as_deref(),
        Some(build_identity::hash_bytes(b"codex-harness.exe").as_str())
    );
    assert!(consumed.changed);
    let repeated = select_variant(&state, &baseline, false).unwrap();
    assert!(
        !repeated.changed,
        "an unchanged prepared variant is not rebuilt"
    );
    assert_eq!(repeated.build, consumed.build);
    let switched = select_variant(&state, &candidate, false).unwrap();
    assert!(switched.changed);
    assert_eq!(
        build_selection::selected(&state).unwrap().0,
        b.canonicalize().unwrap(),
        "the reported consumed identity is the actually selected artifact"
    );
    let back = select_variant(&state, &baseline, false).unwrap();
    assert!(back.changed);
    assert_eq!(
        build_selection::selected(&state).unwrap().0,
        a.canonicalize().unwrap()
    );

    // Selection never rebuilt or modified compiled inputs or build directories.
    assert_eq!(build_names(), before);
    for (name, bytes) in &source_files {
        assert_eq!(&fs::read(source.join(name)).unwrap(), bytes);
    }

    // A stale prepared variant refuses selection instead of rebuilding.
    fs::write(a.join("codex-harness.exe"), "altered after preparation").unwrap();
    let error = select_variant(&state, &baseline, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("prepare it again"), "{error}");

    // An active attempt keeps its frozen runtime.
    let error = select_variant(&state, &candidate, true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("active"), "{error}");
    assert_eq!(
        build_selection::selected(&state).unwrap().0,
        a.canonicalize().unwrap()
    );
}

#[test]
fn bindings_validate_operational_independence() {
    let temp = tempfile::tempdir().unwrap();
    let (state, _source, a, b) = native_state(temp.path());
    let harness = fixture_repo(temp.path(), "harness");
    let base = rev(&harness);
    let candidate = allocate_candidate_checkout(
        &harness,
        &temp.path().join("alloc/candidate"),
        "hypothesis-1",
        &base,
    )
    .unwrap();
    let task = fixture_repo(temp.path(), "task");
    let task_revision = rev(&task);
    let baseline_home = prepare_home(&temp.path().join("homes/baseline")).unwrap();
    let candidate_home = prepare_home(&temp.path().join("homes/candidate")).unwrap();
    let baseline_workload = frozen_copy(
        &task,
        &task_revision,
        &temp.path().join("workload/baseline"),
    )
    .unwrap();
    let candidate_workload = frozen_copy(
        &task,
        &task_revision,
        &temp.path().join("workload/candidate"),
    )
    .unwrap();
    let baseline_variant = prepare_variant(&state, Arm::Baseline, "H", &a).unwrap();
    let candidate_variant = prepare_variant(&state, Arm::Candidate, "H+A", &b).unwrap();
    let bindings = ExperimentBindings {
        schema: 1,
        hypothesis: "sample-hypothesis-1".into(),
        case_id: "case-b".into(),
        base_revision: base,
        candidate: candidate.clone(),
        oracle: "oracle-identity".into(),
        acceptance: "acceptance/locator".into(),
        policy_digest: "policy-digest".into(),
        arms: vec![
            ArmBinding {
                arm: Arm::Baseline,
                home: baseline_home.clone(),
                workload: baseline_workload,
                runtime: baseline_variant,
            },
            ArmBinding {
                arm: Arm::Candidate,
                home: candidate_home,
                workload: candidate_workload,
                runtime: candidate_variant,
            },
        ],
    };
    bindings.validate().unwrap();
    bindings.verify_pre_attempt().unwrap();
    assert_eq!(bindings.arm(Arm::Candidate).unwrap().runtime.label, "H+A");

    // Shared homes, different frozen revisions and a changed binding refuse.
    let mut tampered = bindings.clone();
    tampered.arms[1].home = tampered.arms[0].home.clone();
    assert!(tampered.validate().is_err());

    // Nested, cross-aliased and canonical aliased allocations refuse even when
    // the corresponding paths differ.
    let nested = temp.path().join("homes/baseline/nested");
    fs::create_dir_all(&nested).unwrap();
    let mut tampered = bindings.clone();
    tampered.arms[1].home = nested;
    let error = tampered.validate().unwrap_err().to_string();
    assert!(error.contains("overlap"), "{error}");

    let mut tampered = bindings.clone();
    tampered.arms[1].home = tampered.arms[0].workload.path.clone();
    let error = tampered.validate().unwrap_err().to_string();
    assert!(error.contains("overlap"), "{error}");

    let mut tampered = bindings.clone();
    tampered.arms[1].workload.path = temp.path().join("workload/candidate/../baseline");
    let error = tampered.validate().unwrap_err().to_string();
    assert!(error.contains("overlap"), "{error}");

    fs::write(task.join("later.txt"), "later revision\n").unwrap();
    git(&task, &["add", "."]);
    git(&task, &["commit", "-qm", "later"]);
    let other = frozen_copy(&task, &rev(&task), &temp.path().join("workload/other")).unwrap();
    let mut tampered = bindings.clone();
    tampered.arms[1].workload = other;
    let error = tampered.validate().unwrap_err().to_string();
    assert!(error.contains("same committed task snapshot"), "{error}");

    let mut tampered = bindings.clone();
    tampered.base_revision = "deadbeef".into();
    assert!(tampered.validate().is_err());

    let mut tampered = bindings.clone();
    tampered.policy_digest = "  ".into();
    assert!(tampered.validate().is_err());

    let mut tampered = bindings.clone();
    tampered.arms[0].runtime.record_sha256 = "0".repeat(64);
    let error = tampered.validate().unwrap_err().to_string();
    assert!(error.contains("changed since preparation"), "{error}");

    assert!(prepare_home(&bindings.arms[0].home).is_err());

    // Contamination is refused by the pre-attempt gate while the structural
    // validation, the snapshot check and the artifact stay usable.
    let candidate_workload = bindings.arm(Arm::Candidate).unwrap().workload.path.clone();
    fs::write(
        candidate_workload.join("prior-solution.txt"),
        "earlier attempt\n",
    )
    .unwrap();
    let error = bindings.verify_pre_attempt().unwrap_err().to_string();
    assert!(error.contains("pristine"), "{error}");
    bindings.validate().unwrap();
    verify_frozen(&bindings.arm(Arm::Candidate).unwrap().workload).unwrap();
    assert!(candidate_workload.join("prior-solution.txt").is_file());
}

#[test]
fn pre_attempt_gate_rejects_contamination_and_preserves_work() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture_repo(temp.path(), "task");
    let revision = rev(&source);

    // A pristine copy passes the gate.
    let copy = frozen_copy(&source, &revision, &temp.path().join("copy-pristine")).unwrap();
    verify_frozen_pristine(&copy).unwrap();

    // An added solution file invalidates the gate, is preserved unchanged and
    // does not break the post-attempt snapshot check.
    fs::write(copy.path.join("prior-solution.txt"), "earlier attempt\n").unwrap();
    let error = verify_frozen_pristine(&copy).unwrap_err().to_string();
    assert!(error.contains("earlier solution"), "{error}");
    verify_frozen(&copy).unwrap();
    assert_eq!(
        fs::read_to_string(copy.path.join("prior-solution.txt")).unwrap(),
        "earlier attempt\n"
    );

    // A modified tracked input invalidates the gate and is preserved.
    let modified = frozen_copy(&source, &revision, &temp.path().join("copy-modified")).unwrap();
    let edited = "fn main() { /* prior attempt */ }\n";
    fs::write(modified.path.join("main.rs"), edited).unwrap();
    assert!(verify_frozen_pristine(&modified).is_err());
    assert_eq!(
        fs::read_to_string(modified.path.join("main.rs")).unwrap(),
        edited
    );

    // An extra sibling branch and commit invalidate the gate; the work
    // survives for preservation or explicit reconstruction.
    let branched = frozen_copy(&source, &revision, &temp.path().join("copy-branch")).unwrap();
    git(&branched.path, &["checkout", "-q", "-b", "sibling"]);
    fs::write(branched.path.join("sibling.txt"), "sibling solution\n").unwrap();
    git(&branched.path, &["add", "."]);
    git(&branched.path, &["commit", "-qm", "sibling solution"]);
    let sibling = rev(&branched.path);
    let error = verify_frozen_pristine(&branched).unwrap_err().to_string();
    assert!(error.contains("references"), "{error}");
    assert_eq!(
        git(&branched.path, &["rev-parse", "refs/heads/sibling"]).trim(),
        sibling
    );
    verify_frozen(&branched).unwrap();

    // A loose object without any reference is unreachable sibling history.
    let dangling = frozen_copy(&source, &revision, &temp.path().join("copy-dangling")).unwrap();
    let object = git_stdin(
        &dangling.path,
        &["hash-object", "-w", "--stdin"],
        "sibling solution\n",
    );
    let object = object.trim().to_owned();
    let error = verify_frozen_pristine(&dangling).unwrap_err().to_string();
    assert!(error.contains("outside the frozen revision"), "{error}");
    assert!(git_result(&dangling.path, &["cat-file", "-e", &object]));

    // A shared or alternate object database invalidates the gate.
    let shared = frozen_copy(&source, &revision, &temp.path().join("copy-shared")).unwrap();
    let alternates = shared.path.join(".git/objects/info/alternates");
    fs::create_dir_all(alternates.parent().unwrap()).unwrap();
    fs::write(
        &alternates,
        format!("{}\n", source.join(".git/objects").display()),
    )
    .unwrap();
    let error = verify_frozen_pristine(&shared).unwrap_err().to_string();
    assert!(error.contains("alternate object store"), "{error}");

    // A legitimate executor commit is post-attempt work: the snapshot check
    // stays usable while the pre-attempt gate correctly refuses reuse.
    git(&modified.path, &["add", "."]);
    git(&modified.path, &["commit", "-qm", "executor work"]);
    verify_frozen(&modified).unwrap();
    assert!(verify_frozen_pristine(&modified).is_err());
}
