//! Machine-local shared CPU policy for the installation lifecycle.
//!
//! The record lives in the account CPU directory, outside tracked source and
//! outside every agent home. It is not a copy of `cpu-budget.json` (kernel
//! ownership) or the heavy-command budget (per-operation limits). Inspection,
//! preview and rollback never create a job, assign a process, or terminate
//! work. Measured consumption is never sampled here.

use crate::heavy_command::{self, Budget};
use crate::process::{CPU_BUDGET_ACCOUNT_ENV, SHARED_CPU_PERCENT};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs, io,
    io::Write,
    path::{Component, Path, PathBuf},
};

const POLICY_FILE: &str = "shared-cpu-policy.json";
const OWNERSHIP_RECORD: &str = "cpu-budget.json";
const ESCAPE_HATCH: &str = "CODEX_HARNESS_CPU_PERCENT";
const MEASURED: &str = "not-sampled";
const SCHEMA: u32 = 1;
const MAX_PROCESSES: usize = 8192;

#[derive(Debug, Serialize)]
pub struct CpuPolicyReport {
    pub action: &'static str,
    pub ceiling_percent: Option<f64>,
    pub escape_hatch: &'static str,
    pub policy_path: String,
    pub wrote_policy: bool,
    pub legacy_heavy_policy: String,
    pub ambiguous_shared_ceiling: bool,
    pub activation: &'static str,
    pub restart_boundary: String,
    pub kernel_configuration: String,
    pub measured_consumption: &'static str,
    pub model_calls: u32,
}

enum Mode {
    Preview,
    Establish,
    Inspect,
    Withdraw,
}

pub fn inspect(routes: &[PathBuf]) -> CpuPolicyReport {
    observe(routes, Mode::Inspect)
}

pub(super) fn preview(routes: &[PathBuf]) -> CpuPolicyReport {
    observe(routes, Mode::Preview)
}

pub(super) fn establish(routes: &[PathBuf]) -> CpuPolicyReport {
    observe(routes, Mode::Establish)
}

pub(super) fn withdraw(routes: &[PathBuf]) {
    emit_withdrawal(&observe(routes, Mode::Withdraw));
}

pub(super) fn exe_routes<I>(paths: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        })
        .collect()
}

fn observe(routes: &[PathBuf], mode: Mode) -> CpuPolicyReport {
    let legacy_heavy_policy = legacy_heavy_policy();
    let Some(directory) = policy_directory() else {
        return unavailable(&legacy_heavy_policy, &mode);
    };
    let path = directory.join(POLICY_FILE);
    let existing = match read_policy(&path) {
        Ok(existing) => existing,
        Err(error) => {
            return unavailable(
                &format!("{legacy_heavy_policy}; shared policy record preserved: {error}"),
                &mode,
            );
        }
    };
    let wrote_policy = if matches!(mode, Mode::Establish) && existing.is_none() {
        match create_default(&directory, &path) {
            Ok(created) => created,
            Err(error) => {
                return unavailable(
                    &format!(
                        "{legacy_heavy_policy}; shared policy record was not written: {error}"
                    ),
                    &mode,
                );
            }
        }
    } else {
        false
    };
    let current = if wrote_policy {
        match read_policy(&path) {
            Ok(policy) => policy,
            Err(error) => {
                return unavailable(
                    &format!("{legacy_heavy_policy}; shared policy record preserved: {error}"),
                    &mode,
                );
            }
        }
    } else {
        existing
    };
    let (ceiling, ambiguous_shared_ceiling) = match &current {
        Some(policy) => (policy.ceiling, policy.ambiguous),
        None => (None, false),
    };
    let retention = retained_routes(routes, &directory);
    let file_ready = current.as_ref().is_some_and(|policy| policy.usable);
    CpuPolicyReport {
        action: action(&mode, wrote_policy, current.is_some()),
        ceiling_percent: ceiling,
        escape_hatch: ESCAPE_HATCH,
        policy_path: path.display().to_string(),
        wrote_policy,
        legacy_heavy_policy,
        ambiguous_shared_ceiling,
        activation: activation(&mode, file_ready, &retention),
        restart_boundary: retention.boundary,
        kernel_configuration: kernel_configuration(&directory),
        measured_consumption: MEASURED,
        model_calls: 0,
    }
}

