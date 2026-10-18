//! Model-free structured-assignment validation and rendering through the
//! installed entry point: `executor assignment` names one pool slot, checks
//! the assignment against that checkout and prints the exact brief a session
//! would receive. Nothing here claims a slot, writes a file or starts a
//! launcher, so no model, subscription or terminal is involved.
#![cfg(windows)]

use harness_core::{
    delivery_reflection::{
        ACCEPTANCE_METADATA, AcceptanceBinding, ActivationRequest, JournalPolicy, Purpose,
        ReflectionController, RefusalCode, SourceIdentity,
    },
    process_service::{self, ServiceProcess},
    task_worktree::{
        self, AssignmentSubmission, CompletionFacts, RetainedCompletion, SlotAssignmentBinding,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;
#[path = "fixtures/synthetic_role_build.rs"]
mod synthetic_role_build;

use control_endpoint::{Answer, Bearer, Server};

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

/// Owned source checkout with one registered pool slot: the same layout a
/// dispatch allocates, without any upstream or launcher.
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    slot: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("executor-assignment-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        // Build agents can provide a short 8.3 temp prefix (RUNNER~1) while
        // the rendered checkout path is canonicalized; anchor every path on
        // the long form so the rendered brief matches either environment.
        let canonical = root.canonicalize().unwrap();
        // Strip the verbatim prefix: the manager renders ordinary long paths.
        let root = canonical
            .to_string_lossy()
            .strip_prefix(r"\\?\")
            .map(PathBuf::from)
            .unwrap_or(canonical);
        let source = root.join("proj");
        git(
            &root,
            &[
                "init",
                "-q",
                "--initial-branch=main",
                source.to_str().unwrap(),
            ],
        );
        configure(&source);
        fs::write(source.join("README.md"), "seed\n").unwrap();
        fs::create_dir_all(source.join("global")).unwrap();
        fs::write(
            source.join("global/orchestration.toml"),
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n",
        )
        .unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-qm", "seed"]);
        let slot = root.join("proj-wt1");
        git(
            &source,
            &[
                "worktree",
                "add",
                "--detach",
                slot.to_str().unwrap(),
                "HEAD",
            ],
        );
        Self { root, source, slot }
    }

    fn head(&self) -> String {
        git_output(&self.source, &["rev-parse", "HEAD"])
    }

    fn assignment(&self, name: &str, inputs: &[&str], outputs: &[&str]) -> PathBuf {
        self.assignment_with(name, &[], inputs, outputs)
    }

    /// The same schema-1 document with additional top-level fields, so a check
    /// can declare the optional contract fields or a deliberately malformed
    /// one without hand-writing JSON in every case.
    fn assignment_with(
        &self,
        name: &str,
        extra: &[(&str, serde_json::Value)],
        inputs: &[&str],
        outputs: &[&str],
    ) -> PathBuf {
        let path = self.root.join(name);
        let mut document = json!({
            "schema": 1,
            "objective": "Extend the synthetic module",
            "inputs": inputs,
            "outputs": outputs,
            "invariants": ["keep the change inside the checkout"],
            "acceptance": ["the synthetic check passes"],
        });
        let object = document.as_object_mut().unwrap();
        for (key, value) in extra {
            object.insert((*key).to_owned(), value.clone());
        }
        fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        path
    }

    fn check(&self, assignment: &Path, extra: &[&str]) -> std::process::Output {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "assignment",
            "--source",
            self.source.to_str().unwrap(),
            "--slot",
            "1",
            "--assignment",
            assignment.to_str().unwrap(),
        ]);
        command.args(extra);
        command.output().unwrap()
    }

    fn drop(self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The same owned checkout plus a controlled CODEX_HOME for the real
/// `executor spawn` entry point. The launcher is deliberately absent: a
/// dispatch must reach the durable pool binding and then stop at the missing
/// launcher, so no model, subscription or terminal is involved.
struct SpawnFixture {
    base: Fixture,
    home: PathBuf,
}

impl SpawnFixture {
    fn new(name: &str) -> Self {
        let base = Fixture::new(name);
        let home = base.root.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("ds.config.toml"), "model = 'deepseek-flash'\n").unwrap();
        Self { base, home }
    }

    /// Installs the real bd executable into the controlled home and initializes
    /// one synthetic board in the consuming source checkout.
    fn with_board(name: &str) -> Self {
        let fixture = Self::new(name);
        let bin = fixture.home.join("harness/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::copy(bd_executable(), bin.join("bd.exe")).unwrap();
        let out = fixture.bd_run(&[
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "assign",
        ]);
        assert!(out.status.success(), "{}", bd_failed("init", &out));
        fixture
    }

    fn bd(&self) -> PathBuf {
        self.home.join("harness/bin/bd.exe")
    }

    fn bd_run(&self, args: &[&str]) -> std::process::Output {
        Command::new(self.bd())
            .args(args)
            .current_dir(&self.base.source)
            .env("BD_NON_INTERACTIVE", "1")
            .env("BEADS_ACTOR", "executor-assignment-test")
            .output()
            .unwrap()
    }

    fn bd_json(&self, args: &[&str]) -> Value {
        let out = self.bd_run(args);
        assert!(
            out.status.success(),
            "{}",
            bd_failed(args.first().copied().unwrap_or("bd"), &out)
        );
        serde_json::from_slice(&out.stdout).unwrap_or_else(|error| {
            panic!(
                "bd {} did not return JSON: {error}\n{}",
                args.join(" "),
                String::from_utf8_lossy(&out.stdout)
            )
        })
    }

    fn create_issue(&self, title: &str, issue_type: &str, parent: Option<&str>) -> String {
        let mut args = vec!["create", title, "--type", issue_type];
        if let Some(parent) = parent {
            args.push("--parent");
            args.push(parent);
        }
        args.push("--json");
        self.bd_json(&args)["id"].as_str().unwrap().to_owned()
    }

    fn set_metadata(&self, issue: &str, key: &str, value: &str) {
        let pair = format!("{key}={value}");
        self.bd_json(&["update", issue, "--set-metadata", &pair, "--json"]);
    }

    fn rename_issue(&self, issue: &str, title: &str) {
        self.bd_json(&["update", issue, "--title", title, "--json"]);
    }

    fn unset_metadata(&self, issue: &str, key: &str) {
        self.bd_json(&["update", issue, "--unset-metadata", key, "--json"]);
    }

    fn show(&self, issue: &str) -> Value {
        let shown = self.bd_json(&["show", issue, "--json"]);
        shown
            .as_array()
            .and_then(|rows| rows.first())
            .cloned()
            .unwrap_or(shown)
    }

    fn assignment(&self, name: &str, board_task: &str, purpose: Option<&str>) -> PathBuf {
        let mut contract = json!({
            "schema": 1,
            "objective": "run the synthetic board-bound assignment",
            "board_task": board_task,
            "inputs": ["README.md"],
            "outputs": ["out/bound.txt"],
            "invariants": ["keep the change inside the checkout"],
            "acceptance": ["the synthetic check passes"],
        });
        if let Some(purpose) = purpose {
            contract["purpose"] = json!(purpose);
        }
        let path = self.base.root.join(name);
        fs::write(&path, serde_json::to_vec_pretty(&contract).unwrap()).unwrap();
        path
    }

    fn legacy_assignment(&self, name: &str) -> PathBuf {
        let path = self.base.root.join(name);
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "objective": "legacy structured assignment without a board binding",
                "inputs": ["README.md"],
                "outputs": ["out/legacy.txt"],
                "invariants": ["keep the change inside the checkout"],
                "acceptance": ["the synthetic check passes"],
            }))
            .unwrap(),
        )
        .unwrap();
        path
    }

    /// The real dispatch entry point, pointed at the controlled home and the
    /// exact local base. The missing launcher is the intended stop after the
    /// pool/slot owner wrote its durable binding.
    fn spawn(&self, prompt: &[&str], extra: &[&str]) -> std::process::Output {
        self.spawn_command(prompt, extra).output().unwrap()
    }

    fn spawn_command(&self, prompt: &[&str], extra: &[&str]) -> Command {
        let base = self.base.head();
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "spawn",
            "--source",
            self.base.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
            "--profile",
            "ds",
            "--base",
            &base,
        ]);
        command.args(prompt);
        command.args(extra);
        command.env("CODEX_HOME", &self.home);
        command.env("CODEX_THREAD_ID", "lead-thread-synthetic");
        for key in [
            "HARNESS_EXECUTOR_SESSION",
            "CODEX_SESSION_ID",
            "HARNESS_EXECUTOR_RUN",
            "HARNESS_ORIGINATING_LEAD",
            "HARNESS_LEAD_THREAD",
            "HARNESS_LEAD_RECIPIENT",
        ] {
            command.env_remove(key);
        }
        command
    }

    /// One explicit continuation of the recorded slot session through the real
    /// `executor resume` owner. Free text is recorded as itself, not as the
    /// slot's bound structured assignment.
    fn resume_command(&self, owner: &str, session: &str, prompt: &str) -> Command {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "resume",
            "--source",
            self.base.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
            "--slot",
            "1",
            "--owner",
            owner,
            "--session",
            session,
            "--exec",
            prompt,
        ]);
        command.env("CODEX_HOME", &self.home);
        command.env("CODEX_THREAD_ID", "lead-thread-synthetic");
        for key in [
            "HARNESS_EXECUTOR_SESSION",
            "CODEX_SESSION_ID",
            "HARNESS_EXECUTOR_RUN",
            "HARNESS_ORIGINATING_LEAD",
            "HARNESS_LEAD_THREAD",
            "HARNESS_LEAD_RECIPIENT",
        ] {
            command.env_remove(key);
        }
        command
    }

    fn release(&self, extra: &[&str]) -> std::process::Output {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "release",
            "--source",
            self.base.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
        ]);
        command.args(extra);
        for key in [
            "HARNESS_EXECUTOR_SESSION",
            "CODEX_SESSION_ID",
            "HARNESS_EXECUTOR_RUN",
            "HARNESS_ORIGINATING_LEAD",
            "HARNESS_LEAD_THREAD",
            "HARNESS_LEAD_RECIPIENT",
        ] {
            command.env_remove(key);
        }
        command.output().unwrap()
    }

    fn state_dir(&self) -> PathBuf {
        self.home.join("harness/executor-pool")
    }

    fn record(&self, index: u32) -> Value {
        serde_json::from_slice(&fs::read(self.record_path(index)).unwrap()).unwrap()
    }

    fn pool_dir(&self) -> PathBuf {
        fs::read_dir(self.state_dir())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.is_dir())
            .expect("one pool state directory per source checkout")
    }

    fn record_path(&self, index: u32) -> PathBuf {
        self.pool_dir().join(format!("slot-{index}.json"))
    }

    fn receipt(&self, index: u32) -> Value {
        serde_json::from_slice(
            &fs::read(self.pool_dir().join(format!("spawn-{index}.json"))).unwrap(),
        )
        .unwrap()
    }

    fn drop_fixture(self) {
        self.base.drop();
    }
}

const ACCEPTANCE_LEAD: &str = "lead-acceptance";
const ACCEPTANCE_LEAD_TURN: &str = "lead-turn-1";
const ACCEPTANCE_EXECUTOR_SESSION: &str = "executor-session-1";
const ACCEPTANCE_EXECUTOR_TURN: &str = "executor-turn-1";
const ACCEPTANCE_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// One owned board and executor checkout plus a real loopback control
/// endpoint. The lead registry record names this test process so `registered_lead`
/// exercises its live-process check, while every board effect stays in the
/// fixture root.
struct AcceptanceFixture {
    spawn: SpawnFixture,
    server: Server,
    lead_thread: String,
    lead_turn: String,
}

impl AcceptanceFixture {
    fn new(name: &str) -> Self {
        let spawn = SpawnFixture::with_board(name);
        let server = Server::start(Bearer::Value(ACCEPTANCE_TOKEN.to_owned()));
        server.answer("initialize", Answer::Result(json!({})));
        let fixture = Self {
            spawn,
            server,
            lead_thread: ACCEPTANCE_LEAD.to_owned(),
            lead_turn: ACCEPTANCE_LEAD_TURN.to_owned(),
        };
        fixture.set_lead_turns(vec![
            json!({"id": "lead-turn-0", "status": "completed"}),
            json!({"id": fixture.lead_turn, "status": "inProgress"}),
        ]);
        fixture.register_endpoint(fixture.server.port, &fixture.lead_thread);
        fixture
    }

