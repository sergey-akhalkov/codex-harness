//! Codex-managed executor checkouts. Ordinary Git worktrees are not a substitute.
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub schema: u32,
    pub path: PathBuf,
    pub source: PathBuf,
    pub head: String,
    pub owner_thread: Option<String>,
    pub archived: bool,
    pub unavailable: bool,
    pub remote_tui_omits_worktree_flag: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retirement {
    Deleted,
    Preserved { limitation: String },
}

/// Outcome of returning a lane worktree after an accepted merge. Lanes are
/// lane-owned: a successfully reset lane stays in place for the next task in
/// the same lane, keeping its ignored build caches. Lane retirement and
/// unresettable state remain `retire`'s decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneDisposition {
    Reused { base: String },
    Preserved { limitation: String },
}

pub fn limitation(detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("managed worktrees unavailable: {detail}"),
    )
}

pub fn worktrees_enabled(codex_home: &Path) -> io::Result<bool> {
    let path = codex_home.join("config.toml");
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text.contains("worktrees") && text.contains("true")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn enable_worktrees(args: &mut Vec<OsString>) {
    if !args.iter().any(|arg| arg == "--enable") {
        args.splice(0..0, ["--enable".into(), "worktrees".into()]);
    }
}

pub fn strip_worktree_flag(args: &[OsString]) -> Vec<OsString> {
    args.iter()
        .filter(|arg| *arg != "--worktree")
        .cloned()
        .collect()
}

pub fn remote_tui_args(mapping: &Mapping, remote: &[OsString]) -> io::Result<Vec<OsString>> {
    if remote.iter().any(|arg| arg == "--worktree") && remote.iter().any(|arg| arg == "--remote") {
        return Err(limitation(
            "native CLI rejects --worktree with --remote; attach the view to the managed cwd",
        ));
    }
    let mut args = strip_worktree_flag(remote);
    let cwd = mapping
        .path
        .to_str()
        .ok_or_else(|| limitation("worktree path must be unicode"))?;
    if !args.iter().any(|arg| arg == "-C" || arg == "--cd") {
        args.splice(0..0, ["-C".into(), cwd.into()]);
    }
    Ok(args)
}

pub fn record(path: &Path, mapping: &Mapping) -> io::Result<()> {
    fs::create_dir_all(path.parent().unwrap_or(path))?;
    fs::write(path, serde_json::to_vec_pretty(mapping)?)?;
    Ok(())
}

pub fn load(path: &Path) -> io::Result<Mapping> {
    serde_json::from_slice(&fs::read(path)?).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("worktree mapping: {error}"),
        )
    })
}

pub fn refuse_shared_checkout(shared: &Path, mapping: &Mapping) -> io::Result<()> {
    let shared = fs::canonicalize(shared).unwrap_or_else(|_| shared.to_path_buf());
    let tree = fs::canonicalize(&mapping.path).unwrap_or_else(|_| mapping.path.clone());
    if shared == tree {
        return Err(limitation(
            "executor would write to the shared checkout; refusing substitution",
        ));
    }
    Ok(())
}

