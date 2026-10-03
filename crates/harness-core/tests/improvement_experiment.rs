//! Native owned temporary Git cases for experiment preparation: frozen task
//! copies with independent history, candidate checkout allocation and reuse
//! verdicts, and prepared-runtime identity selection. All repositories are
//! synthetic fixtures inside the test's temporary directory.
use harness_core::board_feedback;
use harness_core::board_hypothesis::{
    self, Admission, BoundedHypothesis, BoundedRetention, HypothesisDraft, RetentionDraft,
    SearchFilter,
};
use harness_core::improvement_experiment::{
    Arm, ArmBinding, CorroborationRequirement, CorroborationStatus, ExclusionReason,
    ExperimentBindings, TaskRetention, prepare_home, prepare_variant, retain_completed_task,
    retained_tasks_from_board, select_corroboration, select_variant,
};
use harness_core::task_worktree::{
    FrozenCopy, ReuseBlock, WorktreeReuse, allocate_candidate_checkout, frozen_copy,
    verify_candidate_checkout, verify_frozen, verify_frozen_pristine, worktree_reuse,
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

fn prepared_bindings(root: &Path) -> (ExperimentBindings, PathBuf) {
    let (state, _source, a, b) = native_state(root);
    let harness = fixture_repo(root, "harness");
    let base = rev(&harness);
    let candidate = allocate_candidate_checkout(
        &harness,
        &root.join("alloc/candidate"),
        "hypothesis-1",
        &base,
    )
    .unwrap();
    let task = fixture_repo(root, "task");
    let task_revision = rev(&task);
    let baseline_home = prepare_home(&root.join("homes/baseline")).unwrap();
    let candidate_home = prepare_home(&root.join("homes/candidate")).unwrap();
    let baseline_workload =
        frozen_copy(&task, &task_revision, &root.join("workload/baseline")).unwrap();
    let candidate_workload =
        frozen_copy(&task, &task_revision, &root.join("workload/candidate")).unwrap();
    let baseline_variant = prepare_variant(&state, Arm::Baseline, "H", &a).unwrap();
    let candidate_variant = prepare_variant(&state, Arm::Candidate, "H+A", &b).unwrap();
    (
        ExperimentBindings {
            schema: 1,
            hypothesis: "sample-hypothesis-1".into(),
            case_id: "case-b".into(),
            base_revision: base,
            candidate,
            oracle: "oracle-identity".into(),
            acceptance: "acceptance/locator".into(),
            policy_digest: "policy-digest".into(),
            arms: vec![
                ArmBinding {
                    arm: Arm::Baseline,
                    home: baseline_home,
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
        },
        task,
    )
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
    let (bindings, task) = prepared_bindings(temp.path());
    bindings.validate().unwrap();
    bindings.verify_pre_attempt(Arm::Baseline).unwrap();
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
    let error = bindings
        .verify_pre_attempt(Arm::Candidate)
        .unwrap_err()
        .to_string();
    assert!(error.contains("pristine"), "{error}");
    bindings.verify_pre_attempt(Arm::Baseline).unwrap();
    bindings.validate().unwrap();
    verify_frozen(&bindings.arm(Arm::Candidate).unwrap().workload).unwrap();
    assert!(candidate_workload.join("prior-solution.txt").is_file());
}

#[test]
fn pre_attempt_gate_is_arm_specific_and_preserves_completed_work() {
    let temp = tempfile::tempdir().unwrap();
    let (bindings, _task) = prepared_bindings(temp.path());
    let baseline = bindings.arm(Arm::Baseline).unwrap().workload.clone();
    let candidate = bindings.arm(Arm::Candidate).unwrap().workload.clone();

    // Two pristine arms: the first arm's pre-check passes.
    bindings.verify_pre_attempt(Arm::Baseline).unwrap();

    // The baseline attempt writes and commits a valid solution.
    fs::write(
        baseline.path.join("baseline-solution.txt"),
        "baseline solution\n",
    )
    .unwrap();
    git(&baseline.path, &["add", "."]);
    git(&baseline.path, &["commit", "-qm", "baseline solution"]);
    let baseline_commit = rev(&baseline.path);

    // The candidate pre-check passes without touching the completed arm, and
    // the completed arm stays identity-checked through the snapshot check.
    bindings.verify_pre_attempt(Arm::Candidate).unwrap();
    assert_eq!(rev(&baseline.path), baseline_commit);
    assert!(baseline.path.join("baseline-solution.txt").is_file());
    verify_frozen(&baseline).unwrap();
    bindings.validate().unwrap();

    // Re-dispatching the completed arm refuses its prior solution...
    let error = bindings
        .verify_pre_attempt(Arm::Baseline)
        .unwrap_err()
        .to_string();
    assert!(error.contains("pristine"), "{error}");
    // ...while the upcoming arm is unaffected by that refusal.
    bindings.verify_pre_attempt(Arm::Candidate).unwrap();

    // A dirty upcoming candidate refuses and keeps its artifacts.
    fs::write(candidate.path.join("scratch.txt"), "wip\n").unwrap();
    let error = bindings
        .verify_pre_attempt(Arm::Candidate)
        .unwrap_err()
        .to_string();
    assert!(error.contains("pristine"), "{error}");
    assert!(candidate.path.join("scratch.txt").is_file());
    verify_frozen(&candidate).unwrap();
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

const SOLUTION_TEXT: &str = "prior solution: earlier attempt answer\n";

/// A completed real task: the frozen pre-solution snapshot plus the committed
/// answer one attempt produced inside its copy.
fn completed_task(root: &Path, name: &str) -> (FrozenCopy, String) {
    let source = fixture_repo(root, &format!("{name}-source"));
    // Each real task has its own inputs; byte-identical snapshots are one
    // unit, not independent corroboration.
    fs::write(source.join("task-id.txt"), format!("{name}\n")).unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-qm", "task identity"]);
    let revision = rev(&source);
    let completed =
        frozen_copy(&source, &revision, &root.join(format!("{name}-completed"))).unwrap();
    fs::write(completed.path.join("answer.txt"), SOLUTION_TEXT).unwrap();
    git(&completed.path, &["add", "."]);
    git(
        &completed.path,
        &["commit", "-qm", "completed attempt answer"],
    );
    let solution = rev(&completed.path);
    (completed, solution)
}

fn retained_task(
    root: &Path,
    name: &str,
    owner: &str,
    case_id: &str,
    mechanism: &str,
    conditions: &str,
) -> (harness_core::improvement_experiment::RetainedTask, String) {
    let (completed, solution) = completed_task(root, name);
    let retention = TaskRetention {
        owner: owner.to_owned(),
        case_id: case_id.to_owned(),
        experiment: "exp-corroboration".to_owned(),
        mechanism: mechanism.to_owned(),
        conditions: conditions.to_owned(),
        oracle: "oracle-7".to_owned(),
        acceptance: "acceptance/run-9".to_owned(),
    };
    let retained = retain_completed_task(
        &completed,
        &root.join(format!("{name}-retained")),
        &retention,
    )
    .unwrap();
    (retained, solution)
}

/// Admit one synthetic hypothesis card on a fresh board.
fn admit_fixture_card(bd: &Path, project: &Path) -> String {
    let bounded = BoundedHypothesis::try_from_draft(HypothesisDraft {
        mechanism: "bounded-output".to_owned(),
        conditions: "local-tool-runs".to_owned(),
        observation: "fixture:observation#1".to_owned(),
        predicted: "less repeated context loading".to_owned(),
        counterexample: "diagnostics vanish on failure".to_owned(),
        acceptance: "diagnostic preservation check passes".to_owned(),
        spec: "openspec/changes/fixture".to_owned(),
        basis: "fixture:seed#1".to_owned(),
    })
    .unwrap();
    let Admission::Created { id } =
        board_hypothesis::admit_hypothesis(bd, project, &bounded, None).unwrap()
    else {
        panic!("a fresh synthetic board must create one card");
    };
    id
}

/// Write the durable retention record exactly as the controller does: the
/// draft carries the identity fields the driver maps from a retained task, and
/// the board writer resolves the frozen git tree object id from the retained
/// copy before recording it.
fn write_board_retention(
    bd: &Path,
    project: &Path,
    item: &str,
    retained: &harness_core::improvement_experiment::RetainedTask,
) -> std::io::Result<board_hypothesis::RetentionRecord> {
    let draft = RetentionDraft {
        case_id: retained.case_id.clone(),
        experiment: retained.experiment.clone(),
        mechanism: retained.mechanism.clone(),
        conditions: retained.conditions.clone(),
        revision: retained.replay.source_revision.clone(),
        frozen: retained.replay.revision.clone(),
        tree: retained.replay.tree_sha256.clone(),
        oracle: retained.oracle.clone(),
        acceptance: retained.acceptance.clone(),
        replay: retained.replay.path.display().to_string(),
        detail: Some("completed real task retained at the decision boundary".to_owned()),
    };
    let bounded = BoundedRetention::try_from_draft(draft).unwrap();
    board_hypothesis::record_retention(bd, project, item, &bounded)
}

/// Append one raw comment through the board's own CLI, as an earlier writer
/// would have.
fn add_board_comment(bd: &Path, project: &Path, item: &str, text: &str) {
    let out = Command::new(bd)
        .args(["comment", item, "--json", text])
        .current_dir(project)
        .output()
        .expect("bd comment runs");
    assert!(
        out.status.success(),
        "bd comment: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn retained_tasks_keep_replayable_pre_solution_inputs_and_refuse_absent_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let (completed, solution) = completed_task(temp.path(), "task");
    verify_frozen(&completed).unwrap();

    let retention = TaskRetention {
        owner: "bdct-h1".to_owned(),
        case_id: "case-b".to_owned(),
        experiment: "exp-1".to_owned(),
        mechanism: "bounded-output".to_owned(),
        conditions: "local-tool-runs".to_owned(),
        oracle: "oracle-7".to_owned(),
        acceptance: "acceptance/run-9".to_owned(),
    };
    let retained =
        retain_completed_task(&completed, &temp.path().join("retained"), &retention).unwrap();
    assert_eq!(retained.schema, 1);
    assert_eq!(retained.replay.tree_sha256, completed.tree_sha256);
    assert_eq!(retained.replay.revision, completed.revision);
    assert_eq!(retained.replay.source_revision, completed.source_revision);
    verify_frozen_pristine(&retained.replay).unwrap();

    // The retained copy is the pre-solution snapshot: the completed attempt's
    // committed answer is neither copied nor reachable from it.
    assert!(!retained.replay.path.join("answer.txt").exists());
    assert!(!git_result(
        &retained.replay.path,
        &["cat-file", "-e", &solution]
    ));
    assert_eq!(
        git(&retained.replay.path, &["log", "--all", "--oneline"])
            .lines()
            .count(),
        1
    );
    assert!(git(&retained.replay.path, &["remote"]).trim().is_empty());

    // Absent evidence is refused: a missing oracle or acceptance reference
    // cannot be replaced by a summary, and the refused destination is not
    // left behind.
    let blank_oracle = TaskRetention {
        oracle: "  ".to_owned(),
        ..retention.clone()
    };
    let destination = temp.path().join("refused-oracle");
    let error = retain_completed_task(&completed, &destination, &blank_oracle)
        .unwrap_err()
        .to_string();
    assert!(error.contains("oracle"), "{error}");
    assert!(!destination.exists());

    let blank_acceptance = TaskRetention {
        acceptance: String::new(),
        ..retention.clone()
    };
    let destination = temp.path().join("refused-acceptance");
    let error = retain_completed_task(&completed, &destination, &blank_acceptance)
        .unwrap_err()
        .to_string();
    assert!(error.contains("acceptance"), "{error}");
    assert!(!destination.exists());

    // An existing destination is never merged or overwritten.
    let occupied = temp.path().join("occupied");
    fs::create_dir_all(&occupied).unwrap();
    assert!(retain_completed_task(&completed, &occupied, &retention).is_err());
}

#[test]
fn corroboration_selection_excludes_inapplicable_workloads_without_leaking_solutions() {
    let temp = tempfile::tempdir().unwrap();
    let (case_a, solution_a) = retained_task(
        temp.path(),
        "case-a",
        "bdct-h1",
        "case-a",
        "bounded-output",
        "local-tool-runs",
    );
    let (case_b, _) = retained_task(
        temp.path(),
        "case-b",
        "bdct-h2",
        "case-b",
        "bounded-output",
        "local-tool-runs",
    );
    let (case_c, _) = retained_task(
        temp.path(),
        "case-c",
        "bdct-h3",
        "case-c",
        "other-mechanism",
        "local-tool-runs",
    );

    let requirement = CorroborationRequirement {
        mechanism: "bounded-output".to_owned(),
        conditions: "local-tool-runs".to_owned(),
        required_units: 2,
        excluded: Vec::new(),
    };

    // Applicable, independent, replayable retained tasks are selected; the
    // workload that never exercises the mechanism is excluded as
    // non-evidence rather than counted for or against the claim.
    let first = select_corroboration(
        &[case_a.clone(), case_c.clone(), case_b.clone()],
        &requirement,
    )
    .unwrap();
    assert!(first.is_ready());
    assert_eq!(first.units.len(), 2);
    assert!(
        first
            .units
            .iter()
            .all(|unit| unit.mechanism == "bounded-output")
    );
    assert!(
        first
            .excluded
            .iter()
            .any(|unit| unit.case_id == "case-c" && unit.reason == ExclusionReason::NotApplicable)
    );

    // Selection is independent of the caller's candidate order.
    let second = select_corroboration(
        &[case_b.clone(), case_a.clone(), case_c.clone()],
        &requirement,
    )
    .unwrap();
    assert_eq!(first.units, second.units);

    // The selection carries identity references only: no solution text, no
    // solution revision, no acceptance content and no replay path.
    let payload = serde_json::to_string(&first).unwrap();
    assert!(!payload.contains("prior solution"), "{payload}");
    assert!(!payload.contains(&solution_a), "{payload}");
    assert!(!payload.contains("oracle-7"), "{payload}");
    assert!(!payload.contains("acceptance/run-9"), "{payload}");
    assert!(!payload.contains("answer.txt"), "{payload}");

    // A fresh executor receives a replayed pre-solution snapshot for a
    // selected unit, never the completed attempt's answer.
    let selected = first
        .units
        .iter()
        .find(|unit| unit.case_id == "case-a")
        .expect("case-a is selected");
    assert_eq!(selected.owner, "bdct-h1");
    let replay = case_a
        .prepare_replay(&temp.path().join("replay-fresh"))
        .unwrap();
    verify_frozen_pristine(&replay).unwrap();
    assert!(!replay.path.join("answer.txt").exists());
    assert!(!git_result(&replay.path, &["cat-file", "-e", &solution_a]));
    assert_eq!(replay.tree_sha256, case_a.replay.tree_sha256);
    assert_eq!(replay.revision, case_a.replay.revision);

    // Fewer applicable units than declared: the broader claim stays
    // inconclusive - inapplicable workloads are not evidence of general
    // uselessness - and no summary or saving appears in the result.
    let strict = CorroborationRequirement {
        required_units: 3,
        ..requirement.clone()
    };
    let selection =
        select_corroboration(&[case_a.clone(), case_c.clone(), case_b.clone()], &strict).unwrap();
    assert!(!selection.is_ready());
    match &selection.status {
        CorroborationStatus::Inconclusive(reason) => {
            assert!(
                reason.contains("broader claim remains unsupported"),
                "{reason}"
            );
            assert!(reason.contains("not evidence against it"), "{reason}");
        }
        other => panic!("expected an inconclusive broader claim, got {other:?}"),
    }

    // One task or one frozen snapshot is one unit: duplicates are not
    // independent corroboration.
    let selection = select_corroboration(
        &[case_a.clone(), case_a.clone(), case_c.clone()],
        &requirement,
    )
    .unwrap();
    assert_eq!(selection.units.len(), 1);
    assert_eq!(
        selection
            .excluded
            .iter()
            .filter(|unit| unit.reason == ExclusionReason::AlreadyUsed)
            .count(),
        1
    );
    assert!(!selection.is_ready());

    // Identities fixed in the declared plan before results are excluded.
    let planned = CorroborationRequirement {
        excluded: vec!["case-a".to_owned()],
        ..requirement.clone()
    };
    let selection = select_corroboration(&[case_a.clone(), case_b.clone()], &planned).unwrap();
    assert_eq!(selection.units.len(), 1);
    assert!(selection.units[0].case_id == "case-b");
    assert!(
        selection
            .excluded
            .iter()
            .any(|unit| unit.case_id == "case-a" && unit.reason == ExclusionReason::AlreadyUsed)
    );

    // A corrupted retained copy - or an absent artifact - cannot substitute
    // for evidence: the unit is excluded, the state is preserved, and the
    // claim stays inconclusive.
    fs::write(case_b.replay.path.join("answer.txt"), "late answer\n").unwrap();
    let selection = select_corroboration(
        &[case_a.clone(), case_b.clone(), case_c.clone()],
        &requirement,
    )
    .unwrap();
    assert!(
        selection
            .excluded
            .iter()
            .any(|unit| unit.case_id == "case-b"
                && matches!(unit.reason, ExclusionReason::NotReplayable { .. }))
    );
    assert_eq!(selection.units.len(), 1);
    assert!(!selection.is_ready());
    assert!(case_b.replay.path.join("answer.txt").is_file());
    assert!(
        case_b
            .prepare_replay(&temp.path().join("replay-late"))
            .is_err()
    );

    let zero = CorroborationRequirement {
        required_units: 0,
        ..requirement.clone()
    };
    assert!(select_corroboration(&[case_a], &zero).is_err());
}

fn bd_name() -> &'static str {
    if cfg!(windows) { "bd.exe" } else { "bd" }
}

fn bd_executable() -> PathBuf {
    if let Some(value) = std::env::var_os("HARNESS_BD_EXE") {
        return PathBuf::from(value);
    }
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        let candidate = PathBuf::from(home).join("harness/bin").join(bd_name());
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(bd_name());
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
}

/// One owned synthetic project with a seeded Git checkout and an initialized
/// bd board.
fn bd_project(root: &Path) -> PathBuf {
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "--initial-branch=main"]);
    git(&project, &["config", "user.email", "fixture@example.test"]);
    git(&project, &["config", "user.name", "Fixture"]);
    fs::write(project.join("README.md"), "synthetic\n").unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "-qm", "seed"]);
    let init = Command::new(bd_executable())
        .args([
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "bdct",
        ])
        .current_dir(&project)
        .output()
        .expect("bd init runs");
    assert!(
        init.status.success(),
        "bd init: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    project
}

#[test]
fn retention_is_recorded_once_under_its_existing_beads_owner() {
    let temp = tempfile::tempdir().unwrap();
    let project = bd_project(temp.path());
    let bd = bd_executable();
    let id = admit_fixture_card(&bd, &project);

    let (retained, _solution) = retained_task(
        temp.path(),
        "task",
        &id,
        "case-b",
        "bounded-output",
        "local-tool-runs",
    );
    let record = write_board_retention(&bd, &project, &id, &retained).unwrap();
    assert!(record.recorded);
    assert_eq!(record.case_id, "case-b");
    let again = write_board_retention(&bd, &project, &id, &retained).unwrap();
    assert!(
        !again.recorded,
        "an identical retention is not recorded twice"
    );

    let comments = board_feedback::list_comments(&bd, &project, &id).unwrap();
    let parsed = board_hypothesis::parse_retentions(&comments);
    assert_eq!(parsed.len(), 1, "one retention record: {comments:?}");
    let parsed = &parsed[0];
    assert_eq!(parsed.item, id);
    assert_eq!(parsed.case_id, retained.case_id);
    assert_eq!(parsed.frozen, retained.replay.revision);
    assert_eq!(parsed.tree, retained.replay.tree_sha256);
    assert_eq!(
        parsed.tree_object.as_deref(),
        Some(retained.replay.tree.as_str()),
        "the durable record carries the frozen git tree object id"
    );
    assert_eq!(parsed.oracle, retained.oracle);
    assert_eq!(parsed.acceptance, retained.acceptance);
    assert_eq!(parsed.replay, retained.replay.path.display().to_string());
    assert!(
        comments
            .iter()
            .all(|comment| !comment.contains("prior solution")),
        "the owner record is references only: {comments:?}"
    );

    // Retention stays under the existing owner: one card, no second store,
    // and the retained task is discoverable through the card's own count.
    let cards = board_hypothesis::list_hypothesis_cards(&bd, &project).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, id);
    let matches = board_hypothesis::search_hypotheses(
        &bd,
        &project,
        &SearchFilter {
            mechanism: Some("bounded-output".to_owned()),
            conditions: Some("local-tool-runs".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].retentions, 1);
}

#[test]
fn board_retained_tasks_rebuild_verifiably_and_stay_selectable() {
    let temp = tempfile::tempdir().unwrap();
    let project = bd_project(temp.path());
    let bd = bd_executable();
    let id = admit_fixture_card(&bd, &project);
    let (retained, solution) = retained_task(
        temp.path(),
        "task",
        &id,
        "case-b",
        "bounded-output",
        "local-tool-runs",
    );
    let record = write_board_retention(&bd, &project, &id, &retained).unwrap();
    assert!(record.recorded);

    // The durable owner record carries the frozen git tree object id and the
    // existing identity references only - never the answer.
    let comments = board_feedback::list_comments(&bd, &project, &id).unwrap();
    let parsed = board_hypothesis::parse_retentions(&comments);
    assert_eq!(parsed.len(), 1, "one retention record: {comments:?}");
    assert_eq!(
        parsed[0].tree_object.as_deref(),
        Some(retained.replay.tree.as_str())
    );
    assert!(
        comments
            .iter()
            .all(|comment| !comment.contains("prior solution") && !comment.contains("answer")),
        "the owner record is references only: {comments:?}"
    );

    // A later run rebuilds the task from the board alone - no run-local index
    // is consulted - and the retained copy must reproduce the recorded frozen
    // identity: root commit, tree object and content digest.
    let discovery = retained_tasks_from_board(&bd, &project).unwrap();
    assert!(
        discovery.unsupported.is_empty(),
        "{:?}",
        discovery.unsupported
    );
    assert_eq!(discovery.tasks.len(), 1);
    let rebuilt = &discovery.tasks[0];
    assert_eq!(rebuilt.owner, id);
    assert_eq!(rebuilt.case_id, "case-b");
    assert_eq!(rebuilt.experiment, retained.experiment);
    assert_eq!(rebuilt.mechanism, "bounded-output");
    assert_eq!(rebuilt.conditions, "local-tool-runs");
    assert_eq!(rebuilt.oracle, retained.oracle);
    assert_eq!(rebuilt.acceptance, retained.acceptance);
    assert_eq!(rebuilt.replay.revision, retained.replay.revision);
    assert_eq!(rebuilt.replay.tree, retained.replay.tree);
    assert_eq!(rebuilt.replay.tree_sha256, retained.replay.tree_sha256);
    assert_eq!(
        rebuilt.replay.source_revision,
        retained.replay.source_revision
    );
    verify_frozen_pristine(&rebuilt.replay).unwrap();

    // The rebuilt prior task is selectable for corroboration by identity only:
    // the fresh executor never sees the earlier answer.
    let requirement = CorroborationRequirement {
        mechanism: "bounded-output".to_owned(),
        conditions: "local-tool-runs".to_owned(),
        required_units: 1,
        excluded: Vec::new(),
    };
    let selection = select_corroboration(&discovery.tasks, &requirement).unwrap();
    assert!(selection.is_ready());
    assert_eq!(selection.units.len(), 1);
    let unit = &selection.units[0];
    assert_eq!(unit.owner, id);
    assert_eq!(unit.case_id, "case-b");
    assert_eq!(unit.revision, retained.replay.revision);
    assert_eq!(unit.tree_sha256, retained.replay.tree_sha256);
    let payload = serde_json::to_string(&selection).unwrap();
    assert!(!payload.contains(SOLUTION_TEXT.trim()), "{payload}");
    assert!(!payload.contains(&solution), "{payload}");
    assert!(!payload.contains("answer.txt"), "{payload}");
    assert!(!payload.contains("oracle-7"), "{payload}");

    // A declared requirement beyond the rebuilt units stays inconclusive: too
    // few verifiable units are not replaced by a summary or a saving.
    let strict = CorroborationRequirement {
        required_units: 2,
        ..requirement.clone()
    };
    let selection = select_corroboration(&discovery.tasks, &strict).unwrap();
    assert!(!selection.is_ready());
    assert_eq!(selection.units.len(), 1, "the one rebuilt unit is reported");
    assert_eq!(selection.units[0].case_id, "case-b");
    match &selection.status {
        CorroborationStatus::Inconclusive(reason) => assert!(
            reason.contains("fewer applicable independent replayable retained tasks"),
            "{reason}"
        ),
        other => panic!("expected an inconclusive broader claim, got {other:?}"),
    }

    // A fresh executor receives the replayed pre-solution snapshot of the
    // rebuilt task, never the completed attempt's answer.
    let replay = rebuilt
        .prepare_replay(&temp.path().join("board-replay"))
        .unwrap();
    verify_frozen_pristine(&replay).unwrap();
    assert!(!replay.path.join("answer.txt").exists());
    assert!(!git_result(&replay.path, &["cat-file", "-e", &solution]));
    assert_eq!(replay.tree_sha256, rebuilt.replay.tree_sha256);
    assert_eq!(replay.revision, rebuilt.replay.revision);
}

#[test]
fn missing_or_changed_board_artifacts_stay_unsupported() {
    let temp = tempfile::tempdir().unwrap();
    let project = bd_project(temp.path());
    let bd = bd_executable();
    let id = admit_fixture_card(&bd, &project);
    let (ok, _) = retained_task(
        temp.path(),
        "ok",
        &id,
        "case-ok",
        "bounded-output",
        "local-tool-runs",
    );
    write_board_retention(&bd, &project, &id, &ok).unwrap();
    let (changed, _) = retained_task(
        temp.path(),
        "changed",
        &id,
        "case-changed",
        "bounded-output",
        "local-tool-runs",
    );
    write_board_retention(&bd, &project, &id, &changed).unwrap();
    let (gone, _) = retained_task(
        temp.path(),
        "gone",
        &id,
        "case-gone",
        "bounded-output",
        "local-tool-runs",
    );
    write_board_retention(&bd, &project, &id, &gone).unwrap();
    fs::remove_dir_all(&gone.replay.path).unwrap();

    // A record written before the frozen tree object id was retained stays
    // readable, but it cannot be rebuilt verifiably.
    let legacy = format!(
        "hypothesis-retention v1 item={id} case=case-legacy experiment=exp-corroboration mechanism=bounded-output conditions=local-tool-runs revision={} frozen={} tree={} oracle=oracle-7 acceptance=acceptance/run-9 replay={} detail=legacy record",
        ok.replay.source_revision,
        ok.replay.revision,
        ok.replay.tree_sha256,
        ok.replay.path.display()
    );
    add_board_comment(&bd, &project, &id, &legacy);

    // A changed artifact: late work moved the frozen branch off the recorded
    // root commit. The state is preserved, not cleaned or reset.
    fs::write(changed.replay.path.join("late.txt"), "late work\n").unwrap();
    git(&changed.replay.path, &["add", "."]);
    git(&changed.replay.path, &["commit", "-qm", "late work"]);

    let discovery = retained_tasks_from_board(&bd, &project).unwrap();
    assert_eq!(discovery.tasks.len(), 1);
    assert_eq!(discovery.tasks[0].case_id, "case-ok");
    assert_eq!(discovery.unsupported.len(), 3);
    let reason = |case: &str| {
        discovery
            .unsupported
            .iter()
            .find(|unit| unit.case_id == case)
            .map(|unit| unit.reason.clone())
            .unwrap_or_else(|| panic!("{case} is reported unsupported: {discovery:?}"))
    };
    assert!(
        reason("case-changed").contains("frozen branch moved"),
        "{}",
        reason("case-changed")
    );
    assert!(
        reason("case-gone").contains("missing"),
        "{}",
        reason("case-gone")
    );
    assert!(
        reason("case-legacy").contains("no frozen git tree object id"),
        "{}",
        reason("case-legacy")
    );
    assert!(
        discovery
            .unsupported
            .iter()
            .all(|unit| !unit.reason.contains("prior solution")),
        "{discovery:?}"
    );
    assert!(changed.replay.path.join("late.txt").is_file());
    assert!(!gone.replay.path.exists());

    // Too few rebuilt units: the declared requirement stays inconclusive and
    // no unit is synthesized from the unsupported records.
    let requirement = CorroborationRequirement {
        mechanism: "bounded-output".to_owned(),
        conditions: "local-tool-runs".to_owned(),
        required_units: 2,
        excluded: Vec::new(),
    };
    let selection = select_corroboration(&discovery.tasks, &requirement).unwrap();
    assert!(!selection.is_ready());
    assert_eq!(selection.units.len(), 1);
    assert_eq!(
        selection.units[0].case_id, "case-ok",
        "only the verifiable unit is selectable; unsupported records never synthesize a unit"
    );

    // A retention whose frozen identity cannot be resolved at write time is
    // refused and not recorded: the durable board never carries an identity
    // the retained artifact cannot support.
    let (absent, _) = retained_task(
        temp.path(),
        "absent",
        &id,
        "case-absent",
        "bounded-output",
        "local-tool-runs",
    );
    fs::remove_dir_all(&absent.replay.path).unwrap();
    let error = write_board_retention(&bd, &project, &id, &absent).unwrap_err();
    assert!(
        error.to_string().contains("frozen tree object id"),
        "{error}"
    );
    let comments = board_feedback::list_comments(&bd, &project, &id).unwrap();
    assert!(
        !comments
            .iter()
            .any(|comment| comment.contains("case-absent")),
        "a refused retention leaves no record: {comments:?}"
    );
}