    fn project(&self) -> &Path {
        &self.spawn.base.source
    }

    fn checkout(&self) -> &Path {
        &self.spawn.base.slot
    }

    fn home(&self) -> &Path {
        &self.spawn.home
    }

    fn set_lead_turns(&self, turns: Vec<Value>) {
        self.server.answer(
            "thread/read",
            Answer::Result(json!({
                "thread": {"id": self.lead_thread, "turns": turns},
            })),
        );
    }

    fn register_endpoint(&self, port: u16, thread: &str) {
        let user = process_service::current_user().unwrap();
        let program = std::env::current_exe().unwrap();
        let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)
            .unwrap()
            .identity();
        let registry = self.home().join("harness/lead-endpoints");
        fs::create_dir_all(&registry).unwrap();
        fs::write(
            registry.join(format!("{thread}.json")),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "port": port,
                "token": ACCEPTANCE_TOKEN,
                "threadId": thread,
                "process": {
                    "pid": identity.pid,
                    "creationTime": identity.creation_time,
                    "program": program,
                },
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn create_task(&self, title: &str, purpose: Option<Purpose>) -> String {
        let task = self.spawn.create_issue(title, "task", None);
        if let Some(purpose) = purpose {
            self.spawn
                .set_metadata(&task, "harness.purpose", purpose.as_str());
        }
        task
    }

    fn controller(&self) -> ReflectionController {
        ReflectionController::new(self.spawn.bd(), self.project(), self.home())
    }

    fn activate(&self) -> ReflectionController {
        let controller = self.controller();
        controller
            .activate(&ActivationRequest {
                lead_session: self.lead_thread.clone(),
                run: Some("acceptance-run".to_owned()),
                boundary: None,
                journal: JournalPolicy::EnableIfMissing,
                limits: None,
            })
            .unwrap();
        controller
    }

    fn board_source(&self) -> SourceIdentity {
        harness_core::delivery_reflection::board_source_identity(&self.spawn.bd(), self.project())
            .unwrap()
    }

    fn retain(
        &self,
        task: &str,
        purpose: Purpose,
        run: &str,
        state: &str,
        source: Option<SourceIdentity>,
    ) -> RetainedCompletion {
        let assignment_sha256 = harness_core::build_identity::hash_bytes(run.as_bytes());
        let assignment = SlotAssignmentBinding {
            source: source.unwrap_or_else(|| self.board_source()),
            issue: task.to_owned(),
            assignment_sha256: assignment_sha256.clone(),
            purpose: Some(purpose.as_str().to_owned()),
            purpose_source: "board".to_owned(),
        };
        self.retain_bound(
            run,
            state,
            "executor-slot-binding",
            &format!("handle-{run}"),
            "executor-acceptance",
            assignment,
        )
    }

    /// Retain one completion with the exact task binding and generation
    /// handle a live slot records. Same-task handoffs reuse both identities, so
    /// a completion alone cannot show that the current startup is finished.
    fn retain_bound(
        &self,
        run: &str,
        state: &str,
        slot_task: &str,
        slot_handle: &str,
        owner: &str,
        assignment: SlotAssignmentBinding,
    ) -> RetainedCompletion {
        thread::sleep(Duration::from_millis(10));
        let result = self.spawn.base.root.join(format!("result-{run}.txt"));
        fs::write(&result, format!("synthetic result for {run}\n")).unwrap();
        let state_dir = task_worktree::pool_state_dir(self.home(), self.checkout()).unwrap();
        fs::create_dir_all(&state_dir).unwrap();
        let assignment_sha256 = assignment.assignment_sha256.clone();
        task_worktree::retain_completion(
            &state_dir,
            &CompletionFacts {
                run_generation: run.to_owned(),
                slot: 1,
                owner: owner.to_owned(),
                checkout: self.checkout().to_path_buf(),
                task: Some(slot_task.to_owned()),
                handle: Some(slot_handle.to_owned()),
                assignment: Some(assignment),
                submitted_assignment: Some(AssignmentSubmission::structured(assignment_sha256)),
                role: Some("executor".to_owned()),
                state: state.to_owned(),
                exit_code: Some(if state == "completed" { 0 } else { 1 }),
                session: Some(ACCEPTANCE_EXECUTOR_SESSION.to_owned()),
                turn: Some(ACCEPTANCE_EXECUTOR_TURN.to_owned()),
                result: Some(result),
            },
        )
        .unwrap()
    }

    fn set_recorded_ms(&self, run: &str, recorded_ms: u64) {
        let state_dir = task_worktree::pool_state_dir(self.home(), self.checkout()).unwrap();
        let manifest = state_dir
            .join("completions")
            .join(run)
            .join("completion.json");
        let mut record: RetainedCompletion =
            serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
        record.recorded_ms = recorded_ms;
        fs::write(&manifest, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    }

    fn accept_command(&self, thread: &str, task: &str, run: Option<&str>) -> Command {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "accept",
            task,
            "--project",
            self.project().to_str().unwrap(),
            "--source",
            self.checkout().to_str().unwrap(),
            "--codex-home",
            self.home().to_str().unwrap(),
        ]);
        if let Some(run) = run {
            command.args(["--run", run]);
        }
        command
            .env("CODEX_HOME", self.home())
            .env("CODEX_THREAD_ID", thread);
        for key in [
            "CODEX_SESSION_ID",
            "HARNESS_EXECUTOR_SESSION",
            "HARNESS_BOUNDED_LEAD_SESSION",
            "HARNESS_EXECUTOR_RUN",
            "HARNESS_ORIGINATING_LEAD",
            "HARNESS_LEAD_THREAD",
            "HARNESS_LEAD_RECIPIENT",
        ] {
            command.env_remove(key);
        }
        command
    }

    fn accept_raw(&self, thread: &str, task: &str, run: Option<&str>) -> std::process::Output {
        self.accept_command(thread, task, run).output().unwrap()
    }

    fn accept_spawn(&self, task: &str, run: Option<&str>) -> std::process::Child {
        self.accept_command(&self.lead_thread, task, run)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn accept(&self, task: &str, run: Option<&str>) -> Value {
        let out = self.accept_raw(&self.lead_thread, task, run);
        let text = output_text(&out);
        assert_eq!(out.status.code(), Some(0), "{text}");
        serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|error| panic!("executor accept did not return JSON: {error}\n{text}"))
    }

    fn refuse(&self, task: &str, run: Option<&str>) -> String {
        let out = self.accept_raw(&self.lead_thread, task, run);
        let text = output_text(&out);
        assert_eq!(out.status.code(), Some(2), "{text}");
        text
    }

    fn show(&self, task: &str) -> Value {
        self.spawn.show(task)
    }

    fn acceptance(&self, task: &str) -> Option<String> {
        self.show(task)
            .get("metadata")
            .and_then(|metadata| metadata.get(ACCEPTANCE_METADATA))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    fn close_with_reason(&self, task: &str, reason: &str) {
        self.spawn
            .bd_json(&["close", task, "--reason", reason, "--json"]);
    }

    /// Dispatch one structured correction through the real spawn owner. The
    /// missing installed launcher stops it after the durable slot binding and
    /// receipt, which are the acceptance inputs this fixture needs.
    fn bind_correction(
        &self,
        name: &str,
        task: &str,
        owner: &str,
    ) -> (String, String, SlotAssignmentBinding, String) {
        self.bind_board(name, task, Some("ordinary"), owner)
    }

    /// Dispatch one structured assignment through the real spawn owner and
    /// return the durable slot identity it recorded. `purpose` is only the
    /// assignment's declared override; `None` exercises lineage inheritance.
    fn bind_board(
        &self,
        name: &str,
        task: &str,
        purpose: Option<&str>,
        owner: &str,
    ) -> (String, String, SlotAssignmentBinding, String) {
        let assignment = self.spawn.assignment(name, task, purpose);
        let dispatched = self.spawn.spawn(
            &["--assignment", assignment.to_str().unwrap()],
            &["--owner", owner],
        );
        let text = output_text(&dispatched);
        assert!(!dispatched.status.success(), "{text}");
        assert!(
            text.contains("installed Codex launcher is missing"),
            "the correction must stop only after its durable slot binding: {text}"
        );
        let (index, record) = self.bound_slot(task);
        let slot_task = record["task"]
            .as_str()
            .expect("durable slot task")
            .to_owned();
        let handle = record["handle"]
            .as_str()
            .expect("durable slot handle")
            .to_owned();
        let assignment = serde_json::from_value(record["assignment"].clone())
            .expect("verified board assignment");
        let run = self.spawn.receipt(index)["originatingLead"]["runGeneration"]
            .as_str()
            .expect("dispatch generation")
            .to_owned();
        (slot_task, handle, assignment, run)
    }

    /// Locate the one slot whose durable record carries this task's board
    /// assignment. A fixture may bind another task after releasing the first
    /// slot, so the slot index is never assumed.
    fn bound_slot(&self, task: &str) -> (u32, Value) {
        for entry in fs::read_dir(self.spawn.pool_dir()).unwrap() {
            let path = entry.unwrap().path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(number) = name
                .strip_prefix("slot-")
                .and_then(|name| name.strip_suffix(".json"))
            else {
                continue;
            };
            let Ok(index) = number.parse::<u32>() else {
                continue;
            };
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let Ok(record) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            if record["assignment"]["issue"].as_str() == Some(task) {
                return (index, record);
            }
        }
        panic!("no durable slot binding records board task {task}");
    }

    fn drop_fixture(self) {
        let Self { spawn, server, .. } = self;
        drop(server);
        spawn.drop_fixture();
    }
}

fn bd_name() -> &'static str {
    "bd.exe"
}