pub fn native_delete_eligible(mapping: &Mapping, current: &Path) -> io::Result<Result<(), String>> {
    let current = fs::canonicalize(current).unwrap_or_else(|_| current.to_path_buf());
    let tree = fs::canonicalize(&mapping.path).unwrap_or_else(|_| mapping.path.clone());
    if current == tree {
        return Ok(Err(
            "native confirmed deletion refuses the current checkout".into(),
        ));
    }
    if mapping.archived || mapping.unavailable {
        return Ok(Err(
            "agents-overview archive or unavailability is not worktree retirement".into(),
        ));
    }
    if !mapping.path.is_dir() {
        return Ok(Err("managed worktree path is missing".into()));
    }
    let inside = git(&mapping.path, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim() != "true" {
        return Ok(Err("checkout is not a Git worktree".into()));
    }
    let porcelain = git(
        &mapping.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !porcelain.trim().is_empty() {
        return Ok(Err(
            "native confirmed deletion refuses local or untracked changes".into(),
        ));
    }
    let ignored = git(
        &mapping.path,
        &["ls-files", "--others", "--ignored", "--exclude-standard"],
    )?;
    if !ignored.trim().is_empty() {
        return Ok(Err("native confirmed deletion refuses ignored files".into()));
    }
    Ok(Ok(()))
}

pub fn retire(mapping: &Mapping, current: &Path) -> io::Result<Retirement> {
    match native_delete_eligible(mapping, current)? {
        Ok(()) => Ok(Retirement::Preserved {
            limitation: "native confirmed deletion is TUI-only on CLI 0.155.1; checkout preserved"
                .into(),
        }),
        Err(limitation) => Ok(Retirement::Preserved { limitation }),
    }
}

/// Reset a lane worktree to the committed base the lead merged, so the next
/// lane task starts from the new base with the lane's ignored build caches
/// intact (`git reset --hard <base>` plus `git clean -fd`). Unresettable state
/// is preserved with its reason for lane retirement instead of deleting or
/// reusing it; the lane inventory guard (`audit`) is unchanged because a reset
/// lane is not a new worktree.
pub fn reset_for_reuse(
    mapping: &Mapping,
    current: &Path,
    base: &str,
) -> io::Result<LaneDisposition> {
    let current = fs::canonicalize(current).unwrap_or_else(|_| current.to_path_buf());
    let tree = fs::canonicalize(&mapping.path).unwrap_or_else(|_| mapping.path.clone());
    if current == tree {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane reset refuses the current checkout".into(),
        });
    }
    if mapping.archived || mapping.unavailable {
        return Ok(LaneDisposition::Preserved {
            limitation: "agents-overview archive or unavailability is not lane reuse".into(),
        });
    }
    if !mapping.path.is_dir() {
        return Ok(LaneDisposition::Preserved {
            limitation: "managed worktree path is missing".into(),
        });
    }
    let inside = git(&mapping.path, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim() != "true" {
        return Ok(LaneDisposition::Preserved {
            limitation: "checkout is not a Git worktree".into(),
        });
    }
    let Ok(commit) = git(
        &mapping.path,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    ) else {
        return Ok(LaneDisposition::Preserved {
            limitation: "committed base is not available in the lane; preserving the lane".into(),
        });
    };
    let commit = commit.trim().to_owned();
    if git(&mapping.path, &["reset", "--hard", &commit]).is_err() {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane reset failed; preserving the lane for retirement".into(),
        });
    }
    if git(&mapping.path, &["clean", "-fd"]).is_err() {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane cleanup failed; preserving the lane for retirement".into(),
        });
    }
    let head = git(&mapping.path, &["rev-parse", "HEAD"])?;
    if head.trim() != commit {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane did not reach the merged base; preserving the lane for retirement"
                .into(),
        });
    }
    let porcelain = git(
        &mapping.path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !porcelain.trim().is_empty() {
        return Ok(LaneDisposition::Preserved {
            limitation: "lane is not clean after reset; preserving the lane for retirement".into(),
        });
    }
    Ok(LaneDisposition::Reused { base: commit })
}

pub fn exec_isolation_args(codex_home: &Path, workspace: &Path) -> io::Result<Vec<String>> {
    if !is_shared_git_checkout(workspace)? {
        return Ok(Vec::new());
    }
    if !worktrees_enabled(codex_home)? {
        return Err(limitation(
            "experimental feature worktrees is disabled; refusing the shared checkout",
        ));
    }
    Ok(vec![
        "--enable".into(),
        "worktrees".into(),
        "--worktree".into(),
    ])
}

pub fn is_shared_git_checkout(path: &Path) -> io::Result<bool> {
    Ok(path.join(".git").is_dir())
}

pub fn is_git_checkout(path: &Path) -> io::Result<bool> {
    Ok(path.join(".git").exists())
}

/// Lane inventory guard: lanes are reused, not multiplied. The authoritative
/// list is `git worktree list`; the lead warns and cleans up instead of
/// silently accumulating worktrees when a lane was not reset or retired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeAudit {
    pub total: u32,
    pub paths: Vec<PathBuf>,
}