fn action(mode: &Mode, wrote: bool, present: bool) -> &'static str {
    match mode {
        Mode::Preview => "preview",
        Mode::Inspect => "inspected",
        Mode::Withdraw => "withdrawn",
        Mode::Establish if wrote => "established",
        Mode::Establish if present => "preserved",
        Mode::Establish => "unavailable",
    }
}

fn activation(mode: &Mode, file_ready: bool, retention: &Retention) -> &'static str {
    if matches!(mode, Mode::Withdraw) {
        return "withdrawn";
    }
    if retention.outside || retention.enumeration_incomplete || !file_ready {
        "incomplete"
    } else {
        "complete"
    }
}

fn unavailable(legacy_heavy_policy: &str, mode: &Mode) -> CpuPolicyReport {
    CpuPolicyReport {
        action: match mode {
            Mode::Withdraw => "withdrawn",
            Mode::Preview => "preview",
            _ => "unavailable",
        },
        ceiling_percent: None,
        escape_hatch: ESCAPE_HATCH,
        policy_path: String::new(),
        wrote_policy: false,
        legacy_heavy_policy: legacy_heavy_policy.to_owned(),
        ambiguous_shared_ceiling: false,
        activation: if matches!(mode, Mode::Withdraw) {
            "withdrawn"
        } else {
            "incomplete"
        },
        restart_boundary: "shared CPU policy storage was not changed; no process was terminated"
            .to_owned(),
        kernel_configuration:
            "kernel configuration was not opened; inspection did not create a job".to_owned(),
        measured_consumption: MEASURED,
        model_calls: 0,
    }
}

fn emit_withdrawal(report: &CpuPolicyReport) {
    // Unit tests compile this crate with `cfg(test)` and must not announce a
    // withdrawal against the live account. The installed command is not a test
    // build, so a real disconnect still reports the boundary.
    #[cfg(test)]
    if std::env::var_os(CPU_BUDGET_ACCOUNT_ENV)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return;
    }
    let preserved = if report.policy_path.is_empty() {
        "none"
    } else {
        report.policy_path.as_str()
    };
    eprintln!(
        "cpu-policy: default coverage is no longer provided by this installation; activation={}; restart boundary: {}; policy preserved at {preserved}; measured consumption {}; {}",
        report.activation,
        report.restart_boundary,
        report.measured_consumption,
        report.kernel_configuration
    );
}

fn policy_directory() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(CPU_BUDGET_ACCOUNT_ENV).filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(value);
        if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
            return None;
        }
        return Some(path);
    }
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        crate::process::cpu_budget_directory(None).ok()
    }
}

struct ParsedPolicy {
    ceiling: Option<f64>,
    usable: bool,
    ambiguous: bool,
}

fn read_policy(path: &Path) -> io::Result<Option<ParsedPolicy>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::other(
                "shared CPU policy record is a link; preserving it",
            ));
        }
        Ok(_) => {}
    }
    crate::build_identity::ordinary(path)?;
    Ok(Some(parse_policy(&fs::read(path)?)))
}

fn parse_policy(bytes: &[u8]) -> ParsedPolicy {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return ParsedPolicy {
            ceiling: None,
            usable: false,
            ambiguous: false,
        };
    };
    let schema_ok = value.get("schema").and_then(Value::as_u64) == Some(u64::from(SCHEMA));
    let ceiling = value
        .get("ceiling_percent")
        .and_then(Value::as_f64)
        .filter(|percent| percent.is_finite() && (0.01..=100.0).contains(percent));
    let ambiguous = ceiling.is_some_and(|percent| {
        heavy_command::legacy_default_cpu_percent(&Budget {
            cpu_percent: Some(percent),
            ..Budget::default()
        })
    });
    ParsedPolicy {
        ceiling,
        usable: schema_ok && ceiling.is_some(),
        ambiguous,
    }
}