fn bd_executable() -> PathBuf {
    if let Some(value) = std::env::var_os("HARNESS_BD_EXE") {
        let candidate = PathBuf::from(value);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        let candidate = PathBuf::from(home).join("harness/bin").join(bd_name());
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let lower = dir.to_string_lossy().to_ascii_lowercase();
            if lower.ends_with(r"\windowsapps") || lower.contains(r"\windowsapps\\") {
                continue;
            }
            let candidate = dir.join(bd_name());
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
}

fn bd_failed(op: &str, out: &std::process::Output) -> String {
    format!(
        "bd {op} failed: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn output_text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn configure(cwd: &Path) {
    git(cwd, &["config", "user.email", "executor@example.test"]);
    git(cwd, &["config", "user.name", "Executor"]);
}

/// Install the native CLI test double where the launcher owner resolves it.
fn install_launcher(home: &Path, exe: &Path) {
    let launcher = home.join("harness/bin/codex.exe");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    if fs::hard_link(exe, &launcher).is_err() {
        fs::copy(exe, &launcher).unwrap_or_else(|error| {
            panic!(
                "the fixture home could not install the native CLI {}: {error}",
                exe.display()
            )
        });
    }
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_output(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// An explicit `--base REV` must name the commit the inspected slot is really
/// at: this check never synchronizes, so a short revision is resolved in the
/// slot and anything the slot would have to move to is refused without writes.
#[test]
fn explicit_base_resolves_in_the_slot_and_rejects_other_revisions() {
    let fixture = Fixture::new("base");
    fs::write(fixture.slot.join("input.txt"), "declared input\n").unwrap();
    let assignment = fixture.assignment("assignment.json", &["input.txt"], &["out.txt"]);
    let head = fixture.head();
    let short = &head[..8];
    let before = git_output(
        &fixture.slot,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    );

    let out = fixture.check(&assignment, &["--base", short]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains(&format!("base={head}")), "{text}");
    assert!(text.contains(&format!("base: {head}")), "{text}");
    // The short revision appears only as the prefix of the emitted full one.
    assert_eq!(text.matches(&format!("base: {short}")).count(), 1, "{text}");
    assert_eq!(
        git_output(
            &fixture.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        ),
        before
    );

    // A revision that exists in the repository but is not this slot's HEAD is
    // refused: the route does not synchronize the slot.
    fs::write(fixture.source.join("moved.txt"), "moved on\n").unwrap();
    git(&fixture.source, &["add", "moved.txt"]);
    git(
        &fixture.source,
        &["commit", "-qm", "move the source checkout on"],
    );
    let moved = git_output(&fixture.source, &["rev-parse", "HEAD"]);
    assert_ne!(moved, head);
    let out = fixture.check(&assignment, &["--base", &moved]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("never synchronizes"), "{text}");
    assert!(text.contains(&moved), "{text}");
    assert!(text.contains(&head), "{text}");

    // An unknown revision is refused the same way.
    let unknown = "0".repeat(40);
    let out = fixture.check(&assignment, &["--base", &unknown]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("is not a commit in"), "{text}");

    // Neither refusal wrote anything.
    assert_eq!(
        git_output(
            &fixture.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        ),
        before
    );
    assert!(!fixture.slot.join("out.txt").exists());
    assert!(!fixture.root.join("home").exists());
    fixture.drop();
}

/// A missing slot cannot be inspected: the remedy must name the dispatch that
/// creates and synchronizes it, because pruning a stale registration alone
/// cannot bring the slot back.
#[test]
fn missing_slot_names_the_dispatch_that_creates_it() {
    let fixture = Fixture::new("missing-slot");
    let assignment = fixture.assignment("assignment.json", &[], &["out.txt"]);
    fs::remove_dir_all(&fixture.slot).unwrap();
    let out = fixture.check(&assignment, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("is missing"), "{text}");
    assert!(text.contains("executor spawn --repository"), "{text}");
    assert!(text.contains("create and synchronize"), "{text}");
    assert!(!fixture.slot.exists());
    assert!(!fixture.root.join("home").exists());
    fixture.drop();
}

#[test]
fn dry_run_renders_the_bound_checkout_base_and_exact_paths() {
    let fixture = Fixture::new("render");
    fs::write(fixture.slot.join("input.txt"), "declared input\n").unwrap();
    fs::write(fixture.slot.join("existing_child.rs"), "// child\n").unwrap();
    let assignment = fixture.assignment(
        "assignment.json",
        &["input.txt", "existing_child.rs"],
        &["crates/new/module.rs"],
    );
    let before = git_output(
        &fixture.slot,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    );
    let out = fixture.check(&assignment, &["--owner", "exec-ds-9"]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("executor assignment valid: slot=1"), "{text}");
    assert!(
        text.contains(&format!("checkout={}", fixture.slot.display())),
        "{text}"
    );
    assert!(text.contains(&format!("base={}", fixture.head())), "{text}");
    assert!(text.contains("inputs=2 outputs=1"), "{text}");
    assert!(
        text.contains("(nothing claimed, written or launched)"),
        "{text}"
    );
    assert!(text.contains("Structured executor assignment (schema 1)"));
    assert!(
        text.contains(&format!("checkout: {}", fixture.slot.display())),
        "{text}"
    );
    assert!(text.contains("owner: exec-ds-9"), "{text}");
    assert!(text.contains("- input.txt"), "{text}");
    assert!(text.contains("- existing_child.rs"), "{text}");
    assert!(text.contains("- crates/new/module.rs"), "{text}");
    assert!(text.contains("- the synthetic check passes"), "{text}");
    assert!(
        text.contains("feedback: return the result and any decision you escalate to the lead that dispatched this assignment through codex-harness lead message (short --text fallback)."),
        "{text}"
    );
    // Standing responsibility guidance (work cycle, base verification, the
    // heavy-command lane, the compact result shape) reaches a managed session
    // through its role context and is not restated in the brief.
    for standing in [
        "work cycle (yours)",
        "verify the checkout is at the base above",
        "Choose the installed skills this assignment needs",
        "report a compact result: done and remaining work",
    ] {
        assert!(!text.contains(standing), "{standing}\n{text}");
    }
    // The check is read-only: the slot keeps exactly the state it had and no
    // state root is written.
    assert_eq!(
        git_output(
            &fixture.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        ),
        before,
        "the dry run must not change the slot"
    );
    assert!(
        !fixture.slot.join("crates/new/module.rs").exists(),
        "the dry run must not create the declared output"
    );
    assert!(!fixture.root.join("home").exists());
    fixture.drop();
}

#[test]
fn missing_inputs_traversal_and_absolute_paths_are_rejected() {
    let fixture = Fixture::new("reject");
    let missing = fixture.assignment("missing.json", &["crates/absent.rs"], &[]);
    let out = fixture.check(&missing, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("crates/absent.rs"), "{text}");
    assert!(text.contains("is missing from the checkout"), "{text}");

    let traversal = fixture.assignment("traversal.json", &["../outside.txt"], &[]);
    let out = fixture.check(&traversal, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("must not traverse with '..'"), "{text}");

    let absolute = fixture.assignment("absolute.json", &[r"C:\outside.txt"], &[]);
    let out = fixture.check(&absolute, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("must be a plain relative path"), "{text}");
    fixture.drop();
}

/// The result consumer, the concrete feedback address and the declared
/// additional boundaries reach the exact brief the installed render path
/// prints: an existing schema-1 document keeps working with the dispatching
/// lead as the consumer, and a declared consumer or trigger is carried beside
/// the feedback address while the standing boundaries stay owned by the
/// executor role fragment.
#[test]
fn consumer_and_escalation_reach_the_rendered_brief() {
    let fixture = Fixture::new("contract");
    fs::write(fixture.slot.join("input.txt"), "declared input\n").unwrap();

    // No declared contract fields: the schema-1 document still validates.
    let minimal = fixture.assignment("assignment.json", &["input.txt"], &["out.txt"]);
    let out = fixture.check(&minimal, &[]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("consumer: the lead that dispatched this assignment"),
        "{text}"
    );
    assert!(
        !text.contains("purpose:"),
        "an omitted purpose stays unspecified: {text}"
    );
    assert!(
        text.contains(
            "escalate to the lead that dispatched this assignment (additional boundaries)"
        ),
        "{text}"
    );
    assert!(
        text.contains("feedback: return the result and any decision you escalate to the lead that dispatched this assignment through codex-harness lead message (short --text fallback)."),
        "{text}"
    );
    // Standing boundaries, waiting behavior and the compact result shape stay
    // owned by the executor role fragment: none of them is copied into the
    // brief.
    for standing in [
        "- a change to the agreed outcome or scope",
        "- a concrete dependency you cannot obtain",
        "work cycle (yours)",
        "no polling, keep-alive loop or resume",
        "report a compact result: done and remaining work",
    ] {
        assert!(!text.contains(standing), "{standing}\n{text}");
    }

    // Declared consumer and additional triggers travel with the standing ones.
    let declared = fixture.assignment_with(
        "declared.json",
        &[
            ("consumer", json!("the lead of epic sample-3mu")),
            ("purpose", json!("improvement")),
            (
                "escalate",
                json!(["any change to the synthetic fixture layout"]),
            ),
        ],
        &["input.txt"],
        &["out.txt"],
    );
    let out = fixture.check(&declared, &[]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("consumer: the lead of epic sample-3mu"),
        "{text}"
    );
    assert!(text.contains("purpose: improvement"), "{text}");
    assert!(
        text.contains("escalate to the lead of epic sample-3mu (additional boundaries)"),
        "{text}"
    );
    assert!(
        text.contains("- any change to the synthetic fixture layout"),
        "{text}"
    );
    assert!(
        text.contains("feedback: return the result and any decision you escalate to the lead of epic sample-3mu through codex-harness lead message (short --text fallback)."),
        "{text}"
    );
    assert!(
        !text.contains("- a material architecture or design change"),
        "standing boundaries stay with the role fragment: {text}"
    );
    fixture.drop();
}

/// Malformed contract fields fail before any rendering, with the field named,
/// and an unknown field beside them is still refused: the optional fields do
/// not loosen schema-1 validation.
#[test]
fn malformed_contract_fields_are_refused_without_writing() {
    let fixture = Fixture::new("contract-reject");
    fs::write(fixture.slot.join("input.txt"), "declared input\n").unwrap();
    let before = git_output(
        &fixture.slot,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    );

    for (name, field, value, expected) in [
        (
            "empty-consumer.json",
            "consumer",
            json!(""),
            "assignment consumer is empty",
        ),
        (
            "long-consumer.json",
            "consumer",
            json!("x".repeat(600)),
            "assignment consumer is",
        ),
        (
            "empty-trigger.json",
            "escalate",
            json!([""]),
            "assignment escalate[0] is empty",
        ),
        (
            "unknown-purpose.json",
            "purpose",
            json!("auto"),
            "purpose 'auto' is not ordinary, reflection or improvement",
        ),
        (
            "empty-purpose.json",
            "purpose",
            json!(""),
            "assignment purpose is empty",
        ),
        (
            "renamed-field.json",
            "escalations",
            json!(["any change to the layout"]),
            "unknown field",
        ),
    ] {
        let assignment =
            fixture.assignment_with(name, &[(field, value)], &["input.txt"], &["out.txt"]);
        let out = fixture.check(&assignment, &[]);
        let text = output_text(&out);
        assert!(!out.status.success(), "{name}: {text}");
        assert!(text.contains(expected), "{name}: {text}");
    }

    // Every refusal happened before anything was rendered, claimed or written.
    assert_eq!(
        git_output(
            &fixture.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        ),
        before
    );
    assert!(!fixture.slot.join("out.txt").exists());
    fixture.drop();
}
/// A directory junction is the unprivileged Windows reparse point a declared
/// path could use to leave the checkout; both an input reaching through it and
/// an output created through it must be rejected before anything is read.
#[test]
fn escaping_reparse_points_are_rejected_for_inputs_and_outputs() {
    let fixture = Fixture::new("escape");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "outside the checkout\n").unwrap();
    let junction = fixture.slot.join("escape");
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    let input = fixture.assignment("link-input.json", &["escape/secret.txt"], &[]);
    let out = fixture.check(&input, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("resolves outside the checkout"), "{text}");

    let output = fixture.assignment("link-output.json", &[], &["escape/new.txt"]);
    let out = fixture.check(&output, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("resolves outside the checkout"), "{text}");
    assert!(
        !outside.join("new.txt").exists(),
        "the escape must not create anything outside the checkout"
    );
    fixture.drop();
}

/// A normal structured assignment that names a consumer-board task reaches the
/// real dispatch owner and leaves a durable slot binding: canonical board
/// scope, issue, assignment content hash and the declared purpose. The
/// missing launcher is the intended stop after that write.
#[test]
fn board_bound_assignment_records_verified_task_purpose_and_identity() {
    let fixture = SpawnFixture::with_board("bound-identity");
    let task = fixture.create_issue("Bound ordinary task", "task", None);
    let assignment = fixture.assignment("bound.json", &task, Some("ordinary"));
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-bound"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "the dispatch must reach the launcher only after the slot binding: {text}"
    );
    let record = fixture.record(1);
    assert_eq!(record["state"], "occupied", "{record}");
    let binding = &record["assignment"];
    assert_eq!(binding["issue"], task, "{record}");
    assert_eq!(binding["purpose"], "ordinary", "{record}");
    assert_eq!(binding["purposeSource"], "declared", "{record}");
    let sha = binding["assignmentSha256"].as_str().expect("content hash");
    assert_eq!(sha.len(), 64, "{record}");
    assert!(
        sha.chars().all(|value| value.is_ascii_hexdigit()),
        "{record}"
    );
    let context = fixture.bd_json(&["context", "--json"]);
    assert_eq!(binding["source"]["project_id"], context["project_id"]);
    assert_eq!(binding["source"]["database"], context["database"]);
    assert_eq!(binding["source"]["beads_dir"], context["beads_dir"]);
    assert!(!binding["source"]["commit"].as_str().unwrap().is_empty());
    assert!(
        Path::new(binding["source"]["repo_root"].as_str().unwrap()).ends_with("proj"),
        "{record}"
    );
    let shown = fixture.show(&task);
    assert_eq!(shown["metadata"]["harness.purpose"], "ordinary", "{shown}");
    fixture.drop_fixture();
}

/// A known nonordinary purpose on the board lineage is retained on the child
/// binding and written to the child's existing `harness.purpose` metadata
/// without disturbing unrelated metadata.
#[test]
fn board_bound_assignment_inherits_known_nonordinary_purpose() {
    let fixture = SpawnFixture::with_board("bound-inherit");
    let epic = fixture.create_issue("Improvement epic", "epic", None);
    fixture.set_metadata(&epic, "harness.purpose", "improvement");
    let child = fixture.create_issue("Improvement child", "task", Some(&epic));
    fixture.set_metadata(&child, "synthetic.keep", "kept");
    let assignment = fixture.assignment("inherit.json", &child, None);
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-inherit"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let record = fixture.record(1);
    let binding = &record["assignment"];
    assert_eq!(binding["issue"], child, "{record}");
    assert_eq!(binding["purpose"], "improvement", "{record}");
    assert_eq!(
        binding["purposeSource"],
        format!("ancestor:{epic}"),
        "{record}"
    );
    let shown = fixture.show(&child);
    assert_eq!(
        shown["metadata"]["harness.purpose"], "improvement",
        "{shown}"
    );
    assert_eq!(shown["metadata"]["synthetic.keep"], "kept", "{shown}");
    fixture.drop_fixture();
}

/// An ordinary declaration under a nonordinary ancestor contradicts the
/// retained lineage and is refused before a slot claim or a launcher.
#[test]
fn conflicting_purpose_declaration_refuses_before_slot_or_launcher() {
    let fixture = SpawnFixture::with_board("bound-conflict");
    let epic = fixture.create_issue("Improvement epic", "epic", None);
    fixture.set_metadata(&epic, "harness.purpose", "improvement");
    let child = fixture.create_issue("Improvement child", "task", Some(&epic));
    let assignment = fixture.assignment("conflict.json", &child, Some("ordinary"));
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-conflict"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("does not silently leave its purpose lineage"),
        "{text}"
    );
    assert!(
        !text.contains("installed Codex launcher"),
        "the refusal must precede every launch stage: {text}"
    );
    assert!(
        !fixture.state_dir().exists(),
        "a refused declaration must not claim a slot"
    );
    let shown = fixture.show(&child);
    assert!(
        shown
            .get("metadata")
            .and_then(|metadata| metadata.get("harness.purpose"))
            .is_none(),
        "a refused declaration must not write board purpose metadata: {shown}"
    );
    fixture.drop_fixture();
}

/// A bound dispatch cannot manufacture a nonordinary purpose by declaration;
/// improvement and reflection purposes must come from the task or its lineage.
#[test]
fn unbacked_nonordinary_declaration_refuses_before_slot_or_launcher() {
    let fixture = SpawnFixture::with_board("bound-unbacked");
    let task = fixture.create_issue("Unbacked improvement", "task", None);
    let assignment = fixture.assignment("unbacked.json", &task, Some("improvement"));
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-unbacked"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("nonordinary purposes are inherited"),
        "{text}"
    );
    assert!(!text.contains("installed Codex launcher"), "{text}");
    assert!(
        !fixture.state_dir().exists(),
        "an unbacked declaration must not claim a slot"
    );
    let shown = fixture.show(&task);
    assert!(
        shown
            .get("metadata")
            .and_then(|metadata| metadata.get("harness.purpose"))
            .is_none(),
        "an unbacked declaration must not write board purpose metadata: {shown}"
    );
    fixture.drop_fixture();
}

