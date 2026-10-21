//! Model-free reflection consumption owned by the existing managed lead host.
//! No child exists while the mode is off; the host's Job contains every child
//! once enabled, including the native reflection app-servers.

use super::*;
use harness_core::{
    delivery_reflection::{LifecycleBinding, LifecycleStatus, SourceIdentity},
    process::{CommandSpec, OwnedProcess},
};
use std::io::BufReader;

const JOURNAL_POLL: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct Header {
    lifecycle: LifecycleStatus,
    binding: LifecycleBinding,
    source: SourceIdentity,
}

pub(super) struct ActiveBoard {
    pub project: PathBuf,
    pub state_file: PathBuf,
    generation: String,
}

/// Read only the header; serde skips retained results without allocating them.
pub(super) fn active_boards(home: &Path, lead: &str) -> io::Result<Vec<ActiveBoard>> {
    let directory = home.join("harness/delivery-reflection");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut boards = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let header: Header = serde_json::from_reader(BufReader::new(fs::File::open(&path)?))
            .map_err(|error| {
                invalid(format!(
                    "reflection lifecycle {} is unreadable: {error}",
                    path.display()
                ))
            })?;
        if header.lifecycle == LifecycleStatus::Active && header.binding.lead_session == lead {
            boards.push(ActiveBoard {
                project: PathBuf::from(header.source.repo_root),
                state_file: path,
                generation: format!(
                    "{}:{}:{}",
                    header.binding.id, header.binding.revision, header.binding.activated_at
                ),
            });
        }
    }
    boards.sort_by(|left, right| left.state_file.cmp(&right.state_file));
    Ok(boards)
}

fn status_path(home: &Path, lead: &str) -> PathBuf {
    home.join("harness/delivery-reflection/hosts")
        .join(format!("{}.json", digest(lead)))
}

fn kit_source(home: &Path, candidate: &Path) -> io::Result<PathBuf> {
    if candidate.join("global/roles.toml").is_file() {
        return Ok(candidate.to_owned());
    }
    crate::feedback_cli::installed_kit_source_in(home)
        .ok_or_else(|| invalid("the managed lead has no installed kit source"))
}

fn board_initialized(project: &Path) -> bool {
    project
        .ancestors()
        .any(|directory| directory.join(".beads/config.yaml").is_file())
}

pub(super) fn status(home: &Path, lead: &str) -> io::Result<Value> {
    let path = status_path(home, lead);
    if !path.is_file() {
        return Ok(
            json!({"state": "unobserved", "detail": "no managed lead host has observed this reflection lifecycle"}),
        );
    }
    let mut value: Value = read_json(&path, MAX_REQUEST_BYTES)?;
    if value["lead"].as_str() != Some(lead) {
        return Err(invalid(
            "the reflection host status belongs to another lead",
        ));
    }
    let live = value
        .get("process")
        .cloned()
        .and_then(|value| serde_json::from_value::<control::EndpointProcess>(value).ok())
        .is_some_and(|process| endpoint_process_live(&process));
    value["processLive"] = json!(live);
    if value["state"] == "running" && !live {
        value["state"] = json!("unavailable");
        value["detail"] = json!(
            "the recorded controller is no longer live; retained stage outcomes need reconciliation before continuation"
        );
    }
    Ok(value)
}

pub(crate) struct LeadLifecycle {
    home: PathBuf,
    source: PathBuf,
    lead: String,
    job: Option<Job>,
    child: Option<OwnedProcess>,
    next_poll: Instant,
    observed_enabled: Option<bool>,
    last_attempt: Option<String>,
    failed: bool,
}

impl LeadLifecycle {
    pub(crate) fn new(home: &Path, source: &Path, lead: &str) -> Self {
        Self {
            home: home.to_owned(),
            source: source.to_owned(),
            lead: lead.to_owned(),
            job: None,
            child: None,
            next_poll: Instant::now(),
            observed_enabled: None,
            last_attempt: None,
            failed: false,
        }
    }

    pub(crate) fn tick(&mut self) -> io::Result<()> {
        if self.failed || Instant::now() < self.next_poll {
            return Ok(());
        }
        self.next_poll = Instant::now() + JOURNAL_POLL;
        if let Err(error) = self.tick_inner() {
            // One failure is surfaced once. No loop retries the same uncertain
            // effect. The lead can inspect the existing receipt and log.
            self.failed = true;
            let _ = self.record("failed", None, &error.to_string());
            return Err(error);
        }
        Ok(())
    }