fn create_default(directory: &Path, path: &Path) -> io::Result<bool> {
    if let Some(parent) = directory.parent()
        && parent.exists()
    {
        crate::build_identity::ordinary(parent)?;
    }
    if directory.exists() {
        crate::build_identity::ordinary(directory)?;
        if !directory.is_dir() {
            return Err(io::Error::other(
                "shared CPU policy account is not a directory; preserving it",
            ));
        }
    } else {
        fs::create_dir_all(directory)?;
        crate::build_identity::ordinary(directory)?;
    }
    let body = serde_json::to_vec_pretty(&json!({
        "schema": SCHEMA,
        "ceiling_percent": SHARED_CPU_PERCENT,
        "escape_hatch": ESCAPE_HATCH,
        "note": "Installed account ceiling in percent of total host CPU. CODEX_HARNESS_CPU_PERCENT is the explicit per-launch escape hatch and is not a second policy record."
    }))?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            if let Err(error) = file.write_all(&body) {
                drop(file);
                let _ = fs::remove_file(path);
                return Err(error);
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

fn legacy_heavy_policy() -> String {
    let Ok(account) = heavy_command::account_dir(None) else {
        return "heavy-command policy was not read or changed".to_owned();
    };
    match Budget::read(&account) {
        Ok(budget) => heavy_command::cpu_policy_summary(&budget),
        Err(error) => format!("heavy-command policy was preserved and not changed: {error}"),
    }
}

fn kernel_configuration(directory: &Path) -> String {
    let record = directory.join(OWNERSHIP_RECORD);
    let Ok(bytes) = fs::read(&record) else {
        return "kernel configuration was not opened: no ownership record; inspection did not create a job"
            .to_owned();
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return "kernel configuration was not opened: ownership record is unreadable and was preserved"
            .to_owned();
    };
    let Some(job) = value.get("job").and_then(Value::as_str) else {
        return "kernel configuration was not opened: ownership record has no job name and was preserved"
            .to_owned();
    };
    match query_job_rate(job) {
        Ok((rate, hard_cap)) => format!(
            "kernel configuration readback: job {job} cpu_rate {rate} hard_cap {hard_cap}; this is configuration, not measured consumption"
        ),
        Err(error) => format!(
            "kernel configuration was not opened for job {job}: {error}; inspection did not create a job"
        ),
    }
}

struct Retention {
    boundary: String,
    outside: bool,
    enumeration_incomplete: bool,
}

fn retained_routes(routes: &[PathBuf], directory: &Path) -> Retention {
    if routes.is_empty() {
        return Retention {
            boundary: "no ordinary route is registered; no process was terminated".to_owned(),
            outside: false,
            enumeration_incomplete: false,
        };
    }
    let keys = route_keys(routes);
    let ids = match process_ids() {
        Ok(ids) => ids,
        Err(error) => {
            return Retention {
                boundary: format!(
                    "process enumeration did not complete ({error}); activation stays incomplete and no process was terminated"
                ),
                outside: true,
                enumeration_incomplete: true,
            };
        }
    };
    let job = recorded_job(directory);
    let mut lines = Vec::new();
    for pid in ids {
        let Ok(image) = image_path(pid) else {
            continue;
        };
        let image_keys = image_keys(&image);
        if !image_keys.iter().any(|key| keys.contains(key)) {
            continue;
        }
        if job
            .as_deref()
            .is_some_and(|name| in_named_job(pid, name).unwrap_or(false))
        {
            continue;
        }
        let membership = if job.is_some() {
            "not a member of the open account job"
        } else {
            "membership unverified because the account job was not opened"
        };
        lines.push(format!(
            "pid {pid} image {} is {membership}; restart that process after its work can stop; installation does not terminate it",
            image.display()
        ));
    }
    if lines.is_empty() {
        Retention {
            boundary: "no retained ordinary route is running; no restart is required and no process was terminated"
                .to_owned(),
            outside: false,
            enumeration_incomplete: false,
        }
    } else {
        Retention {
            boundary: lines.join("; "),
            outside: true,
            enumeration_incomplete: false,
        }
    }
}

fn recorded_job(directory: &Path) -> Option<String> {
    let bytes = fs::read(directory.join(OWNERSHIP_RECORD)).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let job = value.get("job")?.as_str()?;
    query_job_rate(job).ok().map(|_| job.to_owned())
}

fn route_keys(routes: &[PathBuf]) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for route in routes {
        keys.insert(normalize(route));
        if let Ok(target) = fs::read_link(route) {
            keys.insert(normalize(&target));
        }
        if let Ok(canonical) = route.canonicalize() {
            keys.insert(normalize(&canonical));
        }
    }
    keys
}

fn image_keys(image: &Path) -> Vec<String> {
    let mut keys = vec![normalize(image)];
    if let Ok(canonical) = image.canonicalize() {
        keys.push(normalize(&canonical));
    }
    keys
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_start_matches(r"\\?\")
        .to_ascii_lowercase()
}

fn process_ids() -> io::Result<Vec<u32>> {
    use std::mem::size_of;
    use windows_sys::Win32::System::ProcessStatus::K32EnumProcesses;
    let mut capacity = 1024usize;
    loop {
        let mut ids = vec![0u32; capacity];
        let bytes = (ids.len() * size_of::<u32>()) as u32;
        let mut used = 0u32;
        // SAFETY: `ids` is a writable buffer of `bytes` bytes for this call.
        if unsafe { K32EnumProcesses(ids.as_mut_ptr(), bytes, &mut used) } == 0
            || used > bytes
            || !used.is_multiple_of(size_of::<u32>() as u32)
        {
            return Err(io::Error::other(
                "process enumeration failed; no process was terminated",
            ));
        }
        if used == bytes {
            if capacity == MAX_PROCESSES {
                return Err(io::Error::other(
                    "process enumeration exceeded its bound; no process was terminated",
                ));
            }
            capacity = capacity.saturating_mul(2).min(MAX_PROCESSES);
            continue;
        }
        ids.truncate(used as usize / size_of::<u32>());
        ids.retain(|pid| *pid != 0);
        return Ok(ids);
    }
}

fn image_path(pid: u32) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW};
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    // SAFETY: a successful open returns an owned process handle; failure is null.
    let handle = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
    let mut buffer = vec![0u16; 1024];
    let mut length = buffer.len() as u32;
    // SAFETY: `buffer` is writable for `length` wide characters and `handle` is live.
    let ok = unsafe {
        QueryFullProcessImageNameW(handle.as_raw_handle(), 0, buffer.as_mut_ptr(), &mut length)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(length as usize);
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer)))
}