/// A board task id that the consuming board cannot resolve refuses before a
/// slot claim or launcher, rather than becoming an unbound ordinary dispatch.
#[test]
fn unknown_board_task_refuses_before_slot_or_launcher() {
    let fixture = SpawnFixture::with_board("bound-missing");
    let missing = "assign-missing";
    let assignment = fixture.assignment("missing.json", missing, Some("ordinary"));
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-missing"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("show failed") && text.contains(missing),
        "{text}"
    );
    assert!(!text.contains("installed Codex launcher"), "{text}");
    assert!(
        !fixture.state_dir().exists(),
        "an unknown board task must not claim a slot"
    );
    fixture.drop_fixture();
}

/// A task with no declared purpose and no nonordinary ancestor is retained as
/// explicitly unknown, not inferred as ordinary, while the ordinary dispatch
/// itself still works.
#[test]
fn unknown_purpose_is_retained_as_unknown_not_ordinary() {
    let fixture = SpawnFixture::with_board("bound-unknown");
    let task = fixture.create_issue("Unknown purpose task", "task", None);
    let assignment = fixture.assignment("unknown.json", &task, None);
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-unknown"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let record = fixture.record(1);
    let binding = &record["assignment"];
    assert_eq!(binding["issue"], task, "{record}");
    assert!(binding["purpose"].is_null(), "{record}");
    assert_eq!(binding["purposeSource"], "unknown", "{record}");
    let shown = fixture.show(&task);
    assert!(
        shown
            .get("metadata")
            .and_then(|metadata| metadata.get("harness.purpose"))
            .is_none(),
        "{shown}"
    );
    fixture.drop_fixture();
}

