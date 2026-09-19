//! Non-interactive `bd` contract. The controller does not parse the board.
use serde_json::Value;
use std::{
    fs, io,
    path::Path,
    process::{Command, Output},
};

pub const VERSION: &str = "1.3.0";
pub const RELEASE_TAG: &str = "v1.3.0";
pub const WINDOWS_AMD64_ARCHIVE: &str = "beads_1.3.0_windows_amd64.zip";
pub const WINDOWS_AMD64_SHA256: &str =
    "fa4c72c5d27f68f906e89a759656b5a051b330088c4acc38fa445cbb561e5cdf";
pub const FEEDBACK_LABEL: &str = "feedback";
pub const INCUBATOR_LABEL: &str = "incubator";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub epic_id: String,
    pub feature_id: String,
    pub feedback_id: String,
    pub improvement_id: String,
}

pub fn unavailable(detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("board unavailable: {detail}"),
    )
}

pub fn verify_noninteractive_contract(bd: &Path, project: &Path) -> io::Result<Contract> {
    if !bd.is_file() {
        return Err(unavailable(&format!(
            "bd executable is missing at {}",
            bd.display()
        )));
    }
    seed_git(project)?;
    let init = run(
        bd,
        project,
        &[
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "bdct",
        ],
    )?;
    if !init.status.success() {
        return Err(failed("init", &init));
    }
    if project.join("AGENTS.md").exists() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bd init --skip-agents wrote AGENTS.md",
        ));
    }

    let version = run(bd, project, &["version"])?;
    let version_text = String::from_utf8_lossy(&version.stdout);
    if !version_text.contains(VERSION) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected bd version: {version_text}"),
        ));
    }

    let epic = json_ok(
        bd,
        project,
        &[
            "create",
            "Stage: synthetic orchestration",
            "--type",
            "epic",
            "--description",
            "Isolated Windows board-contract stage.",
            "--json",
        ],
    )?;
    let epic_id = string_field(&epic, "id")?;
    if string_field(&epic, "issue_type")? != "epic" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stage was not created as an epic",
        ));
    }

    let feature = json_ok(
        bd,
        project,
        &[
            "create",
            "Spec: board command contract",
            "--type",
            "feature",
            "--parent",
            &epic_id,
            "--description",
            "Verify non-interactive epic, feature, feedback and status operations.",
            "--json",
        ],
    )?;
    let feature_id = string_field(&feature, "id")?;
    if string_field(&feature, "issue_type")? != "feature" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "specification was not created as a feature",
        ));
    }

    let rejected = run(
        bd,
        project,
        &["create", "Executor blocker", "--type", "feedback", "--json"],
    )?;
    if rejected.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bd accepted issue type feedback",
        ));
    }
    let rejected_text = String::from_utf8_lossy(&rejected.stdout);
    if !rejected_text.contains("invalid issue type: feedback") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected feedback-type error: {rejected_text}"),
        ));
    }

    let feedback = json_ok(
        bd,
        project,
        &[
            "create",
            "Executor blocker: missing fixture",
            "--type",
            "task",
            "--labels",
            FEEDBACK_LABEL,
            "--parent",
            &feature_id,
            "--description",
            "Synthetic executor cannot proceed without a named fixture.",
            "--json",
        ],
    )?;
    let feedback_id = string_field(&feedback, "id")?;

    let listed = json_ok(
        bd,
        project,
        &["list", "--label", FEEDBACK_LABEL, "--json", "--brief"],
    )?;
    let Some(rows) = listed.as_array() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "feedback list was not an array",
        ));
    };
    if !rows
        .iter()
        .any(|row| row["id"].as_str() == Some(&feedback_id))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "feedback task missing from label list",
        ));
    }

    json_ok(bd, project, &["status", "--json", "--no-activity"])?;
    json_ok(bd, project, &["epic", "status", "--json"])?;
    json_ok(
        bd,
        project,
        &[
            "close",
            &feedback_id,
            "--reason",
            "Lead resolved.",
            "--json",
        ],
    )?;

    let remote = Command::new(bd)
        .args([
            "-C",
            project
                .to_str()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "project path"))?,
            "status",
            "--json",
            "--no-activity",
        ])
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", "board-contract")
        .output()?;
    if !remote.status.success() {
        return Err(failed("-C status", &remote));
    }

    let improvement = json_ok(
        bd,
        project,
        &[
            "create",
            "Improvement: record fixture in the spec",
            "--type",
            "task",
            "--parent",
            &epic_id,
            "--description",
            "Lead-initiated follow-up that does not change accepted requirements.",
            "--json",
        ],
    )?;
    let improvement_id = string_field(&improvement, "id")?;
    let change = project.join("openspec/changes/synthetic-board-improvement");
    fs::create_dir_all(&change)?;
    fs::write(
        change.join("proposal.md"),
        "Synthetic OpenSpec change for an accepted-requirement improvement.\n",
    )?;
    Ok(Contract {
        epic_id,
        feature_id,
        feedback_id,
        improvement_id,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncubatorContract {
    pub canonical_id: String,
    pub duplicate_id: String,
    pub related_id: String,
}

pub fn verify_incubator_contract(bd: &Path, project: &Path) -> io::Result<IncubatorContract> {
    let contract = verify_noninteractive_contract(bd, project)?;
    let canonical = json_ok(
        bd,
        project,
        &[
            "create",
            "Incubator: dispatch waits too long",
            "--type",
            "task",
            "--labels",
            "incubator",
            "--parent",
            &contract.feature_id,
            "--description",
            "Synthetic recurring wait after tool completion.",
            "--json",
        ],
    )?;
    let canonical_id = string_field(&canonical, "id")?;
    let duplicate = json_ok(
        bd,
        project,
        &[
            "create",
            "Incubator: dispatch waits too long",
            "--type",
            "task",
            "--labels",
            "incubator",
            "--parent",
            &contract.feature_id,
            "--description",
            "Synthetic recurring wait after tool completion.",
            "--json",
        ],
    )?;
    let duplicate_id = string_field(&duplicate, "id")?;
    let related = json_ok(
        bd,
        project,
        &[
            "create",
            "Incubator: related wait observation",
            "--type",
            "task",
            "--labels",
            "incubator",
            "--parent",
            &contract.feature_id,
            "--description",
            "Same wait, different reporter episode.",
            "--json",
        ],
    )?;
    let related_id = string_field(&related, "id")?;

    json_ok(
        bd,
        project,
        &[
            "link",
            &related_id,
            &canonical_id,
            "--type",
            "related",
            "--json",
        ],
    )?;
    json_ok(
        bd,
        project,
        &["duplicate", &duplicate_id, "--of", &canonical_id, "--json"],
    )?;

    run_actor(
        bd,
        project,
        "exec-a",
        &[
            "comment",
            &canonical_id,
            "--json",
            "vote episode=e1 reporter=exec-a",
        ],
    )?;
    run_actor(
        bd,
        project,
        "exec-b",
        &[
            "comment",
            &canonical_id,
            "--json",
            "vote episode=e2 reporter=exec-b",
        ],
    )?;
    run_actor(
        bd,
        project,
        "exec-a",
        &[
            "comment",
            &canonical_id,
            "--json",
            "vote episode=e1 reporter=exec-a",
        ],
    )?;

    json_ok(
        bd,
        project,
        &["label", "add", &canonical_id, "backlog", "--json"],
    )?;
    json_ok(
        bd,
        project,
        &["label", "remove", &canonical_id, "incubator", "--json"],
    )?;
    json_ok(
        bd,
        project,
        &[
            "close",
            &related_id,
            "--reason",
            "archived: stale incubator item",
            "--json",
        ],
    )?;

    let listed = json_ok(
        bd,
        project,
        &["list", "--label", "incubator", "--json", "--brief"],
    )?;
    let incubator_rows = listed.as_array().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "incubator list was not an array",
        )
    })?;
    if incubator_rows
        .iter()
        .any(|row| row["id"].as_str() == Some(&canonical_id))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "promoted item remained on incubator label",
        ));
    }
    json_ok(bd, project, &["status", "--json", "--no-activity"])?;
    json_ok(bd, project, &["comments", &canonical_id, "--json"])?;

    let shown = json_ok(bd, project, &["show", &duplicate_id, "--json"])?;
    let closed = match &shown {
        Value::Array(rows) => rows.iter().any(|row| {
            row["id"].as_str() == Some(&duplicate_id)
                && (row["status"] == "closed" || row.get("closed_at").is_some())
        }),
        Value::Object(_) => {
            shown["id"].as_str() == Some(&duplicate_id)
                && (shown["status"] == "closed" || shown.get("closed_at").is_some())
        }
        _ => false,
    };
    if !closed {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("duplicate was not closed after merge: {shown}"),
        ));
    }

    Ok(IncubatorContract {
        canonical_id,
        duplicate_id,
        related_id,
    })
}