fn query_job_rate(name: &str) -> io::Result<(u32, bool)> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
        JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JobObjectCpuRateControlInformation, OpenJobObjectW,
        QueryInformationJobObject,
    };
    const JOB_OBJECT_QUERY: u32 = 0x0004;
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated; a successful open returns an owned handle.
    let handle = owned(unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, wide.as_ptr()) })?;
    // SAFETY: the query writes one rate-control structure into `cpu`.
    let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = unsafe { zeroed() };
    let ok = unsafe {
        QueryInformationJobObject(
            handle.as_raw_handle(),
            JobObjectCpuRateControlInformation,
            (&mut cpu as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
            size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let enabled = cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_ENABLE != 0;
    let hard_cap = cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP != 0;
    let rate = if enabled {
        // SAFETY: the enable flag selects the CpuRate union member.
        unsafe { cpu.Anonymous.CpuRate }
    } else {
        0
    };
    Ok((rate, hard_cap))
}

fn in_named_job(pid: u32, name: &str) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{IsProcessInJob, OpenJobObjectW};
    use windows_sys::Win32::System::Threading::OpenProcess;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const JOB_OBJECT_QUERY: u32 = 0x0004;
    // SAFETY: successful opens return owned handles for this query only.
    let process = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let job = owned(unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, wide.as_ptr()) })?;
    let mut member = 0i32;
    // SAFETY: both handles are live query handles for this call.
    let ok = unsafe { IsProcessInJob(process.as_raw_handle(), job.as_raw_handle(), &mut member) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(member != 0)
}

fn owned(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` came from a successful native open and is not a pseudo-handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}