/// A repeated owner continues its exact slot binding; a different board task
/// on that binding is refused and the original binding stays intact.
#[test]
fn conflicting_recorded_board_binding_refuses_a_retry() {
    let fixture = SpawnFixture::with_board("bound-retry");
    let first = fixture.create_issue("First bound task", "task", None);
    let second = fixture.create_issue("Second bound task", "task", None);
    let first_assignment = fixture.assignment("first.json", &first, None);
    let second_assignment = fixture.assignment("second.json", &second, None);
    let out = fixture.spawn(
        &["--assignment", first_assignment.to_str().unwrap()],
        &["--owner", "exec-ds-retry"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let before = fixture.record(1);
    assert_eq!(before["assignment"]["issue"], first, "{before}");

    let out = fixture.spawn(
        &["--assignment", second_assignment.to_str().unwrap()],
        &["--owner", "exec-ds-retry"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("is already bound to board task"),
        "the recorded binding conflict must be named: {text}"
    );
    assert!(!text.contains("installed Codex launcher"), "{text}");
    let after = fixture.record(1);
    assert_eq!(
        after["assignment"], before["assignment"],
        "the refused retry must not replace the binding"
    );
    assert_eq!(after["handle"], before["handle"], "{after}");
    fixture.drop_fixture();
}

/// A multi-generation improvement descendant keeps the nontriggering purpose
/// through title changes, a removed leaf purpose and a resumed slot. The
/// intermediate task records no purpose, so a binding that only reads the
/// immediate parent would regress to unknown or ordinary here.
#[test]
fn improvement_descendants_keep_purpose_across_subdivision_rename_retry_and_resume() {
    let fixture = SpawnFixture::with_board("lineage-improvement");
    let epic = fixture.create_issue("Improvement epic", "epic", None);
    fixture.set_metadata(&epic, "harness.purpose", "improvement");
    let child = fixture.create_issue("Implementation split", "task", Some(&epic));
    let leaf = fixture.create_issue("Implementation leaf", "task", Some(&child));
    fixture.rename_issue(&child, "Renamed implementation split");
    fixture.rename_issue(&leaf, "Renamed implementation leaf");

    let assignment = fixture.assignment("improvement-lineage.json", &leaf, None);
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-lineage"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let first = fixture.record(1);
    assert_eq!(first["assignment"]["issue"], leaf, "{first}");
    assert_eq!(first["assignment"]["purpose"], "improvement", "{first}");
    assert_eq!(
        first["assignment"]["purposeSource"],
        format!("ancestor:{epic}"),
        "{first}"
    );
    assert_eq!(
        fixture.show(&leaf)["metadata"]["harness.purpose"],
        "improvement",
        "the first dispatch records the inherited purpose on the leaf"
    );

    // A title is not lineage identity: renaming every task and retrying the
    // same owner continues the exact recorded binding.
    fixture.rename_issue(&epic, "Renamed improvement epic");
    fixture.rename_issue(&leaf, "Renamed implementation leaf again");
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-lineage"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let retried = fixture.record(1);
    assert_eq!(
        retried["assignment"], first["assignment"],
        "a title rename must not replace the purpose-bearing binding"
    );
    assert_eq!(retried["task"], first["task"], "{retried}");
    assert_eq!(retried["handle"], first["handle"], "{retried}");

    // Removing the leaf's recorded value must re-derive the improvement from
    // the multi-generation lineage, not reset it to unknown or ordinary.
    fixture.unset_metadata(&leaf, "harness.purpose");
    assert!(
        fixture.show(&leaf)["metadata"]
            .get("harness.purpose")
            .is_none(),
        "the synthetic removal must erase the leaf purpose"
    );
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-lineage"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let restored = fixture.record(1);
    assert_eq!(
        restored["assignment"], first["assignment"],
        "the retry must restore the same improvement binding"
    );
    assert_eq!(
        fixture.show(&leaf)["metadata"]["harness.purpose"],
        "improvement",
        "the inherited purpose is recorded again"
    );

    // A contradictory ordinary value on the leaf is refused before a new
    // slot claim or launcher, and the original binding stays intact.
    fixture.set_metadata(&leaf, "harness.purpose", "ordinary");
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-lineage"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("records purpose 'ordinary' but ancestor")
            && text.contains("declares nonordinary purpose 'improvement'"),
        "{text}"
    );
    assert!(
        !text.contains("installed Codex launcher"),
        "the conflicting purpose must refuse before launch: {text}"
    );
    assert_eq!(
        fixture.record(1)["assignment"],
        first["assignment"],
        "a refused contradiction must not replace the original binding"
    );

    // The recorded binding is also what a free-text resume continues. The
    // resume path does not re-resolve the board, so the retained purpose must
    // already be correct rather than being reset by the continuation.
    fixture.set_metadata(&leaf, "harness.purpose", "improvement");
    // The continuation resolves the configured upstream without fetching it;
    // a local origin keeps this check offline.
    git(
        &fixture.base.source,
        &[
            "remote",
            "add",
            "origin",
            fixture.base.source.to_str().unwrap(),
        ],
    );
    let resumed = fixture
        .resume_command(
            "exec-ds-lineage",
            "01a0c719-f4d4-7880-a9d2-1a96ee0f23f6",
            "continue the improvement leaf",
        )
        .output()
        .unwrap();
    let text = output_text(&resumed);
    assert!(!resumed.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let continued = fixture.record(1);
    assert_eq!(continued["task"], first["task"], "{continued}");
    assert_eq!(continued["handle"], first["handle"], "{continued}");
    assert_eq!(
        continued["assignment"], first["assignment"],
        "resume must continue the recorded improvement binding"
    );
    fixture.drop_fixture();
}

/// Reflection descendants inherit the same nontriggering exclusion; renaming
/// the task and retrying its owner continues the recorded binding.
#[test]
fn reflection_descendants_inherit_nontriggering_purpose_across_rename() {
    let fixture = SpawnFixture::with_board("lineage-reflection");
    let parent = fixture.create_issue("Reflection work", "task", None);
    fixture.set_metadata(&parent, "harness.purpose", "reflection");
    let child = fixture.create_issue("Reflection follow-up", "task", Some(&parent));
    fixture.rename_issue(&child, "Renamed reflection follow-up");
    let assignment = fixture.assignment("reflection-lineage.json", &child, None);
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-reflection-lineage"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let first = fixture.record(1);
    assert_eq!(first["assignment"]["issue"], child, "{first}");
    assert_eq!(first["assignment"]["purpose"], "reflection", "{first}");
    assert_eq!(
        first["assignment"]["purposeSource"],
        format!("ancestor:{parent}"),
        "{first}"
    );

    fixture.rename_issue(&child, "Reflection follow-up renamed again");
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-reflection-lineage"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    assert_eq!(
        fixture.record(1)["assignment"],
        first["assignment"],
        "a reflection descendant must not become ordinary after a rename"
    );
    assert_eq!(
        fixture.show(&child)["metadata"]["harness.purpose"],
        "reflection",
        "the child retains its inherited reflection purpose"
    );
    fixture.drop_fixture();
}

/// Free text never resolves a board: the dispatch reaches the launcher and
/// records no assignment binding.
#[test]
fn free_text_dispatch_keeps_an_unbound_slot_record() {
    let fixture = SpawnFixture::new("free-text");
    let out = fixture.spawn(&["--exec", "free text assignment"], &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let record = fixture.record(1);
    assert!(record["assignment"].is_null(), "{record}");
    assert!(
        !fixture.base.source.join(".beads").exists(),
        "free text must not initialize or contact a board"
    );
    fixture.drop_fixture();
}

/// A schema-1 assignment without `board_task` keeps the legacy unbound path.
#[test]
fn legacy_structured_dispatch_keeps_an_unbound_slot_record() {
    let fixture = SpawnFixture::new("legacy-structure");
    let assignment = fixture.legacy_assignment("legacy.json");
    let out = fixture.spawn(&["--assignment", assignment.to_str().unwrap()], &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let record = fixture.record(1);
    assert!(record["assignment"].is_null(), "{record}");
    fixture.drop_fixture();
}

/// One completed ordinary executor's exact native facts are retained by run
/// generation, so releasing and reusing the slot index cannot replace them.
/// A later free-text continuation of the same bound slot is retained as
/// explicitly ineligible while its ordinary output and receipt stay usable.
#[test]
fn completed_ordinary_facts_survive_slot_reuse_and_free_text_is_ineligible() {
    let fixture = SpawnFixture::with_board("completion-retained");
    // The continuation resolves the recorded upstream; no dispatch fetches or
    // rewrites it. A local path keeps this check offline.
    git(
        &fixture.base.source,
        &[
            "remote",
            "add",
            "origin",
            fixture.base.source.to_str().unwrap(),
        ],
    );
    install_launcher(
        &fixture.home,
        Path::new(env!("CARGO_BIN_EXE_harness-executor-fixture")),
    );
    fs::write(
        fixture.home.join("ds.config.toml"),
        "model = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    // The controlled child reports its started-thread identity from the
    // active config, so the explicit route must be present there too before
    // dispatch appends the workspace trust entry.
    fs::write(
        fixture.home.join("config.toml"),
        "model = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    let kit = synthetic_role_build::kit_source(&fixture.base.root);
    let build = synthetic_role_build::recorded_build(&fixture.base.root, &kit);
    synthetic_role_build::register(&fixture.home, &fixture.base.root, &build);

    let final_message = "fixture turn completed; the committed solution is in the bound slot";
    let first_task = fixture.create_issue("Completed ordinary task", "task", None);
    let first_assignment =
        fixture.assignment("completion-first.json", &first_task, Some("ordinary"));
    let first_session = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
    let mut command = fixture.spawn_command(
        &["--assignment", first_assignment.to_str().unwrap()],
        &["--owner", "exec-ds-completion-a"],
    );
    command.env("HARNESS_EXECUTOR_FIXTURE_MODE", "hang");
    command.env("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE", "control-app-server");
    command.env("HARNESS_IMPROVEMENT_FIXTURE_SESSION", first_session);
    command.env_remove("WT_SESSION");
    let first_run = command.output().unwrap();
    let text = output_text(&first_run);
    assert_eq!(first_run.status.code(), Some(0), "{text}");

    let state = harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.base.source)
        .expect("pool state directory");
    let receipt_path = state.join("spawn-1.json");
    let slot_message = state.join("message-1.txt");
    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    let first_generation = receipt["originatingLead"]["runGeneration"]
        .as_str()
        .expect("run generation")
        .to_owned();
    assert_eq!(receipt["control"]["role"]["role"], "executor", "{receipt}");
    assert_eq!(
        receipt["control"]["submittedAssignment"]["kind"], "structured",
        "{receipt}"
    );
    assert_eq!(receipt["observation"]["state"], "completed", "{receipt}");
    assert_eq!(receipt["observation"]["exitCode"], 0, "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], first_session,
        "{receipt}"
    );
    assert_eq!(
        receipt["observation"]["turn"], "fixture-turn-1",
        "the completed turn is observed, not inferred: {receipt}"
    );
    assert_eq!(
        receipt["slot"]["assignment"]["issue"], first_task,
        "{receipt}"
    );
    assert!(
        receipt["slot"]["handle"]
            .as_str()
            .is_some_and(|handle| !handle.is_empty()),
        "{receipt}"
    );
    assert_eq!(fs::read_to_string(&slot_message).unwrap(), final_message);
    let first_revision = git_output(&fixture.base.slot, &["rev-parse", "HEAD"]);
    assert!(
        git_output(
            &fixture.base.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty(),
        "the completed fixture run leaves the bound checkout clean"
    );

    let load = |generation: &str| {
        harness_core::task_worktree::load_retained_completion(
            &fixture.home,
            &fixture.base.source,
            generation,
        )
        .unwrap()
    };
    let first = load(&first_generation).expect("the first completion is retained");
    println!("retained-first={}", serde_json::to_string(&first).unwrap());
    assert!(first.eligibility.eligible, "{first:?}");
    assert!(first.eligibility.reasons.is_empty(), "{first:?}");
    assert_eq!(first.run_generation, first_generation);
    assert_eq!(first.slot, 1);
    assert_eq!(first.owner, "exec-ds-completion-a");
    assert_eq!(first.task.as_deref(), Some("exec-ds-completion-a"));
    assert_eq!(first.role.as_deref(), Some("executor"));
    assert_eq!(first.state, "completed");
    assert_eq!(first.exit_code, Some(0));
    assert_eq!(first.session.as_deref(), Some(first_session));
    assert_eq!(first.turn.as_deref(), Some("fixture-turn-1"));
    let binding = first.assignment.as_ref().expect("board assignment binding");
    assert_eq!(binding.issue, first_task);
    assert_eq!(binding.purpose.as_deref(), Some("ordinary"));
    assert_eq!(
        first.submitted_assignment,
        Some(
            harness_core::task_worktree::AssignmentSubmission::structured(
                binding.assignment_sha256.clone()
            )
        )
    );
    let result = first.result.as_ref().expect("retained result artifact");
    assert!(result.locator.is_file(), "{result:?}");
    assert_eq!(
        result.sha256,
        harness_core::build_identity::hash_file(&result.locator).unwrap()
    );
    assert_eq!(result.bytes, fs::metadata(&result.locator).unwrap().len());
    assert_eq!(
        first.source.revision.as_deref(),
        Some(first_revision.as_str())
    );
    assert_eq!(first.source.dirty, Some(false));

    let released = fixture.release(&[
        "--slot",
        "1",
        "--disposition",
        "discarded",
        "--reason",
        "synthetic completion accepted",
    ]);
    let text = output_text(&released);
    assert_eq!(released.status.code(), Some(0), "{text}");
    let retained_result = first.result.as_ref().expect("retained result");
    assert!(
        retained_result.locator.is_file(),
        "the per-run retained copy survives release, independent of the slot index"
    );
    assert_eq!(
        harness_core::build_identity::hash_file(&retained_result.locator).unwrap(),
        retained_result.sha256,
        "release must not alter the retained acceptance input"
    );
    assert_eq!(
        load(&first_generation).expect("retained through release"),
        first,
        "release must not change the completed run's retained facts or result"
    );

    let second_task = fixture.create_issue("Second completed task", "task", None);
    let second_assignment =
        fixture.assignment("completion-second.json", &second_task, Some("ordinary"));
    let second_session = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f6";
    let mut command = fixture.spawn_command(
        &["--assignment", second_assignment.to_str().unwrap()],
        &["--owner", "exec-ds-completion-b"],
    );
    command.env("HARNESS_EXECUTOR_FIXTURE_MODE", "hang");
    command.env("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE", "control-app-server");
    command.env("HARNESS_IMPROVEMENT_FIXTURE_SESSION", second_session);
    command.env_remove("WT_SESSION");
    let second_run = command.output().unwrap();
    let text = output_text(&second_run);
    assert_eq!(second_run.status.code(), Some(0), "{text}");

    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(
        receipt["slot"]["assignment"]["issue"], second_task,
        "{receipt}"
    );
    assert_eq!(
        receipt["observation"]["session"], second_session,
        "{receipt}"
    );
    let second_generation = receipt["originatingLead"]["runGeneration"]
        .as_str()
        .expect("second run generation")
        .to_owned();
    assert_ne!(first_generation, second_generation);
    assert_eq!(
        load(&first_generation).expect("first retained after slot reuse"),
        first,
        "reusing the slot must not replace the previous run's acceptance input"
    );
    let second = load(&second_generation).expect("the second completion is retained");
    println!(
        "retained-second={}",
        serde_json::to_string(&second).unwrap()
    );
    assert!(second.eligibility.eligible, "{second:?}");
    assert_eq!(second.task.as_deref(), Some("exec-ds-completion-b"));
    assert_eq!(
        fs::read_to_string(&slot_message).unwrap(),
        final_message,
        "the slot-indexed result now belongs to the reused slot's run"
    );

    let first_list = harness_core::task_worktree::retained_completions_for_task(
        &fixture.home,
        &fixture.base.source,
        &first_task,
    )
    .unwrap();
    assert_eq!(first_list.len(), 1, "{first_list:?}");
    assert_eq!(first_list[0].run_generation, first_generation);
    let second_list = harness_core::task_worktree::retained_completions_for_task(
        &fixture.home,
        &fixture.base.source,
        &second_task,
    )
    .unwrap();
    assert_eq!(second_list.len(), 1, "{second_list:?}");
    assert_eq!(second_list[0].run_generation, second_generation);

    // A free-text continuation keeps the bound slot's board binding but must
    // never read as the bound structured assignment. Its own output remains
    // ordinary and inspectable while acceptance eligibility is refused.
    let mut command = fixture.resume_command(
        "exec-ds-completion-b",
        second_session,
        "continue with free text",
    );
    command.env("HARNESS_EXECUTOR_FIXTURE_MODE", "hang");
    command.env("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE", "control-app-server");
    command.env("HARNESS_IMPROVEMENT_FIXTURE_SESSION", second_session);
    command.env_remove("WT_SESSION");
    let resumed = command.output().unwrap();
    let text = output_text(&resumed);
    assert_eq!(resumed.status.code(), Some(0), "{text}");
    // `resume --exec` returns at dispatch acceptance. The terminal
    // ineligibility is asserted from the retained facts below, which are the
    // authority for acceptance rather than this process's transient output.

    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(
        receipt["control"]["submittedAssignment"]["kind"], "freeText",
        "{receipt}"
    );
    assert_eq!(receipt["observation"]["state"], "completed", "{receipt}");
    assert_eq!(receipt["observation"]["exitCode"], 0, "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], second_session,
        "{receipt}"
    );
    let third_generation = receipt["originatingLead"]["runGeneration"]
        .as_str()
        .expect("continuation run generation")
        .to_owned();
    assert_ne!(third_generation, second_generation);
    let third = load(&third_generation).expect("the continuation completion is retained");
    println!(
        "retained-free-text={}",
        serde_json::to_string(&third).unwrap()
    );
    assert!(
        !third.eligibility.eligible,
        "free text must not claim the bound structured assignment: {third:?}"
    );
    assert!(
        third
            .eligibility
            .reasons
            .iter()
            .any(|reason| reason.contains("freeText")),
        "{:?}",
        third.eligibility.reasons
    );
    assert_eq!(third.task.as_deref(), Some("exec-ds-completion-b"));
    assert_eq!(
        third
            .assignment
            .as_ref()
            .map(|binding| binding.issue.as_str()),
        Some(second_task.as_str()),
        "the free-text continuation keeps the bound board issue for review"
    );
    assert_eq!(
        fs::read_to_string(&slot_message).unwrap(),
        final_message,
        "an ineligible completion keeps its ordinary output available"
    );
    assert_eq!(
        load(&first_generation).expect("first retained through continuation"),
        first
    );
    assert_eq!(
        load(&second_generation).expect("second retained through continuation"),
        second
    );

    fixture.drop_fixture();
}

#[test]
fn accept_ordinary_binds_exact_turns_emits_one_job_and_repeat_is_a_noop() {
    let fixture = AcceptanceFixture::new("accept-ordinary");
    let controller = fixture.activate();
    let task = fixture.create_task("Accept ordinary work", Some(Purpose::Ordinary));
    let retained = fixture.retain(&task, Purpose::Ordinary, "run-1", "completed", None);
    assert!(retained.eligibility.eligible, "{retained:?}");

    let receipt = fixture.accept(&task, None);
    let acceptance_id = receipt["acceptanceId"].as_str().unwrap().to_owned();
    assert_eq!(receipt["status"], "accepted", "{receipt}");
    assert_eq!(receipt["duplicate"], false, "{receipt}");
    assert_eq!(receipt["run"], "run-1", "{receipt}");
    assert_eq!(
        receipt["executorSession"], ACCEPTANCE_EXECUTOR_SESSION,
        "{receipt}"
    );
    assert_eq!(
        receipt["executorTurn"], ACCEPTANCE_EXECUTOR_TURN,
        "{receipt}"
    );
    assert_eq!(receipt["leadSession"], ACCEPTANCE_LEAD, "{receipt}");
    assert_eq!(receipt["leadTurn"], ACCEPTANCE_LEAD_TURN, "{receipt}");
    assert_eq!(
        receipt["currentLeadTurn"], ACCEPTANCE_LEAD_TURN,
        "{receipt}"
    );
    assert_eq!(receipt["reflection"]["status"], "active", "{receipt}");
    assert_eq!(receipt["reflection"]["automaticTrigger"], true, "{receipt}");
    assert_eq!(receipt["modelCalls"], 0, "{receipt}");
    let recorded = fixture.acceptance(&task).expect("accepted metadata");
    let binding: AcceptanceBinding = serde_json::from_str(&recorded).unwrap();
    assert_eq!(binding.task, task);
    assert_eq!(binding.run, "run-1");
    assert_eq!(binding.lead_session, ACCEPTANCE_LEAD);
    assert_eq!(binding.lead_turn.as_deref(), Some(ACCEPTANCE_LEAD_TURN));
    assert_eq!(
        binding.executor_turn.as_deref(),
        Some(ACCEPTANCE_EXECUTOR_TURN)
    );

    let poll = controller.poll().unwrap();
    assert_eq!(poll.model_calls, 0, "{poll:?}");
    assert_eq!(poll.jobs.len(), 1, "{poll:?}");
    let job = &poll.jobs[0];
    assert_eq!(job.acceptance_id, acceptance_id, "{job:?}");
    assert_eq!(job.stage.as_str(), "executor", "{job:?}");
    assert_eq!(job.task_id, task, "{job:?}");
    assert_eq!(
        job.source_sessions,
        vec![ACCEPTANCE_EXECUTOR_SESSION.to_owned()],
        "{job:?}"
    );
    assert_eq!(
        job.executor_session.as_deref(),
        Some(ACCEPTANCE_EXECUTOR_SESSION),
        "{job:?}"
    );
    assert_eq!(
        job.executor_turn.as_deref(),
        Some(ACCEPTANCE_EXECUTOR_TURN),
        "{job:?}"
    );
    assert_eq!(
        job.lead_turn.as_deref(),
        Some(ACCEPTANCE_LEAD_TURN),
        "{job:?}"
    );
    assert_eq!(job.lead_session, ACCEPTANCE_LEAD, "{job:?}");
    assert_eq!(job.run, "run-1", "{job:?}");
    for exact in [
        format!("assignment={}", binding.assignment),
        format!("result={}", binding.result),
        format!("revision={}", binding.revision),
    ] {
        assert!(job.context.contains(&exact), "{exact} missing from {job:?}");
    }

    // A later caller turn and a newer ineligible correction do not change the
    // accepted identity or select the newer run for the repeat.
    fixture.set_lead_turns(vec![
        json!({"id": ACCEPTANCE_LEAD_TURN, "status": "completed"}),
        json!({"id": "lead-turn-2", "status": "inProgress"}),
    ]);
    let newer = fixture.retain(&task, Purpose::Ordinary, "run-2", "failed", None);
    assert!(!newer.eligibility.eligible, "{newer:?}");
    let repeat = fixture.accept(&task, None);
    assert_eq!(repeat["duplicate"], true, "{repeat}");
    assert_eq!(repeat["acceptanceId"], acceptance_id, "{repeat}");
    assert_eq!(repeat["run"], "run-1", "{repeat}");
    assert_eq!(repeat["leadTurn"], ACCEPTANCE_LEAD_TURN, "{repeat}");
    assert_eq!(repeat["currentLeadTurn"], "lead-turn-2", "{repeat}");
    assert_eq!(repeat["reflection"]["automaticTrigger"], true, "{repeat}");
    assert_eq!(
        fixture.acceptance(&task).as_deref(),
        Some(recorded.as_str())
    );

    let after_repeat = controller.poll().unwrap();
    assert_eq!(after_repeat.events_seen, 0, "{after_repeat:?}");
    assert_eq!(after_repeat.accepted, 0, "{after_repeat:?}");
    assert_eq!(after_repeat.duplicates, 0, "{after_repeat:?}");
    assert_eq!(after_repeat.jobs.len(), 1, "{after_repeat:?}");
    assert_eq!(
        after_repeat.jobs[0].acceptance_id, acceptance_id,
        "{after_repeat:?}"
    );
    assert_eq!(after_repeat.model_calls, 0, "{after_repeat:?}");
    let mismatch = fixture.refuse(&task, Some("run-2"));
    assert!(
        mismatch.contains("does not match the recorded accepted run"),
        "{mismatch}"
    );
    assert_eq!(
        fixture.acceptance(&task).as_deref(),
        Some(recorded.as_str())
    );
    fixture.drop_fixture();
}

#[test]
fn accept_with_reflection_mode_off_keeps_the_ordinary_close() {
    let fixture = AcceptanceFixture::new("accept-mode-off");
    let task = fixture.create_task("Accept without reflection", Some(Purpose::Ordinary));
    fixture.retain(&task, Purpose::Ordinary, "run-off", "completed", None);

    let receipt = fixture.accept(&task, None);
    assert_eq!(receipt["status"], "accepted", "{receipt}");
    assert_eq!(receipt["duplicate"], false, "{receipt}");
    assert_eq!(receipt["reflection"]["status"], "inactive", "{receipt}");
    assert_eq!(
        receipt["reflection"]["automaticTrigger"], false,
        "{receipt}"
    );
    assert_eq!(receipt["modelCalls"], 0, "{receipt}");
    let shown = fixture.show(&task);
    assert_eq!(shown["status"], "closed", "{shown}");
    assert_eq!(shown["close_reason"], "accepted", "{shown}");
    assert!(fixture.acceptance(&task).is_some(), "{shown}");
    fixture.drop_fixture();
}

#[test]
fn accept_improvement_work_never_auto_triggers_reflection() {
    let fixture = AcceptanceFixture::new("accept-improvement");
    let controller = fixture.activate();
    let task = fixture.create_task("Accept improvement work", Some(Purpose::Improvement));
    fixture.retain(
        &task,
        Purpose::Improvement,
        "run-improvement",
        "completed",
        None,
    );

    let receipt = fixture.accept(&task, None);
    assert_eq!(receipt["status"], "accepted", "{receipt}");
    assert_eq!(receipt["purpose"], "improvement", "{receipt}");
    assert_eq!(
        receipt["reflection"]["automaticTrigger"], false,
        "{receipt}"
    );
    assert!(
        receipt["reflection"]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("never auto-triggers reflection")),
        "{receipt}"
    );
    let poll = controller.poll().unwrap();
    assert!(poll.jobs.is_empty(), "{poll:?}");
    assert_eq!(poll.model_calls, 0, "{poll:?}");
    assert_eq!(fixture.show(&task)["status"], "closed");
    fixture.drop_fixture();
}

/// Acceptance of a multi-generation improvement descendant retains the
/// inherited nontriggering purpose: the task closes normally with its
/// result, but the active reflection controller records an explicit
/// non-trigger refusal and schedules no job. A neighboring ordinary
/// acceptance through the same controller still emits its one job.
#[test]
fn accepted_improvement_descendant_stays_nontriggering_while_ordinary_neighbor_triggers() {
    let fixture = AcceptanceFixture::new("accept-improvement-lineage");
    let controller = fixture.activate();
    let epic = fixture
        .spawn
        .create_issue("Improvement campaign", "epic", None);
    fixture
        .spawn
        .set_metadata(&epic, "harness.purpose", "improvement");
    let child = fixture
        .spawn
        .create_issue("Split improvement work", "task", Some(&epic));
    let leaf = fixture
        .spawn
        .create_issue("Improvement leaf", "task", Some(&child));
    fixture
        .spawn
        .rename_issue(&leaf, "Renamed improvement leaf");

    let owner = "exec-ds-accepted-lineage";
    let (slot_task, handle, assignment, run) =
        fixture.bind_board("accepted-improvement.json", &leaf, None, owner);
    assert_eq!(assignment.issue, leaf);
    assert_eq!(assignment.purpose.as_deref(), Some("improvement"));
    assert_eq!(assignment.purpose_source, format!("ancestor:{epic}"));
    assert_eq!(
        fixture.show(&leaf)["metadata"]["harness.purpose"],
        "improvement"
    );
    task_worktree::release_startup_reservation(fixture.home(), fixture.checkout(), 1, owner)
        .unwrap();
    let retained = fixture.retain_bound(&run, "completed", &slot_task, &handle, owner, assignment);
    assert!(!retained.eligibility.eligible, "{retained:?}");
    assert_eq!(
        retained.eligibility.reasons,
        vec![format!(
            "board task '{leaf}' purpose 'improvement' is not ordinary"
        )],
        "{retained:?}"
    );

    let receipt = fixture.accept(&leaf, Some(&run));
    assert_eq!(receipt["status"], "accepted", "{receipt}");
    assert_eq!(receipt["purpose"], "improvement", "{receipt}");
    assert_eq!(receipt["reflection"]["status"], "active", "{receipt}");
    assert_eq!(
        receipt["reflection"]["automaticTrigger"], false,
        "{receipt}"
    );
    assert!(
        receipt["reflection"]["detail"].as_str().is_some_and(
            |detail| detail.contains("improvement work never auto-triggers reflection")
        ),
        "{receipt}"
    );
    let closed = fixture.show(&leaf);
    assert_eq!(closed["status"], "closed", "{closed}");
    assert_eq!(closed["close_reason"], "accepted", "{closed}");
    assert!(fixture.acceptance(&leaf).is_some(), "{closed}");

    let after_improvement = controller.poll().unwrap();
    assert!(after_improvement.jobs.is_empty(), "{after_improvement:?}");
    let refusal = after_improvement
            .refusals
            .iter()
            .find(|refusal| refusal.task == leaf)
            .unwrap_or_else(|| {
                panic!(
                    "the accepted improvement descendant must be explicitly refused: {after_improvement:?}"
                )
            });
    assert_eq!(
        refusal.code,
        RefusalCode::NonTriggerPurpose,
        "{after_improvement:?}"
    );
    assert!(
        refusal
            .detail
            .contains("improvement work never auto-triggers reflection"),
        "{after_improvement:?}"
    );

    // The exclusion is specific to the purpose lineage: an ordinary task
    // accepted through the same active controller still triggers exactly
    // one reflection job.
    let ordinary = fixture.create_task("Neighbor ordinary work", Some(Purpose::Ordinary));
    fixture.retain(
        &ordinary,
        Purpose::Ordinary,
        "run-ordinary-neighbor",
        "completed",
        None,
    );
    let ordinary_receipt = fixture.accept(&ordinary, None);
    assert_eq!(ordinary_receipt["status"], "accepted", "{ordinary_receipt}");
    assert_eq!(
        ordinary_receipt["reflection"]["automaticTrigger"], true,
        "{ordinary_receipt}"
    );
    let after_ordinary = controller.poll().unwrap();
    assert_eq!(after_ordinary.jobs.len(), 1, "{after_ordinary:?}");
    assert_eq!(
        after_ordinary.jobs[0].task_id, ordinary,
        "{after_ordinary:?}"
    );
    assert_eq!(after_ordinary.model_calls, 0, "{after_ordinary:?}");
    assert_eq!(after_ordinary.refusals.len(), 1, "{after_ordinary:?}");
    assert_eq!(after_ordinary.refusals[0].task, leaf, "{after_ordinary:?}");
    assert_eq!(
        after_ordinary.refusals[0].code,
        RefusalCode::NonTriggerPurpose,
        "the ordinary acceptance must not add a refusal: {after_ordinary:?}"
    );
    fixture.drop_fixture();
}

/// A missing purpose stays explicitly unknown and refuses acceptance even
/// with a completed bound run. A descendant that records ordinary after a
/// real dispatch retained its improvement lineage is refused as stale
/// instead of becoming an ordinary acceptance. Neither refusal closes the
/// task or creates a reflection event.
#[test]
fn accept_refuses_missing_and_reset_descendant_purpose() {
    let fixture = AcceptanceFixture::new("accept-lineage-refusals");
    let controller = fixture.activate();

    let unknown = fixture.create_task("Unknown purpose lineage", None);
    let unknown_owner = "exec-ds-unknown-lineage";
    let (slot_task, handle, assignment, run) =
        fixture.bind_board("unknown-lineage.json", &unknown, None, unknown_owner);
    assert_eq!(assignment.issue, unknown);
    assert!(assignment.purpose.is_none(), "{assignment:?}");
    assert_eq!(assignment.purpose_source, "unknown", "{assignment:?}");
    task_worktree::release_startup_reservation(
        fixture.home(),
        fixture.checkout(),
        1,
        unknown_owner,
    )
    .unwrap();
    let retained = fixture.retain_bound(
        &run,
        "completed",
        &slot_task,
        &handle,
        unknown_owner,
        assignment,
    );
    assert!(!retained.eligibility.eligible, "{retained:?}");
    let refused = fixture.refuse(&unknown, Some(&run));
    assert!(refused.contains("records no harness.purpose"), "{refused}");
    assert!(
        refused.contains("unknown purpose never becomes ordinary"),
        "{refused}"
    );
    assert!(fixture.acceptance(&unknown).is_none(), "{refused}");
    assert_ne!(fixture.show(&unknown)["status"], "closed", "{refused}");

    // Free the only slot for the second real dispatch; the retained result
    // remains the acceptance input for the refused unknown-purpose run.
    let released = fixture.spawn.release(&[
        "--slot",
        "1",
        "--disposition",
        "discarded",
        "--reason",
        "synthetic unknown-purpose acceptance refusal reviewed",
    ]);
    let text = output_text(&released);
    assert!(released.status.success(), "{text}");

    let epic = fixture
        .spawn
        .create_issue("Improvement parent", "epic", None);
    fixture
        .spawn
        .set_metadata(&epic, "harness.purpose", "improvement");
    let child = fixture
        .spawn
        .create_issue("Improvement reset child", "task", Some(&epic));
    let owner = "exec-ds-reset-lineage";
    let (slot_task, handle, assignment, run) =
        fixture.bind_board("reset-lineage.json", &child, None, owner);
    assert_eq!(
        assignment.purpose.as_deref(),
        Some("improvement"),
        "{assignment:?}"
    );
    task_worktree::release_startup_reservation(fixture.home(), fixture.checkout(), 1, owner)
        .unwrap();
    fixture.retain_bound(&run, "completed", &slot_task, &handle, owner, assignment);

    fixture
        .spawn
        .set_metadata(&child, "harness.purpose", "ordinary");
    let refused = fixture.refuse(&child, Some(&run));
    assert!(
        refused.contains("stale purpose is never accepted"),
        "{refused}"
    );
    assert!(refused.contains("records purpose improvement"), "{refused}");
    assert!(refused.contains("records ordinary"), "{refused}");
    assert!(fixture.acceptance(&child).is_none(), "{refused}");
    assert_ne!(fixture.show(&child)["status"], "closed", "{refused}");

    let poll = controller.poll().unwrap();
    assert!(poll.jobs.is_empty(), "{poll:?}");
    assert!(poll.refusals.is_empty(), "{poll:?}");
    assert_eq!(poll.model_calls, 0, "{poll:?}");
    fixture.drop_fixture();
}

#[test]
fn accept_refuses_a_foreign_live_lead_without_board_effects() {
    let fixture = AcceptanceFixture::new("accept-foreign-lead");
    let controller = fixture.activate();
    let task = fixture.create_task("Foreign lead task", Some(Purpose::Ordinary));
    fixture.retain(
        &task,
        Purpose::Ordinary,
        "run-foreign-lead",
        "completed",
        None,
    );
    let before = fixture.acceptance(&task);

    let foreign_thread = "lead-foreign";
    let foreign_server = Server::start(Bearer::Value(ACCEPTANCE_TOKEN.to_owned()));
    foreign_server.answer("initialize", Answer::Result(json!({})));
    foreign_server.answer(
        "thread/read",
        Answer::Result(json!({
            "thread": {
                "id": foreign_thread,
                "turns": [{"id": "foreign-turn", "status": "inProgress"}],
            },
        })),
    );
    fixture.register_endpoint(foreign_server.port, foreign_thread);
    let out = fixture.accept_raw(foreign_thread, &task, None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(text.contains("WrongLead"), "{text}");
    assert!(text.contains("not expected 'lead-foreign'"), "{text}");
    assert_eq!(fixture.acceptance(&task), before);
    assert_ne!(fixture.show(&task)["status"], "closed");
    assert_eq!(controller.poll().unwrap().jobs.len(), 0);
    fixture.drop_fixture();
}

#[test]
fn accept_requires_exactly_one_in_progress_lead_turn() {
    let fixture = AcceptanceFixture::new("accept-turn-count");
    let task = fixture.create_task("Exact turn count", Some(Purpose::Ordinary));
    fixture.retain(
        &task,
        Purpose::Ordinary,
        "run-turn-count",
        "completed",
        None,
    );
    let before = fixture.acceptance(&task);

    fixture.set_lead_turns(vec![json!({"id": "lead-turn-0", "status": "completed"})]);
    let none = fixture.refuse(&task, None);
    assert!(none.contains("has 0 in-progress native turns"), "{none}");

    fixture.set_lead_turns(vec![
        json!({"id": "lead-turn-a", "status": "inProgress"}),
        json!({"id": "lead-turn-b", "status": "inProgress"}),
    ]);
    let multiple = fixture.refuse(&task, None);
    assert!(
        multiple.contains("has 2 in-progress native turns"),
        "{multiple}"
    );
    assert_eq!(fixture.acceptance(&task), before);
    assert_ne!(fixture.show(&task)["status"], "closed");
    fixture.drop_fixture();
}

#[test]
fn accept_refuses_a_newer_ineligible_completion_even_with_the_old_exact_run() {
    let fixture = AcceptanceFixture::new("accept-run-selection");
    let task = fixture.create_task("Run selection", Some(Purpose::Ordinary));
    fixture.retain(&task, Purpose::Ordinary, "run-1", "completed", None);
    fixture.retain(&task, Purpose::Ordinary, "run-2", "failed", None);
    let before = fixture.acceptance(&task);

    let newest = fixture.refuse(&task, None);
    assert!(newest.contains("run-2"), "{newest}");
    assert!(newest.contains("not a completed zero-exit"), "{newest}");
    assert_eq!(fixture.acceptance(&task), before);
    assert_ne!(fixture.show(&task)["status"], "closed");

    let explicit = fixture.refuse(&task, Some("run-1"));
    assert!(explicit.contains("run-2"), "{explicit}");
    assert!(explicit.contains("newer correction"), "{explicit}");
    assert_eq!(fixture.acceptance(&task), before);
    assert_ne!(fixture.show(&task)["status"], "closed");

    let unrelated = fixture.create_task("Unrelated task", Some(Purpose::Ordinary));
    fixture.retain(
        &unrelated,
        Purpose::Ordinary,
        "run-unrelated",
        "completed",
        None,
    );
    let foreign = fixture.refuse(&task, Some("run-unrelated"));
    assert!(
        foreign.contains("retained for another board task"),
        "{foreign}"
    );
    assert_eq!(fixture.acceptance(&task), before);
    fixture.drop_fixture();
}

/// A retained completion is not enough to accept an open board task when the
/// durable slot owner records a later correction for that same task. The
/// unfinished correction has no completion manifest, so only the board-bound
/// slot record can prevent the older result from being accepted by omission.
#[test]
fn accept_refuses_a_stale_result_while_a_newer_correction_is_unresolved() {
    let fixture = AcceptanceFixture::new("accept-unresolved-correction");
    let task = fixture.create_task("Corrected task", Some(Purpose::Ordinary));
    fixture.retain(&task, Purpose::Ordinary, "run-completed", "completed", None);
    let before = fixture.show(&task);

    let correction =
        fixture
            .spawn
            .assignment("unresolved-correction.json", &task, Some("ordinary"));
    let dispatched = fixture.spawn.spawn(
        &["--assignment", correction.to_str().unwrap()],
        &["--owner", "exec-ds-unresolved-correction"],
    );
    let text = output_text(&dispatched);
    assert!(!dispatched.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "the correction must stop only after its durable slot binding: {text}"
    );
    let record = fixture.spawn.record(1);
    let correction_task = record["task"]
        .as_str()
        .expect("durable correction task")
        .to_owned();
    let correction_handle = record["handle"]
        .as_str()
        .expect("durable correction handle")
        .to_owned();
    let correction_run = fixture.spawn.receipt(1)["originatingLead"]["runGeneration"]
        .as_str()
        .expect("correction dispatch generation")
        .to_owned();
    assert_eq!(record["assignment"]["issue"], task, "{record}");
    assert_ne!(correction_task, "executor-slot-binding", "{record}");

    let newest = fixture.refuse(&task, None);
    assert!(newest.contains("run-completed"), "{newest}");
    assert!(newest.contains(&correction_task), "{newest}");
    assert!(newest.contains(&correction_run), "{newest}");
    assert!(newest.contains("unresolved"), "{newest}");
    assert_eq!(fixture.show(&task)["status"], before["status"]);
    assert_eq!(fixture.acceptance(&task), None);

    let explicit = fixture.refuse(&task, Some("run-completed"));
    assert!(explicit.contains("run-completed"), "{explicit}");
    assert!(explicit.contains(&correction_task), "{explicit}");
    assert!(explicit.contains(&correction_handle), "{explicit}");
    assert!(explicit.contains(&correction_run), "{explicit}");
    assert!(explicit.contains("unresolved"), "{explicit}");
    assert_eq!(fixture.show(&task)["status"], before["status"]);
    assert_eq!(fixture.acceptance(&task), None);

    let released = fixture.spawn.release(&[
        "--slot",
        "1",
        "--disposition",
        "discarded",
        "--reason",
        "synthetic correction resolved before accepting the completed run",
    ]);
    let released_text = output_text(&released);
    assert!(released.status.success(), "{released_text}");

    let unrelated = fixture.create_task("Unrelated task", Some(Purpose::Ordinary));
    let unrelated_assignment =
        fixture
            .spawn
            .assignment("unrelated-reuse.json", &unrelated, Some("ordinary"));
    let reused = fixture.spawn.spawn(
        &["--assignment", unrelated_assignment.to_str().unwrap()],
        &["--owner", "exec-ds-unrelated-reuse"],
    );
    let reused_text = output_text(&reused);
    assert!(!reused.status.success(), "{reused_text}");
    assert!(
        reused_text.contains("installed Codex launcher is missing"),
        "{reused_text}"
    );
    let reused_record = fixture.spawn.record(1);
    assert_eq!(
        reused_record["assignment"]["issue"], unrelated,
        "{reused_record}"
    );

    let accepted = fixture.accept(&task, Some("run-completed"));
    assert_eq!(accepted["run"], "run-completed", "{accepted}");
    assert_eq!(accepted["duplicate"], false, "{accepted}");
    let repeated = fixture.accept(&task, Some("run-completed"));
    assert_eq!(repeated["duplicate"], true, "{repeated}");

    // Same-task handoffs keep the durable task and generation handle. A
    // startup reservation means the slot's receipt may still describe the
    // prior run, so a retained completion for that same binding is not proof
    // that the startup finished.
    let reused_task = reused_record["task"]
        .as_str()
        .expect("reused durable task")
        .to_owned();
    let reused_handle = reused_record["handle"]
        .as_str()
        .expect("reused generation handle")
        .to_owned();
    let reused_owner = reused_record["owner"]
        .as_str()
        .expect("reused slot owner")
        .to_owned();
    let reused_assignment: SlotAssignmentBinding =
        serde_json::from_value(reused_record["assignment"].clone()).unwrap();
    let reused_run = fixture.spawn.receipt(1)["originatingLead"]["runGeneration"]
        .as_str()
        .expect("reused dispatch generation")
        .to_owned();
    task_worktree::hold_startup_reservation(fixture.home(), fixture.checkout(), 1, &reused_owner)
        .unwrap();
    assert!(
        fixture.spawn.record(1)["startup"].is_object(),
        "{}",
        fixture.spawn.record(1)
    );
    fixture.retain_bound(
        &reused_run,
        "completed",
        &reused_task,
        &reused_handle,
        &reused_owner,
        reused_assignment,
    );
    let before_handoff = fixture.show(&unrelated);
    let handoff = fixture.refuse(&unrelated, Some(&reused_run));
    assert!(handoff.contains("startup"), "{handoff}");
    assert!(handoff.contains(&reused_task), "{handoff}");
    assert_eq!(fixture.show(&unrelated)["status"], before_handoff["status"]);
    assert_eq!(fixture.acceptance(&unrelated), None);

    task_worktree::release_startup_reservation(
        fixture.home(),
        fixture.checkout(),
        1,
        &reused_owner,
    )
    .unwrap();
    let accepted_reuse = fixture.accept(&unrelated, Some(&reused_run));
    assert_eq!(accepted_reuse["run"], reused_run, "{accepted_reuse}");
    assert_eq!(accepted_reuse["duplicate"], false, "{accepted_reuse}");
    fixture.drop_fixture();
}

/// A new structured correction can reuse the same durable task and generation
/// handle while changing the assignment content before it writes a new run
/// receipt. The retained completion's task and handle alone must not make the
/// old result current; acceptance binds the assignment identity as well.
#[test]
fn accept_refuses_a_changed_assignment_for_the_same_slot_and_handle() {
    let fixture = AcceptanceFixture::new("accept-changed-assignment");
    let task = fixture.create_task("Changed assignment", Some(Purpose::Ordinary));
    let owner = "exec-ds-changed-assignment";
    let (slot_task, handle, assignment, run) =
        fixture.bind_correction("changed-first.json", &task, owner);
    fixture.retain_bound(&run, "completed", &slot_task, &handle, owner, assignment);
    let before = fixture.show(&task);

    let changed = harness_core::build_identity::hash_bytes(b"changed same-slot correction");
    let mut record = fixture.spawn.record(1);
    assert_eq!(record["task"], json!(slot_task.as_str()), "{record}");
    assert_eq!(record["handle"], json!(handle.as_str()), "{record}");
    record["assignment"]["assignmentSha256"] = json!(changed.as_str());
    fs::write(
        fixture.spawn.record_path(1),
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();
    let changed_record = fixture.spawn.record(1);
    assert_eq!(
        changed_record["task"],
        json!(slot_task.as_str()),
        "{changed_record}"
    );
    assert_eq!(
        changed_record["handle"],
        json!(handle.as_str()),
        "{changed_record}"
    );
    assert_eq!(
        changed_record["assignment"]["assignmentSha256"],
        json!(changed.as_str()),
        "{changed_record}"
    );

    for requested in [None, Some(run.as_str())] {
        let refused = fixture.refuse(&task, requested);
        assert!(
            refused.contains("unresolved executor correction"),
            "{refused}"
        );
        assert!(refused.contains(&changed), "{refused}");
        assert!(refused.contains(&run), "{refused}");
        assert!(
            refused.contains("No board metadata was changed"),
            "{refused}"
        );
        assert_eq!(fixture.show(&task)["status"], before["status"]);
        assert_eq!(fixture.acceptance(&task), None);
    }

    let released = fixture.spawn.release(&[
        "--slot",
        "1",
        "--disposition",
        "discarded",
        "--reason",
        "synthetic changed-assignment correction resolved before accepting the completed run",
    ]);
    let released_text = output_text(&released);
    assert!(released.status.success(), "{released_text}");

    let accepted = fixture.accept(&task, Some(&run));
    assert_eq!(accepted["run"], run, "{accepted}");
    assert_eq!(accepted["duplicate"], false, "{accepted}");
    let repeated = fixture.accept(&task, Some(&run));
    assert_eq!(repeated["run"], run, "{repeated}");
    assert_eq!(repeated["duplicate"], true, "{repeated}");
    fixture.drop_fixture();
}

/// The pre-close observation and dispatch preparation share the pool-wide
/// admission owner. While that guard is held, acceptance must not close the
/// board; a same-task reservation published inside the claim-to-bind window
/// must make the later observation refuse instead of closing from stale state.
#[test]
fn accept_does_not_close_while_a_same_task_reservation_crosses_validation() {
    let fixture = AcceptanceFixture::new("accept-interleaved-reservation");
    let task = fixture.create_task("Interleaved reservation", Some(Purpose::Ordinary));
    let owner = "exec-ds-interleaved";
    let (slot_task, handle, assignment, run) =
        fixture.bind_correction("interleaved-first.json", &task, owner);
    fixture.retain_bound(&run, "completed", &slot_task, &handle, owner, assignment);
    let before = fixture.show(&task);

    let admission =
        task_worktree::begin_pool_admission(fixture.home(), fixture.checkout()).unwrap();
    let mut child = fixture.accept_spawn(&task, Some(&run));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let out = child.wait_with_output().unwrap();
            panic!(
                "acceptance exited while the pool admission lock was held ({status:?}): {}",
                output_text(&out)
            );
        }
        assert_ne!(
            fixture.show(&task)["status"],
            json!("closed"),
            "acceptance closed the board while the pool admission lock was held"
        );
        if std::time::Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    // Publish the same-task startup reservation before the dispatch binds its
    // board assignment. A fresh claim reuses the slot task with a new
    // generation handle, so the reservation is identified by its durable task,
    // not by the retained completion's handle. This is the claim-to-bind window
    // the admission guard serializes with the acceptance observation.
    task_worktree::hold_startup_reservation(fixture.home(), fixture.checkout(), 1, owner).unwrap();
    let mut unbound = fixture.spawn.record(1);
    assert!(unbound["startup"].is_object(), "{unbound}");
    unbound["handle"] = json!("interleaved-new-generation");
    unbound["assignment"] = Value::Null;
    fs::write(
        fixture.spawn.record_path(1),
        serde_json::to_vec_pretty(&unbound).unwrap(),
    )
    .unwrap();
    drop(admission);

    let out = child.wait_with_output().unwrap();
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(text.contains("unresolved executor correction"), "{text}");
    assert!(text.contains("no board assignment is published"), "{text}");
    assert!(text.contains("interleaved-new-generation"), "{text}");
    assert!(text.contains("startup reservation"), "{text}");
    assert_eq!(fixture.show(&task)["status"], before["status"]);
    assert_eq!(fixture.acceptance(&task), None);

    task_worktree::release_startup_reservation(fixture.home(), fixture.checkout(), 1, owner)
        .unwrap();
    let released = fixture.spawn.release(&[
        "--slot",
        "1",
        "--disposition",
        "discarded",
        "--reason",
        "synthetic interleaved reservation resolved before accepting the completed run",
    ]);
    let released_text = output_text(&released);
    assert!(released.status.success(), "{released_text}");

    let accepted = fixture.accept(&task, Some(&run));
    assert_eq!(accepted["run"], run, "{accepted}");
    assert_eq!(accepted["duplicate"], false, "{accepted}");
    let repeated = fixture.accept(&task, Some(&run));
    assert_eq!(repeated["duplicate"], true, "{repeated}");
    fixture.drop_fixture();
}

#[test]
fn accept_requires_run_when_the_newest_generations_share_a_timestamp() {
    let fixture = AcceptanceFixture::new("accept-run-timestamp");
    let task = fixture.create_task("Timestamp ambiguity", Some(Purpose::Ordinary));
    let first = fixture.retain(&task, Purpose::Ordinary, "run-a", "completed", None);
    let second = fixture.retain(&task, Purpose::Ordinary, "run-b", "completed", None);
    assert!(second.recorded_ms > first.recorded_ms);
    fixture.set_recorded_ms("run-a", second.recorded_ms);

    let ambiguous = fixture.refuse(&task, None);
    assert!(ambiguous.contains("same newest timestamp"), "{ambiguous}");
    assert!(ambiguous.contains("--run GENERATION"), "{ambiguous}");
    assert_eq!(fixture.acceptance(&task), None);
    assert_ne!(fixture.show(&task)["status"], "closed");

    let accepted = fixture.accept(&task, Some("run-b"));
    assert_eq!(accepted["run"], "run-b", "{accepted}");
    assert_eq!(accepted["duplicate"], false, "{accepted}");
    assert_eq!(fixture.show(&task)["status"], "closed");
    fixture.drop_fixture();
}

#[test]
fn accept_refuses_a_stale_board_source_without_metadata() {
    let fixture = AcceptanceFixture::new("accept-stale-source");
    let task = fixture.create_task("Stale source", Some(Purpose::Ordinary));
    let mut stale = fixture.board_source();
    stale.project_id = "another-board-project".to_owned();
    fixture.retain(
        &task,
        Purpose::Ordinary,
        "run-stale",
        "completed",
        Some(stale),
    );

    let refused = fixture.refuse(&task, None);
    assert!(refused.contains("stale board source"), "{refused}");
    assert!(fixture.acceptance(&task).is_none(), "{refused}");
    assert_ne!(fixture.show(&task)["status"], "closed");
    fixture.drop_fixture();
}

#[test]
fn accept_refuses_a_changed_result_without_rewriting_acceptance() {
    let fixture = AcceptanceFixture::new("accept-changed-result");
    let task = fixture.create_task("Changed result", Some(Purpose::Ordinary));
    let retained = fixture.retain(&task, Purpose::Ordinary, "run-result", "completed", None);
    fixture.accept(&task, None);
    let before = fixture.acceptance(&task).expect("accepted metadata");
    let locator = retained
        .result
        .as_ref()
        .expect("retained result")
        .locator
        .clone();
    fs::write(locator, "changed after acceptance\n").unwrap();

    let refused = fixture.refuse(&task, None);
    assert!(refused.contains("retained result artifact"), "{refused}");
    assert!(refused.contains("not acceptable"), "{refused}");
    assert_eq!(fixture.acceptance(&task).as_deref(), Some(before.as_str()));
    assert_eq!(fixture.show(&task)["status"], "closed");
    fixture.drop_fixture();
}

#[test]
fn accept_refuses_unknown_purpose_and_another_close_reason() {
    let fixture = AcceptanceFixture::new("accept-refusals");
    let unknown = fixture.create_task("Unknown purpose", None);
    let refused = fixture.refuse(&unknown, None);
    assert!(refused.contains("records no harness.purpose"), "{refused}");
    assert!(fixture.acceptance(&unknown).is_none(), "{refused}");

    let cancelled = fixture.create_task("Cancelled work", Some(Purpose::Ordinary));
    fixture.close_with_reason(&cancelled, "cancelled");
    let refused = fixture.refuse(&cancelled, None);
    assert!(refused.contains("not accepted"), "{refused}");
    assert!(refused.contains("cancelled"), "{refused}");
    assert!(fixture.acceptance(&cancelled).is_none(), "{refused}");
    fixture.drop_fixture();
}

#[test]
fn accepted_without_exact_turn_metadata_repeats_but_is_reflection_ineligible() {
    let fixture = AcceptanceFixture::new("accept-missing-turns");
    let task = fixture.create_task("Legacy accepted record", Some(Purpose::Ordinary));
    fixture.retain(&task, Purpose::Ordinary, "run-legacy", "completed", None);
    let first = fixture.accept(&task, None);
    let acceptance_id = first["acceptanceId"].as_str().unwrap().to_owned();
    let recorded = fixture.acceptance(&task).expect("accepted metadata");
    let binding: AcceptanceBinding = serde_json::from_str(&recorded).unwrap();
    let stripped = binding.with_turns(None, None, None);
    let stripped_raw = serde_json::to_string(&stripped).unwrap();
    fixture
        .spawn
        .set_metadata(&task, ACCEPTANCE_METADATA, &stripped_raw);

    let repeat = fixture.accept(&task, None);
    assert_eq!(repeat["duplicate"], true, "{repeat}");
    assert_eq!(repeat["acceptanceId"], acceptance_id, "{repeat}");
    assert!(repeat["executorSession"].is_null(), "{repeat}");
    assert!(repeat["executorTurn"].is_null(), "{repeat}");
    assert!(repeat["leadTurn"].is_null(), "{repeat}");
    assert_eq!(repeat["reflection"]["automaticTrigger"], false, "{repeat}");
    assert!(
        repeat["reflection"]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("no exact executor session")),
        "{repeat}"
    );
    assert_eq!(
        fixture.acceptance(&task).as_deref(),
        Some(stripped_raw.as_str())
    );
    fixture.drop_fixture();
}
