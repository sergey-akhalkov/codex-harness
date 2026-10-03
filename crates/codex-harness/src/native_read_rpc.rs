//! Fixed model-free app-server reads inside one owned Windows job.
use crate::outcome_run::{create, write_new};
use harness_core::process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Write},
    os::windows::io::FromRawHandle,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const RECORD_LIMIT: usize = 8 * 1024 * 1024;
const REQUEST_LIMIT: usize = 32 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum Protocol {
    Outcome,
    Sources,
    Profile,
}

impl Protocol {
    fn response_keys(self) -> &'static [&'static str] {
        match self {
            Self::Outcome => &["native", "listed", "config"],
            Self::Sources => &["native", "config", "requirements", "listed"],
            Self::Profile => &["native", "config"],
        }
    }
}

pub(crate) struct Request<'a> {
    pub upstream: &'a Path,
    pub case: &'a Path,
    pub working_directory: &'a Path,
    pub home: &'a Path,
    pub extra: &'a [String],
    pub root: &'a Path,
    pub timeout: Duration,
    pub output_limit: u64,
    pub protocol: Protocol,
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

fn pipe() -> io::Result<(File, File)> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Pipes::CreatePipe,
    };
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null_mut(), 64 * 1024) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if read.is_null()
        || write.is_null()
        || read == INVALID_HANDLE_VALUE
        || write == INVALID_HANDLE_VALUE
    {
        unsafe {
            if !read.is_null() && read != INVALID_HANDLE_VALUE {
                CloseHandle(read);
            }
            if !write.is_null() && write != INVALID_HANDLE_VALUE {
                CloseHandle(write);
            }
        }
        return Err(invalid("pipe returned an invalid handle"));
    }
    // CreatePipe returns two new, non-inheritable owned handles. CommandSpec
    // duplicates only the child's read endpoint into its explicit handle list.
    Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
}

struct Channel {
    input: File,
    output: BufReader<File>,
    requests: File,
    pending: Vec<u8>,
    deadline: Deadline,
    cancellation: Cancellation,
}
impl Channel {
    fn send(&mut self, value: &Value) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        if bytes.len() > REQUEST_LIMIT {
            return Err(invalid("RPC request size limit"));
        }
        self.requests.write_all(&bytes)?;
        self.requests.flush()?;
        self.input.write_all(&bytes)?;
        self.input.flush()
    }
    fn request(&mut self, id: u64, method: &str, params: Value) -> io::Result<Value> {
        self.send(&json!({"id":id,"method":method,"params":params}))?;
        loop {
            if self.cancellation.is_cancelled() || self.deadline.expired() {
                return Err(invalid("RPC stopped before its response"));
            }
            let mut bounded =
                Read::by_ref(&mut self.output).take((RECORD_LIMIT + 1 - self.pending.len()) as u64);
            let count = bounded.read_until(b'\n', &mut self.pending)?;
            if self.pending.len() > RECORD_LIMIT {
                return Err(invalid("RPC response size limit"));
            }
            if !self.pending.ends_with(b"\n") {
                if count == 0 {
                    std::thread::sleep(Duration::from_millis(10));
                }
                continue;
            }
            let row: Value = serde_json::from_slice(&std::mem::take(&mut self.pending))
                .map_err(|_| invalid("malformed native RPC response"))?;
            if !row.is_object() {
                return Err(invalid("native RPC response is not an object"));
            }
            if row.get("id").is_none() && row["method"].is_string() {
                continue;
            }
            if row["id"] != id || row.get("method").is_some() {
                return Err(invalid("unexpected native RPC response identity"));
            }
            if row.get("error").is_some() {
                return Err(invalid("native RPC returned an error; see rpc.jsonl"));
            }
            return row
                .get("result")
                .cloned()
                .ok_or_else(|| invalid("native RPC response lacks result"));
        }
    }
    fn discover(
        mut self,
        case: &Path,
        home: &Path,
        root: &Path,
        protocol: Protocol,
    ) -> io::Result<Value> {
        let result = (|| {
            let client = match protocol {
                Protocol::Outcome => "outcome-discovery",
                _ => "codex-harness-source-check",
            };
            let native = self.request(1,"initialize",json!({"clientInfo":{"name":client,"version":"1"},"capabilities":{"experimentalApi":true}}))?;
            write_new(&root.join("initialize.json"), &native)?;
            let observed_home = native["codexHome"]
                .as_str()
                .map(Path::new)
                .ok_or_else(|| invalid("initialize did not identify the effective Codex home"))?;
            if !observed_home.is_absolute()
                || observed_home.canonicalize()? != home
                || native["platformFamily"] != "windows"
                || native["platformOs"] != "windows"
                || !native["userAgent"].as_str().is_some_and(|s| !s.is_empty())
            {
                return Err(invalid(
                    "initialize did not confirm the requested native context",
                ));
            }
            self.send(&json!({"method":"initialized"}))?;
            let response = match protocol {
                Protocol::Outcome => {
                    let listed =
                        self.request(2, "skills/list", json!({"cwds":[case],"forceReload":true}))?;
                    let config = self.request(3, "config/read", json!({"cwd":case}))?;
                    json!({"native":native,"listed":listed,"config":config})
                }
                Protocol::Sources => {
                    let config =
                        self.request(2, "config/read", json!({"cwd":case,"includeLayers":true}))?;
                    let requirements = self.request(3, "configRequirements/read", json!({}))?;
                    let listed =
                        self.request(4, "skills/list", json!({"cwds":[case],"forceReload":true}))?;
                    json!({"native":native,"config":config,"requirements":requirements,"listed":listed})
                }
                Protocol::Profile => {
                    let config =
                        self.request(2, "config/read", json!({"cwd":case,"includeLayers":true}))?;
                    json!({"native":native,"config":config})
                }
            };
            write_new(&root.join("response.json"), &response)?;
            Ok(response)
        })();
        if result.is_err() {
            self.cancellation.cancel();
        }
        // Closing this only parent writer supplies EOF; Job.wait owns shutdown.
        result
    }
}