pub fn audit(source: &Path) -> io::Result<WorktreeAudit> {
    // A source that is not a Git checkout has no registered lanes; the guard
    // is a lane-accumulation warning and must not fail dispatch with an
    // unrelated `git worktree` error.
    if !is_git_checkout(source)? {
        return Ok(WorktreeAudit {
            total: 0,
            paths: Vec::new(),
        });
    }
    let output = git(source, &["worktree", "list", "--porcelain"])?;
    let mut paths = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            paths.push(native_path(path));
        }
    }
    Ok(WorktreeAudit {
        total: paths.len() as u32,
        paths,
    })
}

/// `git worktree list --porcelain` reports forward-slash paths on Windows; the
/// guard compares them with native paths, so normalize the separator and drop
/// the verbatim prefix `Path::canonicalize` adds.
fn native_path(path: &str) -> PathBuf {
    let text = path.strip_prefix(r"\\?\").unwrap_or(path);
    if cfg!(windows) {
        PathBuf::from(text.replace('/', "\\"))
    } else {
        PathBuf::from(text)
    }
}

#[cfg(test)]
fn allocate_in_pool(
    codex_home: &Path,
    source: &Path,
    head: &str,
    owner_thread: Option<String>,
) -> io::Result<Mapping> {
    if !worktrees_enabled(codex_home)? {
        return Err(limitation(
            "experimental feature worktrees is disabled; refusing the shared checkout",
        ));
    }
    let source = fs::canonicalize(source)?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| limitation("repository name must be unicode"))?;
    let id = owner_thread
        .clone()
        .unwrap_or_else(|| format!("{:x}", std::process::id()));
    let path = codex_home
        .join("worktrees")
        .join(&id[..8.min(id.len())])
        .join(name);
    fs::create_dir_all(path.parent().unwrap())?;
    let status = Command::new("git")
        .args(["worktree", "add", "--detach", path.to_str().unwrap(), head])
        .current_dir(&source)
        .status()?;
    if !status.success() {
        return Err(limitation(
            "native managed allocation failed; ordinary Git worktree substitution is refused",
        ));
    }
    fs::write(path.join(".codex-managed-worktree"), b"codex-cli-managed\n")?;
    Ok(Mapping {
        schema: 1,
        path,
        source,
        head: head.to_owned(),
        owner_thread,
        archived: false,
        unavailable: false,
        remote_tui_omits_worktree_flag: true,
    })
}