fn run_actor(bd: &Path, project: &Path, actor: &str, args: &[&str]) -> io::Result<Output> {
    let out = command(bd, project, actor).args(args).output()?;
    if !out.status.success() {
        return Err(failed(args.first().unwrap_or(&"bd"), &out));
    }
    Ok(out)
}

pub(crate) fn seed_git(project: &Path) -> io::Result<()> {
    fs::create_dir_all(project)?;
    git(project, &["init", "-q"])?;
    git(
        project,
        &["config", "user.email", "board-contract@example.test"],
    )?;
    git(project, &["config", "user.name", "Board Contract"])?;
    fs::write(
        project.join("README.md"),
        "Synthetic board-contract fixture.\n",
    )?;
    git(project, &["add", "README.md"])?;
    git(project, &["commit", "-qm", "seed"])?;
    Ok(())
}

fn git(project: &Path, args: &[&str]) -> io::Result<()> {
    let out = Command::new("git")
        .args(args)
        .current_dir(project)
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(failed("git", &out))
    }
}

pub(crate) fn json_ok(bd: &Path, project: &Path, args: &[&str]) -> io::Result<Value> {
    json_ok_actor(bd, project, "board-contract", args)
}

pub(crate) fn json_ok_actor(
    bd: &Path,
    project: &Path,
    actor: &str,
    args: &[&str],
) -> io::Result<Value> {
    let out = run_actor(bd, project, actor, args)?;
    serde_json::from_slice(&out.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("bd JSON: {error}")))
}