/// Git variables that change which repository, work tree, common directory or
/// object store a child resolves. The discovery child runs beside a controlled
/// case whose isolation depends on resolution following its own working
/// directory, while this process may itself run inside a linked worktree with
/// these exported, so the child must not inherit them.
const GIT_REDIRECTION_VARIABLES: [&str; 5] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

/// Drop every Git variable that can point the child at another repository's
/// history. Variables such as `GIT_INDEX_FILE`, `GIT_NAMESPACE` or the
/// discovery bounds do not select another repository or object store, so they
/// stay untouched.
fn restrict_git_resolution(command: &mut CommandSpec) {
    for name in GIT_REDIRECTION_VARIABLES {
        command.env.insert(name.into(), None);
    }
}

/// Build the app-server child command for one discovery exchange. The child
/// inherits the parent environment except for `CODEX_HOME` and the Git
/// variables that could redirect repository resolution.
fn discovery_command(
    upstream: &Path,
    working_directory: &Path,
    home: &Path,
    extra: &[String],
) -> CommandSpec {
    let mut command = CommandSpec::new(upstream);
    command.args = ["app-server", "--stdio"]
        .into_iter()
        .map(OsString::from)
        .chain(extra.iter().map(OsString::from))
        .collect();
    command.current_dir = Some(working_directory.to_owned());
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.as_os_str().into()));
    restrict_git_resolution(&mut command);
    command
}