    fn tick_inner(&mut self) -> io::Result<()> {
        let child_running = if let Some(child) = &self.child {
            if child.is_running()? {
                true
            } else {
                let code = child.exit_code()?.unwrap_or(1);
                self.child = None;
                if let Some(job) = self.job.take() {
                    job.terminate(1, Duration::from_secs(5))?;
                }
                self.record(if code == 0 { "stopped" } else { "failed" }, None,
                    &format!("controller exited with code {code}; see the retained log before disable/enable or source recovery"))?;
                false
            }
        } else {
            false
        };
        if !board_initialized(&self.source) {
            // A managed lead may start before its project's native board
            // is initialized. Keep the existing poll boundary and retry
            // once the board owner writes its configuration; no error is
            // recorded and no daemon or model probe is introduced.
            return Ok(());
        }
        let source = kit_source(&self.home, &self.source)?;
        let gate = gate::ReflectionGate::load(&source)?;
        if !gate.enabled {
            self.observed_enabled = Some(false);
            // A later true observation is a new preparation transition and may
            // start the model-free consumer even for the same retained binding.
            self.last_attempt = None;
            return Ok(());
        }
        if self.observed_enabled != Some(true) {
            match lifecycle::prepare_automatic(
                &self.home,
                &source,
                &self.source,
                &self.lead,
                lifecycle::PreparationTrigger::Host,
            )? {
                lifecycle::Preparation::Active => {
                    self.observed_enabled = Some(true);
                }
                lifecycle::Preparation::Disabled | lifecycle::Preparation::Stopped => {
                    self.observed_enabled = Some(false);
                    self.last_attempt = None;
                    return Ok(());
                }
            }
        }
        if child_running {
            // The existing consumer re-reads the gate for every stage. Keep the
            // host's transition state current without launching a duplicate.
            return Ok(());
        }
        let boards = active_boards(&self.home, &self.lead)?;
        if boards.is_empty() {
            return Ok(());
        }
        let generation = boards
            .iter()
            .map(|board| board.generation.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if self.last_attempt.as_ref() == Some(&generation) {
            return Ok(());
        }
        self.last_attempt = Some(generation);
        let path = status_path(&self.home, &self.lead);
        fs::create_dir_all(path.parent().unwrap())?;
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.with_extension("log"))?;
        let program = std::env::current_exe()?.canonicalize()?;
        let mut spec = CommandSpec::new(&program);
        spec.args = vec![
            "delivery-improvement".into(),
            "_serve".into(),
            "--source".into(),
            source.into_os_string(),
            "--project".into(),
            self.source.as_os_str().to_owned(),
            "--codex-home".into(),
            self.home.as_os_str().to_owned(),
        ];
        spec.current_dir = Some(self.source.clone());
        spec.env
            .insert("CODEX_HOME".into(), Some(self.home.as_os_str().to_owned()));
        spec.env
            .insert("CODEX_THREAD_ID".into(), Some(self.lead.clone().into()));
        spec.env.insert(
            "HARNESS_REFLECTION_HOST".into(),
            Some(self.lead.clone().into()),
        );
        spec.stdout = Some(log.try_clone()?);
        spec.stderr = Some(log);
        let job = Job::new(Limits::default())?;
        let child = job.spawn(&spec)?;
        let identity = child.identity();
        self.job = Some(job);
        self.child = Some(child);
        self.record(
            "running",
            Some(control::EndpointProcess {
                pid: identity.pid,
                creation_time: identity.creation_time,
                program,
            }),
            "the existing managed lead host owns this model-free journal consumer",
        )
    }

    fn record(
        &self,
        state: &str,
        process: Option<control::EndpointProcess>,
        detail: &str,
    ) -> io::Result<()> {
        let path = status_path(&self.home, &self.lead);
        fs::create_dir_all(path.parent().unwrap())?;
        write_staged(
            &path,
            &serde_json::to_vec_pretty(&json!({
                "schema": 1, "lead": self.lead, "state": state, "process": process,
                "detail": detail, "log": path.with_extension("log"),
            }))?,
        )
    }

    pub(crate) fn stop(&mut self) -> io::Result<()> {
        if let Some(job) = self.job.take() {
            job.terminate(1, Duration::from_secs(5))?;
            self.child = None;
            self.record(
                "stopped",
                None,
                "the owning lead host ended; retained results and pending work are preserved",
            )?;
        }
        Ok(())
    }
}

pub(super) const POLL_INTERVAL: Duration = JOURNAL_POLL;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_initialization_is_found_from_a_nested_project() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("workspace/project/subdirectory");
        fs::create_dir_all(&project).unwrap();

        assert!(!board_initialized(&project));

        fs::create_dir_all(root.path().join("workspace/.beads")).unwrap();
        fs::write(root.path().join("workspace/.beads/config.yaml"), "").unwrap();
        assert!(board_initialized(&project));
    }
}