pub(crate) fn run(bd: &Path, project: &Path, args: &[&str]) -> io::Result<Output> {
    command(bd, project, "board-contract").args(args).output()
}

fn command(bd: &Path, project: &Path, actor: &str) -> Command {
    let mut cmd = Command::new(bd);
    cmd.current_dir(project)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", actor);
    if let Some(path) = child_path() {
        cmd.env("PATH", path);
    }
    cmd
}

fn child_path() -> Option<std::ffi::OsString> {
    let path = std::env::var_os("PATH")?;
    let filtered = std::env::split_paths(&path).filter(|entry| {
        let lower = entry.to_string_lossy().to_ascii_lowercase();
        !(lower.ends_with(r"\windowsapps") || lower.contains(r"\windowsapps\"))
    });
    std::env::join_paths(filtered).ok()
}

pub(crate) fn string_field(value: &Value, field: &str) -> io::Result<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing bd field {field}"),
            )
        })
}

pub(crate) fn failed(op: &str, out: &Output) -> io::Error {
    io::Error::other(format!(
        "{op} failed: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn pin_and_unavailable_error_are_explicit() {
        assert_eq!(VERSION, "1.3.0");
        assert_eq!(WINDOWS_AMD64_ARCHIVE, "beads_1.3.0_windows_amd64.zip");
        assert_eq!(
            WINDOWS_AMD64_SHA256,
            "fa4c72c5d27f68f906e89a759656b5a051b330088c4acc38fa445cbb561e5cdf"
        );
        let error = unavailable("bd is not installed");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("board unavailable"));
        assert!(!error.to_string().contains("substitut"));
    }

    #[test]
    fn team_lead_skill_requires_explicit_activation() {
        let text = include_str!("../../../.agents/skills/team-lead/SKILL.md");
        assert!(text.contains("name: team-lead"));
        assert!(text.contains("orchestrated asynchronous development"));
        assert!(text.contains("must not spawn executors"));
        assert!(text.contains("board-workflow"));
        assert!(text.contains("executor spawn"));
        assert!(text.contains("Do not use for a small direct task"));
    }

    #[test]
    #[ignore = "requires HARNESS_BD_EXE pointing at checksummed beads v1.3.0; owned temp project only"]
    fn windows_noninteractive_epic_feature_feedback_and_status() {
        let bd = PathBuf::from(std::env::var_os("HARNESS_BD_EXE").expect("HARNESS_BD_EXE"));
        let root = std::env::temp_dir().join(format!(
            "board-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let project = root.join("orch-board-contract");
        let contract = verify_noninteractive_contract(&bd, &project).unwrap();
        assert!(
            contract
                .feature_id
                .starts_with(&format!("{}.", contract.epic_id))
        );
        assert!(
            contract
                .feedback_id
                .starts_with(&format!("{}.", contract.feature_id))
        );
        assert!(!contract.improvement_id.is_empty());
        assert!(
            project
                .join("openspec/changes/synthetic-board-improvement/proposal.md")
                .is_file()
        );
        assert!(!project.join("AGENTS.md").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    #[ignore = "requires HARNESS_BD_EXE pointing at checksummed beads v1.3.0; owned temp project only"]
    fn windows_noninteractive_merge_vote_promote_and_archive() {
        let bd = PathBuf::from(std::env::var_os("HARNESS_BD_EXE").expect("HARNESS_BD_EXE"));
        let root = std::env::temp_dir().join(format!(
            "board-incubator-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let project = root.join("orch-incubator-contract");
        let contract = verify_incubator_contract(&bd, &project).unwrap();
        assert!(!contract.canonical_id.is_empty());
        assert_ne!(contract.canonical_id, contract.duplicate_id);
        assert_ne!(contract.canonical_id, contract.related_id);
        let _ = fs::remove_dir_all(&root);
    }
}