fn git(cwd: &Path, args: &[&str]) -> io::Result<String> {
    let out = Command::new("git").args(args).current_dir(cwd).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(io::Error::other(format!(
            "git {}: {}",
            args.first().unwrap_or(&"git"),
            String::from_utf8_lossy(&out.stderr)
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_counts_the_authoritative_worktree_inventory() {
        let root = tempfile::tempdir().unwrap();
        let repo = repo(root.path());
        let lane = root.path().join("lane");
        git_ok(
            &repo,
            &["worktree", "add", "--detach", lane.to_str().unwrap()],
        );
        let audit = audit(&repo).unwrap();
        assert_eq!(audit.total, 2);
        assert!(audit.paths.contains(&repo));
        // The guard reports native paths (`git` prints forward slashes on
        // Windows); it must not report the verbatim `canonicalize` form.
        assert!(audit.paths.contains(&lane), "{:?}", audit.paths);
    }

    fn repo(root: &Path) -> PathBuf {
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        git_ok(&repo, &["init", "-q"]);
        git_ok(&repo, &["config", "user.email", "worktree@example.test"]);
        git_ok(&repo, &["config", "user.name", "Worktree"]);
        fs::write(repo.join("README.md"), "shared\n").unwrap();
        git_ok(&repo, &["add", "README.md"]);
        git_ok(&repo, &["commit", "-qm", "seed"]);
        repo
    }

    fn git_ok(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn remote_tui_rejects_worktree_flag_and_attaches_cwd() {
        let mapping = Mapping {
            schema: 1,
            path: PathBuf::from(r"D:\wt\exec"),
            source: PathBuf::from(r"D:\repo"),
            head: "abc".into(),
            owner_thread: Some("thread-1".into()),
            archived: false,
            unavailable: false,
            remote_tui_omits_worktree_flag: true,
        };
        let error = remote_tui_args(
            &mapping,
            &[
                "--remote".into(),
                "ws://127.0.0.1:1".into(),
                "--worktree".into(),
            ],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rejects --worktree with --remote")
        );
        let args = remote_tui_args(
            &mapping,
            &[
                "--remote".into(),
                "ws://127.0.0.1:1".into(),
                "resume".into(),
            ],
        )
        .unwrap();
        assert_eq!(args[0], "-C");
        assert_eq!(args[1], r"D:\wt\exec");
        assert!(!args.iter().any(|arg| arg == "--worktree"));
    }

    #[test]
    fn exec_isolation_uses_native_worktree_flag_not_ordinary_git() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let workspace = root.path().join("scratch");
        fs::create_dir_all(&workspace).unwrap();
        assert!(exec_isolation_args(&home, &workspace).unwrap().is_empty());
        let source = repo(root.path());
        let error = exec_isolation_args(&home, &source).unwrap_err();
        assert!(error.to_string().contains("refusing the shared checkout"));
        fs::write(home.join("config.toml"), "features.worktrees = true\n").unwrap();
        assert_eq!(
            exec_isolation_args(&home, &source).unwrap(),
            ["--enable", "worktrees", "--worktree"]
        );
    }

    #[test]
    fn disabled_feature_refuses_shared_checkout() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let error = allocate_in_pool(&home, root.path(), "HEAD", None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("refusing the shared checkout"));
        assert!(!error.to_string().contains("substitut"));
    }

    #[test]
    fn allocated_tree_is_not_the_shared_checkout_and_dirty_delete_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("config.toml"), "features.worktrees = true\n").unwrap();
        let source = repo(root.path());
        let mapping = allocate_in_pool(&home, &source, "HEAD", Some("exec-1".into())).unwrap();
        let mapping_b = allocate_in_pool(&home, &source, "HEAD", Some("exec-2".into())).unwrap();
        refuse_shared_checkout(&source, &mapping).unwrap();
        assert_ne!(mapping.path, mapping_b.path);
        assert_ne!(mapping.owner_thread, mapping_b.owner_thread);
        assert!(mapping.path.starts_with(home.join("worktrees")));
        assert!(mapping.remote_tui_omits_worktree_flag);
        fs::write(mapping.path.join("scratch.txt"), "dirty\n").unwrap();
        let retired = retire(&mapping, &source).unwrap();
        match retired {
            Retirement::Preserved { limitation } => {
                assert!(limitation.contains("untracked") || limitation.contains("TUI-only"));
            }
            Retirement::Deleted => panic!("dirty tree must not be deleted"),
        }
        assert!(mapping.path.exists());
        record(&source.join("executor-worktree.json"), &mapping).unwrap();
        assert_eq!(
            load(&source.join("executor-worktree.json")).unwrap().head,
            mapping.head
        );
    }

    #[test]
    fn overview_archive_is_not_retirement() {
        let mapping = Mapping {
            schema: 1,
            path: PathBuf::from("."),
            source: PathBuf::from("."),
            head: "HEAD".into(),
            owner_thread: None,
            archived: true,
            unavailable: false,
            remote_tui_omits_worktree_flag: true,
        };
        let retired = retire(&mapping, Path::new("..")).unwrap();
        match retired {
            Retirement::Preserved { limitation } => {
                assert!(limitation.contains("archive"));
            }
            Retirement::Deleted => panic!("archive must not delete"),
        }
    }

    #[test]
    fn audit_of_a_non_git_source_has_no_lanes() {
        let root = tempfile::tempdir().unwrap();
        let exported = root.path().join("packaged-kit");
        fs::create_dir_all(&exported).unwrap();
        let audit = audit(&exported).unwrap();
        assert_eq!(audit.total, 0);
        assert!(audit.paths.is_empty());
    }

    #[test]
    fn lane_reset_reuses_the_checkout_and_keeps_ignored_build_caches() {
        let root = tempfile::tempdir().unwrap();
        let repo = repo(root.path());
        fs::write(repo.join(".gitignore"), "target/\n").unwrap();
        git_ok(&repo, &["add", ".gitignore"]);
        git_ok(&repo, &["commit", "-qm", "ignore build output"]);
        let lane = root.path().join("lane");
        git_ok(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                lane.to_str().unwrap(),
                "HEAD",
            ],
        );
        fs::create_dir_all(lane.join("target")).unwrap();
        fs::write(lane.join("target/cache.bin"), "warm\n").unwrap();
        fs::write(lane.join("scratch.txt"), "leftover\n").unwrap();
        fs::write(lane.join("README.md"), "executor edit\n").unwrap();
        // The lead's accepted merge moves the shared checkout to the new base.
        fs::write(repo.join("merged.txt"), "accepted\n").unwrap();
        git_ok(&repo, &["add", "merged.txt"]);
        git_ok(&repo, &["commit", "-qm", "accepted merge"]);
        let base = rev(&repo);

        let mapping = Mapping {
            schema: 1,
            path: lane.clone(),
            source: repo.clone(),
            head: base.clone(),
            owner_thread: Some("exec-1".into()),
            archived: false,
            unavailable: false,
            remote_tui_omits_worktree_flag: true,
        };
        let disposition = reset_for_reuse(&mapping, &repo, &base).unwrap();
        assert_eq!(disposition, LaneDisposition::Reused { base: base.clone() });
        assert_eq!(rev(&lane), base);
        assert_eq!(
            fs::read(lane.join("target/cache.bin")).unwrap(),
            b"warm\n",
            "ignored build caches stay for the next lane task"
        );
        assert!(!lane.join("scratch.txt").exists());
        let readme = fs::read_to_string(lane.join("README.md")).unwrap();
        assert!(readme.contains("shared"), "{readme}");
        assert!(!readme.contains("executor edit"), "{readme}");
        let status = git(
            &lane,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
        .unwrap();
        assert!(status.trim().is_empty(), "{status}");
        assert_eq!(
            audit(&repo).unwrap().total,
            2,
            "a reused lane adds no worktree"
        );
    }

    #[test]
    fn lane_reset_preserves_unresettable_state_for_retirement() {
        let root = tempfile::tempdir().unwrap();
        let repo = repo(root.path());
        let lane = root.path().join("lane");
        git_ok(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                lane.to_str().unwrap(),
                "HEAD",
            ],
        );
        let mapping = Mapping {
            schema: 1,
            path: lane.clone(),
            source: repo.clone(),
            head: "HEAD".into(),
            owner_thread: Some("exec-1".into()),
            archived: false,
            unavailable: false,
            remote_tui_omits_worktree_flag: true,
        };
        fs::write(lane.join("scratch.txt"), "dirty\n").unwrap();
        match reset_for_reuse(&mapping, &repo, "not-a-commit").unwrap() {
            LaneDisposition::Preserved { limitation } => {
                assert!(
                    limitation.contains("committed base"),
                    "unavailable base must be explicit: {limitation}"
                );
            }
            LaneDisposition::Reused { .. } => panic!("unknown base must not reset the lane"),
        }
        assert!(lane.join("scratch.txt").exists());
        match reset_for_reuse(&mapping, &lane, "HEAD").unwrap() {
            LaneDisposition::Preserved { limitation } => {
                assert!(limitation.contains("current checkout"));
            }
            LaneDisposition::Reused { .. } => panic!("the current checkout must not reset"),
        }
        let archived = Mapping {
            archived: true,
            ..mapping.clone()
        };
        match reset_for_reuse(&archived, &repo, "HEAD").unwrap() {
            LaneDisposition::Preserved { limitation } => {
                assert!(limitation.contains("archive"));
            }
            LaneDisposition::Reused { .. } => panic!("archive is not lane reuse"),
        }
        let missing_path = Mapping {
            path: root.path().join("gone"),
            ..mapping
        };
        match reset_for_reuse(&missing_path, &repo, "HEAD").unwrap() {
            LaneDisposition::Preserved { limitation } => {
                assert!(limitation.contains("missing"));
            }
            LaneDisposition::Reused { .. } => panic!("a missing lane cannot be reused"),
        }
    }

    fn rev(cwd: &Path) -> String {
        git(cwd, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
    }
}