pub(crate) fn exchange(request: Request<'_>, report: &mut Value) -> io::Result<Value> {
    let Request {
        upstream,
        case,
        working_directory,
        home,
        extra,
        root,
        timeout,
        output_limit,
        protocol,
    } = request;
    let recorded_seconds = if timeout.subsec_nanos() == 0 {
        json!(timeout.as_secs())
    } else {
        json!(timeout.as_secs_f64())
    };
    let stdout = root.join("rpc.jsonl");
    let stderr = root.join("stderr.txt");
    let (input, writer) = pipe()?;
    let mut command = discovery_command(upstream, working_directory, home, extra);
    command.stdin = Some(input);
    command.stdout = Some(create(&stdout)?);
    command.stderr = Some(create(&stderr)?);
    write_new(
        &root.join("request.json"),
        &json!({"executable":upstream,"arguments":command.args.iter().map(|s|s.to_string_lossy()).collect::<Vec<_>>(),"cwd":working_directory,"codex_home":home,
        "memory_limit_bytes":512*1024*1024_u64,"timeout_seconds":recorded_seconds,"output_limit":output_limit}),
    )?;
    let cancellation = Cancellation::default();
    let deadline = Deadline::after(timeout)?;
    let channel = Channel {
        input: writer,
        output: BufReader::new(File::open(&stdout)?),
        requests: create(&root.join("requests.jsonl"))?,
        pending: Vec::new(),
        deadline,
        cancellation: cancellation.clone(),
    };
    let job = Job::new(Limits {
        memory_bytes: Some(512 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })?;
    let suspended = job.spawn_suspended(&command)?;
    if !job.contains(suspended.process())? {
        return Err(invalid("discovery process is outside its job"));
    }
    let identity = suspended.process().identity();
    write_new(
        &root.join("process-started.json"),
        &json!({"process_id":identity.pid,"assigned_before_resume":true}),
    )?;
    let child = suspended.resume()?;
    drop(command);
    let worker = {
        let case = case.to_owned();
        let home = home.to_owned();
        let root = root.to_owned();
        std::thread::spawn(move || channel.discover(&case, &home, &root, protocol))
    };
    let stop = Arc::new(AtomicBool::new(false));
    let over_limit = Arc::new(AtomicBool::new(false));
    let monitor = {
        let stop = stop.clone();
        let over_limit = over_limit.clone();
        let cancellation = cancellation.clone();
        let paths = [stdout.clone(), stderr.clone()];
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if exceeded(&paths, output_limit) {
                    over_limit.store(true, Ordering::Relaxed);
                    cancellation.cancel();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };
    let outcome = job.wait(&child, deadline, &cancellation, Duration::from_secs(5));
    stop.store(true, Ordering::Relaxed);
    cancellation.cancel();
    let _ = monitor.join();
    let protocol_result = worker.join().map_err(|_| invalid("RPC worker panicked"));
    let outcome = outcome?;
    let output_exceeded =
        over_limit.load(Ordering::Relaxed) || exceeded(&[stdout, stderr], output_limit);
    let receipt = json!({"outcome":outcome,"assigned_before_resume":true,"process_id":identity.pid,"output_limit_reached":output_exceeded});
    write_new(&root.join("process.json"), &receipt)?;
    report["process"] = receipt;
    let response = protocol_result??;
    if output_exceeded {
        return Err(invalid("native discovery output limit"));
    }
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(invalid(
            "native discovery process did not exit successfully",
        ));
    }
    verify_complete_transcript(&root.join("rpc.jsonl"), &response, output_limit, protocol)?;
    Ok(response)
}

/// The last wanted reply may already be buffered when a contradictory response
/// or unsupported server request follows it. Inspect the complete retained
/// stream after owned shutdown, including records after config/read.
fn verify_complete_transcript(
    path: &Path,
    response: &Value,
    limit: u64,
    protocol: Protocol,
) -> io::Result<()> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut total = 0_u64;
    let mut seen = 0_usize;
    let keys = protocol.response_keys();
    loop {
        let mut bytes = Vec::new();
        let count = Read::by_ref(&mut reader)
            .take((RECORD_LIMIT + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > limit || bytes.len() > RECORD_LIMIT || !bytes.ends_with(b"\n") {
            return Err(invalid("incomplete or over-limit completed RPC transcript"));
        }
        let row: Value = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("malformed completed RPC transcript"))?;
        if row.is_object()
            && row.get("id").is_none()
            && row["method"].is_string()
            && row.get("result").is_none()
            && row.get("error").is_none()
        {
            continue;
        }
        if seen >= keys.len()
            || row["id"] != (seen as u64 + 1)
            || row.get("method").is_some()
            || row.get("error").is_some()
            || row.get("result") != Some(&response[keys[seen]])
        {
            return Err(invalid(
                "contradictory or unexpected completed RPC response",
            ));
        }
        seen += 1;
    }
    if seen != keys.len() {
        return Err(invalid("incomplete completed RPC transcript"));
    }
    Ok(())
}
fn exceeded(paths: &[PathBuf], limit: u64) -> bool {
    paths
        .iter()
        .any(|p| fs::metadata(p).is_ok_and(|m| m.len() > limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_program() -> PathBuf {
        let output = std::process::Command::new("where.exe")
            .arg("git")
            .output()
            .expect("where.exe is available");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(PathBuf::from)
            .expect("git is required to probe repository resolution")
    }

    /// Run `git rev-parse --show-toplevel` the way the discovery child runs:
    /// through `CommandSpec`, from the isolated case directory, optionally with
    /// the Git redirection variables added and the production sanitation
    /// applied. Returns the exit code and the resolved top-level directory.
    fn probe(
        git: &Path,
        root: &Path,
        case: &Path,
        redirect: Option<(&Path, &Path)>,
        sanitize: bool,
    ) -> (u32, Option<PathBuf>) {
        let stdout = root.join("probe-stdout.txt");
        let stderr = root.join("probe-stderr.txt");
        let mut command = CommandSpec::new(git);
        command.args = ["rev-parse", "--show-toplevel"]
            .into_iter()
            .map(OsString::from)
            .collect();
        command.current_dir = Some(case.to_owned());
        if let Some((git_dir, work_tree)) = redirect {
            command
                .env
                .insert("GIT_DIR".into(), Some(git_dir.as_os_str().into()));
            command
                .env
                .insert("GIT_WORK_TREE".into(), Some(work_tree.as_os_str().into()));
        }
        if sanitize {
            restrict_git_resolution(&mut command);
        }
        command.stdout = Some(File::create(&stdout).unwrap());
        command.stderr = Some(File::create(&stderr).unwrap());
        let job = Job::new(Limits {
            memory_bytes: Some(256 * 1024 * 1024),
            cpu_percent: Some(90.0),
        })
        .unwrap();
        let suspended = job.spawn_suspended(&command).unwrap();
        let child = suspended.resume().unwrap();
        let outcome = job
            .wait(
                &child,
                Deadline::after(Duration::from_secs(30)).unwrap(),
                &Cancellation::default(),
                Duration::from_secs(5),
            )
            .unwrap();
        let text = fs::read_to_string(&stdout).unwrap_or_default();
        (
            outcome.exit_code,
            PathBuf::from(text.trim()).canonicalize().ok(),
        )
    }

    #[test]
    fn discovery_command_drops_git_redirection_and_keeps_its_owned_context() {
        let command = discovery_command(
            Path::new("C:\\owned\\codex.exe"),
            Path::new("C:\\owned\\case"),
            Path::new("C:\\owned\\home"),
            &["--extra".to_owned()],
        );
        assert_eq!(
            command.current_dir.as_deref(),
            Some(Path::new("C:\\owned\\case"))
        );
        assert_eq!(
            command.args,
            ["app-server", "--stdio", "--extra"].map(OsString::from)
        );
        assert_eq!(
            command.env.get(&OsString::from("CODEX_HOME")),
            Some(&Some(OsString::from("C:\\owned\\home")))
        );
        for name in GIT_REDIRECTION_VARIABLES {
            assert_eq!(command.env.get(&OsString::from(name)), Some(&None));
        }
        assert_eq!(command.env.len(), GIT_REDIRECTION_VARIABLES.len() + 1);
    }

    #[test]
    fn git_redirection_cannot_resolve_sibling_history_after_sanitation() {
        let git = git_program();
        let root = tempfile::tempdir().unwrap();
        let sibling = root.path().join("sibling");
        let status = std::process::Command::new(&git)
            .args(["init", "-q"])
            .arg(&sibling)
            .status()
            .unwrap();
        assert!(status.success(), "git init failed for the sibling fixture");
        let case = root.path().join("case");
        fs::create_dir(&case).unwrap();
        let git_dir = sibling.join(".git");
        let sibling_top = sibling.canonicalize().unwrap();

        // The redirection is effective without sanitation: the child resolves
        // sibling history from an isolated case directory.
        let control = probe(&git, root.path(), &case, Some((&git_dir, &sibling)), false);
        assert_eq!(control.0, 0);
        assert_eq!(control.1, Some(sibling_top.clone()));

        // The production sanitation removes the redirection, so the same child
        // cannot reach the sibling repository.
        let sanitized = probe(&git, root.path(), &case, Some((&git_dir, &sibling)), true);
        assert_ne!(sanitized.1, Some(sibling_top));
    }
}
