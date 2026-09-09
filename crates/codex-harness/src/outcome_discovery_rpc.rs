//! Fixed initialize/skills/config protocol inside one owned Windows job.
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
    fn discover(mut self, case: &Path, home: &Path, root: &Path) -> io::Result<Value> {
        let result = (|| {
            let native = self.request(1,"initialize",json!({"clientInfo":{"name":"outcome-discovery","version":"1"},"capabilities":{"experimentalApi":true}}))?;
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
            let listed =
                self.request(2, "skills/list", json!({"cwds":[case],"forceReload":true}))?;
            let config = self.request(3, "config/read", json!({"cwd":case}))?;
            let response = json!({"native":native,"listed":listed,"config":config});
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

pub(super) fn exchange(
    request: &super::Request,
    case: &Path,
    home: &Path,
    extra: &[String],
    root: &Path,
    report: &mut Value,
) -> io::Result<Value> {
    let upstream = &request.upstream;
    let (timeout, output_limit) = (request.timeout, request.output_limit);
    let stdout = root.join("rpc.jsonl");
    let stderr = root.join("stderr.txt");
    let (input, writer) = pipe()?;
    let mut command = CommandSpec::new(upstream);
    command.args = ["app-server", "--stdio"]
        .into_iter()
        .map(OsString::from)
        .chain(extra.iter().map(OsString::from))
        .collect();
    command.current_dir = Some(case.to_owned());
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.as_os_str().into()));
    command.stdin = Some(input);
    command.stdout = Some(create(&stdout)?);
    command.stderr = Some(create(&stderr)?);
    write_new(
        &root.join("request.json"),
        &json!({"executable":upstream,"arguments":command.args.iter().map(|s|s.to_string_lossy()).collect::<Vec<_>>(),"cwd":case,"codex_home":home,
        "memory_limit_bytes":512*1024*1024_u64,"timeout_seconds":timeout,"output_limit":output_limit}),
    )?;
    let cancellation = Cancellation::default();
    let deadline = Deadline::after(Duration::from_secs(timeout))?;
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
        std::thread::spawn(move || channel.discover(&case, &home, &root))
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
    let protocol = worker.join().map_err(|_| invalid("RPC worker panicked"));
    let outcome = outcome?;
    let output_exceeded =
        over_limit.load(Ordering::Relaxed) || exceeded(&[stdout, stderr], output_limit);
    let receipt = json!({"outcome":outcome,"assigned_before_resume":true,"process_id":identity.pid,"output_limit_reached":output_exceeded});
    write_new(&root.join("process.json"), &receipt)?;
    report["process"] = receipt;
    let response = protocol??;
    if output_exceeded {
        return Err(invalid("native discovery output limit"));
    }
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(invalid(
            "native discovery process did not exit successfully",
        ));
    }
    verify_complete_transcript(&root.join("rpc.jsonl"), &response, output_limit)?;
    Ok(response)
}

/// The last wanted reply may already be buffered when a contradictory response
/// or unsupported server request follows it. Inspect the complete retained
/// stream after owned shutdown, including records after config/read.
fn verify_complete_transcript(path: &Path, response: &Value, limit: u64) -> io::Result<()> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut total = 0_u64;
    let mut seen = 0_usize;
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
        if seen >= 3
            || row["id"] != (seen as u64 + 1)
            || row.get("method").is_some()
            || row.get("error").is_some()
            || row.get("result") != Some(&response[["native", "listed", "config"][seen]])
        {
            return Err(invalid(
                "contradictory or unexpected completed RPC response",
            ));
        }
        seen += 1;
    }
    if seen != 3 {
        return Err(invalid("incomplete completed RPC transcript"));
    }
    Ok(())
}
fn exceeded(paths: &[PathBuf], limit: u64) -> bool {
    paths
        .iter()
        .any(|p| fs::metadata(p).is_ok_and(|m| m.len() > limit))
}
