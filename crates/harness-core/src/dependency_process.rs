//! Explicit, read-only observation of dependency consumers on the real host.
//!
//! Only `active_consumers` is replaced. No command, shell, package, environment
//! reader or network operation is invoked. A process observation is advisory;
//! it never grants update/termination authority or changes `update_safe`.
//!
//! Windows-sys 0.61 additionally needs Win32_System_ProcessStatus,
//! Win32_System_Diagnostics_Debug, Win32_System_LibraryLoader and
//! Win32_System_Kernel. Integration owns the manifest and opt-in CLI switch.

use serde_json::{Value, json};
use std::io;

const UNAVAILABLE: &str =
    "Host process inspection is unavailable; no absence of consumers is established.";
const INCOMPLETE: &str = "Some required paths or live process identities could not be verified; no absence of consumers is established.";

/// Inspect this Windows host, independently of the dependency home/PATH policy.
///
/// OS failures are withheld and represented in each record, including partial
/// positive observations. `Err` is reserved for invalid record containers and
/// contains only fixed text. `observed` means a bounded observation, not a lock
/// against processes starting or changing immediately afterwards.
pub fn observe(records: &mut [Value]) -> io::Result<()> {
    if records.iter().any(|record| !record.is_object()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Dependency process records must be objects.",
        ));
    }
    if records.is_empty() {
        return Ok(());
    }
    #[cfg(windows)]
    windows::observe(records);
    #[cfg(not(windows))]
    for record in records {
        record["active_consumers"] = report(Vec::new(), "unavailable");
    }
    Ok(())
}

fn report(processes: Vec<Value>, state: &str) -> Value {
    let mut value = json!({"state": state, "processes": processes});
    match state {
        "unavailable" => value["reason"] = json!(UNAVAILABLE),
        "incomplete" => value["reason"] = json!(INCOMPLETE),
        _ => {}
    }
    value
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::mem::{offset_of, size_of};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::{Component, Path, PathBuf};
    use std::ptr::{read_volatile, write_volatile};
    use std::sync::atomic::{Ordering, compiler_fence};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use windows_sys::Win32::Foundation::{FILETIME, HANDLE, UNICODE_STRING, WAIT_TIMEOUT};
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Environment::GetCommandLineW;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::ProcessStatus::K32EnumProcesses;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessId, GetProcessTimes, OpenProcess, PEB,
        PROCESS_BASIC_INFORMATION, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, PROCESS_VM_READ, QueryFullProcessImageNameW,
        RTL_USER_PROCESS_PARAMETERS, WaitForSingleObject,
    };

    const MAX_PROCESSES: usize = 65_536;
    const MAX_WIDE: usize = 32_768;
    const SCAN_LIMIT: Duration = Duration::from_secs(20);

    // No Debug/Serialize implementations: command lines must never cross the
    // observation boundary, including on failures or test assertions.
    struct PrivateCommand(Vec<u16>);

    impl Drop for PrivateCommand {
        fn drop(&mut self) {
            for unit in &mut self.0 {
                // Make the overwrite observable to the optimizer as well.
                unsafe { write_volatile(unit, 0) };
            }
            compiler_fence(Ordering::SeqCst);
        }
    }

    fn equal(left: &[u16], right: &[u16]) -> bool {
        left.len() == right.len()
            && left.len() <= i32::MAX as usize
            && (left.is_empty()
                || unsafe {
                    CompareStringOrdinal(
                        left.as_ptr(),
                        left.len() as i32,
                        right.as_ptr(),
                        right.len() as i32,
                        1,
                    ) == CSTR_EQUAL
                })
    }

    // Pure lexical normalization: never resolve a symlink, consult PATH or touch
    // an UNC server. In particular, aliases of a shared Python/node executable
    // do not identify the venv/package which is using that interpreter.
    fn normalized(value: &str) -> Option<Vec<u16>> {
        if value.is_empty() || value.chars().any(|c| c.is_control() || c == '"') {
            return None;
        }
        let text = value.replace('/', "\\");
        let text = if text
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case(r"\\?\UNC\"))
        {
            format!(r"\\{}", &text[8..])
        } else {
            text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
        };
        if text.starts_with(r"\\.\") || !Path::new(&text).is_absolute() {
            return None;
        }
        let mut path = PathBuf::new();
        for component in Path::new(&text).components() {
            match component {
                Component::ParentDir => {
                    if !matches!(path.components().next_back(), Some(Component::Normal(_))) {
                        return None;
                    }
                    path.pop();
                }
                Component::CurDir => {}
                other => path.push(other.as_os_str()),
            }
        }
        use std::os::windows::ffi::OsStrExt;
        let mut units: Vec<_> = path.as_os_str().encode_wide().collect();
        while units.last() == Some(&92) {
            units.pop();
        }
        (units.len() < MAX_WIDE).then_some(units)
    }

    fn within(path: &[u16], root: &[u16]) -> bool {
        path.len() > root.len() && path[root.len()] == 92 && equal(&path[..root.len()], root)
    }

    struct Needle {
        path: Vec<u16>,
        directory: bool,
    }

    struct Target {
        needles: Vec<Needle>,
        complete: bool,
    }

    impl Target {
        fn new(record: &Value) -> Self {
            let root = record["installation_root"].as_str().and_then(normalized);
            let mut target = Self {
                complete: root.is_some(),
                needles: root
                    .into_iter()
                    .map(|path| Needle {
                        path,
                        directory: true,
                    })
                    .collect(),
            };
            // A native companion package can be a sibling of installation_root.
            // Match only its named payload, never broaden to its parent directory.
            for key in ["native_executable", "original_native_executable"] {
                let value = &record["paths"][key];
                if value.is_null() {
                    continue;
                }
                if let Some(path) = value.as_str().and_then(normalized) {
                    target.needles.push(Needle {
                        path,
                        directory: false,
                    });
                } else {
                    target.complete = false;
                }
            }
            // command[0] and paths.python may denote shared interpreters. Their
            // presence alone cannot attribute a process to this installation.
            target
        }

        fn executable_matches(&self, executable: &[u16]) -> bool {
            self.needles.iter().any(|needle| {
                if needle.directory {
                    within(executable, &needle.path)
                } else {
                    equal(executable, &needle.path)
                }
            })
        }

        fn argument_matches(&self, command: &[u16]) -> bool {
            self.needles.iter().any(|needle| {
                argument_match(command, &needle.path, needle.directory) || {
                    // Match Win32 extended path spelling without conflating
                    // unrelated filesystem aliases or sibling versions.
                    let prefix = if needle.path.starts_with(&[92, 92]) {
                        r"\\?\UNC\"
                    } else {
                        r"\\?\"
                    };
                    let skip = if needle.path.starts_with(&[92, 92]) {
                        2
                    } else {
                        0
                    };
                    let extended: Vec<_> = prefix
                        .encode_utf16()
                        .chain(needle.path[skip..].iter().copied())
                        .collect();
                    argument_match(command, &extended, needle.directory)
                }
            })
        }
    }

    fn whitespace(unit: u16) -> bool {
        char::from_u32(u32::from(unit)).is_some_and(char::is_whitespace)
    }

    fn argument_match(command: &[u16], path: &[u16], directory: bool) -> bool {
        if path.is_empty() || command.len() < path.len() {
            return false;
        }
        (0..=command.len() - path.len()).any(|start| {
            let end = start + path.len();
            (start == 0 || matches!(command[start - 1], 34 | 61) || whitespace(command[start - 1]))
                && (end == command.len()
                    || command[end] == 34
                    || whitespace(command[end])
                    || (directory && command[end] == 92))
                && equal(&command[start..end], path)
        })
    }

    fn pids() -> Result<Vec<u32>, ()> {
        let mut capacity = 1024;
        loop {
            let mut ids = vec![0; capacity];
            let bytes = (ids.len() * size_of::<u32>()) as u32;
            let mut used = 0;
            if unsafe { K32EnumProcesses(ids.as_mut_ptr(), bytes, &mut used) } == 0
                || used > bytes
                || !used.is_multiple_of(size_of::<u32>() as u32)
            {
                return Err(());
            }
            // Equal sizes do NOT establish exhaustive enumeration (PSAPI docs).
            if used == bytes {
                if capacity == MAX_PROCESSES {
                    return Err(());
                }
                capacity *= 2;
                continue;
            }
            ids.truncate(used as usize / size_of::<u32>());
            // PID 0 is the system idle pseudo-process, not a user executable.
            // Other unreadable system/protected processes remain incomplete.
            ids.retain(|pid| *pid != 0);
            ids.sort_unstable();
            ids.dedup();
            return if ids.is_empty() { Err(()) } else { Ok(ids) };
        }
    }

    fn open(pid: u32) -> Option<OwnedHandle> {
        for access in [
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
        ] {
            let handle = unsafe { OpenProcess(access, 0, pid) };
            if !handle.is_null() {
                // As in process.rs, only newly acquired real handles are owned.
                return Some(unsafe { OwnedHandle::from_raw_handle(handle) });
            }
        }
        None
    }

    fn creation_time(handle: HANDLE) -> Option<u64> {
        let (mut creation, mut exit, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0
        {
            return None;
        }
        let ticks = u64::from(creation.dwHighDateTime) << 32 | u64::from(creation.dwLowDateTime);
        (ticks != 0).then_some(ticks)
    }

    fn live_identity(handle: HANDLE, pid: u32, created: u64) -> bool {
        (unsafe { GetProcessId(handle) == pid && WaitForSingleObject(handle, 0) == WAIT_TIMEOUT })
            && creation_time(handle) == Some(created)
    }

    fn image(handle: HANDLE) -> Option<String> {
        let mut path = vec![0u16; MAX_WIDE];
        let mut used = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut used) } == 0
            || used == 0
            || used as usize >= path.len()
        {
            return None;
        }
        String::from_utf16(&path[..used as usize]).ok()
    }

    // Official contracts, checked against SDK 10.0.26100.0/winternl.h:
    // https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntqueryinformationprocess
    // https://learn.microsoft.com/en-us/windows/win32/api/winternl/ns-winternl-peb
    // https://learn.microsoft.com/en-us/windows/win32/api/winternl/ns-winternl-rtl_user_process_parameters
    // https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-readprocessmemory
    // These structures are version-sensitive. Dynamically resolve class 0 and
    // validate the layout against GetCommandLineW on this actual process before
    // relying on it. No undocumented information class or cross-bitness offsets.
    type NtQuery = unsafe extern "system" fn(HANDLE, i32, *mut c_void, u32, *mut u32) -> i32;
    type MachineQuery = unsafe extern "system" fn(HANDLE, *mut u16, *mut u16) -> i32;

    struct CommandReader {
        query: NtQuery,
        machine: MachineQuery,
    }

    impl CommandReader {
        fn new() -> Option<Self> {
            if !cfg!(target_arch = "x86_64") {
                return None;
            }
            // Both modules are already loaded by the Windows process runtime;
            // no library is loaded through PATH or from a dependency home.
            let reader = unsafe {
                let ntdll = GetModuleHandleW(windows_sys::core::w!("ntdll.dll"));
                let kernel = GetModuleHandleW(windows_sys::core::w!("kernel32.dll"));
                if ntdll.is_null() || kernel.is_null() {
                    return None;
                }
                Self {
                    query: std::mem::transmute::<unsafe extern "system" fn() -> isize, NtQuery>(
                        GetProcAddress(ntdll, c"NtQueryInformationProcess".as_ptr().cast())?,
                    ),
                    machine: std::mem::transmute::<
                        unsafe extern "system" fn() -> isize,
                        MachineQuery,
                    >(GetProcAddress(
                        kernel,
                        c"IsWow64Process2".as_ptr().cast(),
                    )?),
                }
            };
            let actual = reader.read(unsafe { GetCurrentProcess() })?;
            let local = unsafe { GetCommandLineW() };
            if local.is_null() || actual.0.is_empty() {
                return None;
            }
            // GetCommandLineW guarantees a terminated string owned by Windows.
            // Do not format either side when checking the native layout.
            for (index, unit) in actual.0.iter().enumerate() {
                if unsafe { read_volatile(local.add(index)) } != *unit {
                    return None;
                }
            }
            if unsafe { read_volatile(local.add(actual.0.len())) } != 0 {
                return None;
            }
            Some(reader)
        }

        fn read(&self, handle: HANDLE) -> Option<PrivateCommand> {
            let (mut process_machine, mut native_machine) = (0, 0);
            if unsafe { (self.machine)(handle, &mut process_machine, &mut native_machine) } == 0
                || process_machine != 0
                || native_machine != 0x8664
            // IMAGE_FILE_MACHINE_AMD64, winnt.h
            {
                return None;
            }
            let mut basic = PROCESS_BASIC_INFORMATION::default();
            let mut used = 0;
            if unsafe {
                (self.query)(
                    handle,
                    0,
                    (&mut basic as *mut PROCESS_BASIC_INFORMATION).cast(),
                    size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                    &mut used,
                )
            } != 0
                || used as usize != size_of::<PROCESS_BASIC_INFORMATION>()
                || basic.PebBaseAddress.is_null()
                || basic.UniqueProcessId != unsafe { GetProcessId(handle) } as usize
            {
                return None;
            }
            let pointer_address =
                (basic.PebBaseAddress as usize).checked_add(offset_of!(PEB, ProcessParameters))?;
            let mut parameters = 0usize;
            read_value(handle, pointer_address, &mut parameters)?;
            let descriptor_address =
                parameters.checked_add(offset_of!(RTL_USER_PROCESS_PARAMETERS, CommandLine))?;
            let mut descriptor = UNICODE_STRING::default();
            read_value(handle, descriptor_address, &mut descriptor)?;
            let first = read_command(handle, &descriptor)?;
            let second = read_command(handle, &descriptor)?;
            let mut after = UNICODE_STRING::default();
            let mut parameters_after = 0usize;
            read_value(handle, descriptor_address, &mut after)?;
            read_value(handle, pointer_address, &mut parameters_after)?;
            if parameters == 0
                || parameters != parameters_after
                || descriptor.Length != after.Length
                || descriptor.MaximumLength != after.MaximumLength
                || descriptor.Buffer != after.Buffer
                || first.0 != second.0
            {
                return None;
            }
            Some(first)
        }
    }

    // Only used with initialized integer/pointer-only SDK fields. Remote
    // pointers are never dereferenced locally and partial reads are rejected.
    fn read_value<T>(handle: HANDLE, address: usize, value: &mut T) -> Option<()> {
        read_memory(handle, address, (value as *mut T).cast(), size_of::<T>())
    }

    fn read_memory(
        handle: HANDLE,
        address: usize,
        output: *mut c_void,
        bytes: usize,
    ) -> Option<()> {
        if address == 0 || address.checked_add(bytes).is_none() {
            return None;
        }
        let mut read = 0;
        (unsafe { ReadProcessMemory(handle, address as *const c_void, output, bytes, &mut read) }
            != 0
            && read == bytes)
            .then_some(())
    }

    fn read_command(handle: HANDLE, descriptor: &UNICODE_STRING) -> Option<PrivateCommand> {
        let bytes = usize::from(descriptor.Length);
        if bytes == 0
            || !bytes.is_multiple_of(2)
            || !descriptor.MaximumLength.is_multiple_of(2)
            || descriptor.Length > descriptor.MaximumLength
            || bytes / 2 >= MAX_WIDE
            || !(descriptor.Buffer as usize).is_multiple_of(2)
        {
            return None;
        }
        let mut command = PrivateCommand(vec![0; bytes / 2]);
        read_memory(
            handle,
            descriptor.Buffer as usize,
            command.0.as_mut_ptr().cast(),
            bytes,
        )?;
        if command.0.contains(&0) {
            return None;
        }
        Some(command)
    }

    struct Sample {
        handle: OwnedHandle,
        pid: u32,
        created: u64,
        executable: String,
        matches: Vec<bool>,
        complete: bool,
    }

    fn sample(pid: u32, targets: &[Target], reader: Option<&CommandReader>) -> Option<Sample> {
        let handle = open(pid)?;
        let raw = handle.as_raw_handle();
        let created = creation_time(raw)?;
        if !live_identity(raw, pid, created) {
            return None;
        }
        let executable = image(raw)?;
        let path = normalized(&executable)?;
        let mut command = reader.and_then(|reader| reader.read(raw));
        if let Some(command) = &mut command {
            for unit in &mut command.0 {
                if *unit == 47 {
                    *unit = 92;
                }
            }
        }
        let matches = targets
            .iter()
            .map(|target| {
                target.executable_matches(&path)
                    || command
                        .as_ref()
                        .is_some_and(|command| target.argument_matches(&command.0))
            })
            .collect();
        Some(Sample {
            handle,
            pid,
            created,
            executable,
            matches,
            complete: command.is_some(),
        })
    }

    fn finish(records: &mut [Value], targets: &[Target], samples: &[Sample], complete: bool) {
        for (index, (record, target)) in records.iter_mut().zip(targets).enumerate() {
            let processes = samples
                .iter()
                .filter(|sample| sample.matches[index])
                .map(|sample| {
                    json!({
                        "pid": sample.pid, "executable": sample.executable,
                        // Same FILETIME identity (100 ns since 1601) as process.rs.
                        "creation_time": sample.created, "evidence": "installation-path-match",
                    })
                })
                .collect();
            record["active_consumers"] = report(
                processes,
                if complete && target.complete {
                    "observed"
                } else {
                    "incomplete"
                },
            );
        }
    }

    pub(super) fn observe(records: &mut [Value]) {
        let targets: Vec<_> = records.iter().map(Target::new).collect();
        let started = Instant::now();
        let start_ticks = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| {
                u64::try_from(duration.as_nanos() / 100)
                    .ok()?
                    .checked_add(116_444_736_000_000_000)
            });
        let Ok(initial) = pids() else {
            for record in records {
                record["active_consumers"] = report(Vec::new(), "unavailable");
            }
            return;
        };
        let reader = CommandReader::new();
        let mut complete = start_ticks.is_some();
        let mut samples = Vec::new();
        for pid in &initial {
            if started.elapsed() >= SCAN_LIMIT {
                complete = false;
                break;
            }
            match sample(*pid, &targets, reader.as_ref()) {
                Some(sample) => {
                    // An enumerated PID can be reused before OpenProcess. Never
                    // attribute that replacement to the initial observation.
                    if start_ticks.is_none_or(|start| sample.created >= start) {
                        complete = false;
                    } else {
                        complete &= sample.complete;
                        samples.push(sample);
                    }
                }
                None => complete = false,
            }
        }
        complete &= pids().is_ok_and(|final_ids| final_ids == initial);
        // Keep handles through the final check, so PID-only reopening cannot
        // replace an earlier identity. Exited/unreadable samples are not emitted.
        samples.retain(|sample| {
            let live = live_identity(sample.handle.as_raw_handle(), sample.pid, sample.created);
            complete &= live;
            live
        });
        complete &= started.elapsed() < SCAN_LIMIT;
        finish(records, &targets, &samples, complete);
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ptr::null_mut;

        fn target(root: &str) -> Target {
            Target::new(&json!({"installation_root": root}))
        }
        fn command(value: &str) -> PrivateCommand {
            PrivateCommand(value.replace('/', "\\").encode_utf16().collect())
        }

        #[test]
        fn paths_require_boundaries_and_preserve_case_and_version_distinctions() {
            let target = target(r"C:\Tools\Server 1");
            for text in [
                r#"python.exe "c:/TOOLS/Server 1/main.py" --token=PRIVATE"#,
                r#"python.exe --root="C:\Tools\Server 1""#,
                r"python.exe --root=C:\Tools\Server 1\main.py",
                r#"python.exe "\\?\C:\Tools\Server 1\main.py""#,
            ] {
                assert!(target.argument_matches(&command(text).0));
            }
            for text in [
                r"python.exe C:\Tools\Server 10\main.py",
                r"python.exe otherC:\Tools\Server 1\main.py",
                r"python.exe --root=C:\Tools\Server 1-other\main.py",
                r"python.exe C:\Tools\Server 2\main.py",
            ] {
                assert!(!target.argument_matches(&command(text).0));
            }
            assert!(
                target
                    .executable_matches(&normalized(r"c:/TOOLS/Server 1/bin/server.exe").unwrap())
            );
            assert!(
                !target.executable_matches(&normalized(r"C:\Tools\Server 10\server.exe").unwrap())
            );
        }

        #[test]
        fn exact_companion_payloads_do_not_broaden_to_siblings_or_shared_interpreters() {
            let target = Target::new(&json!({
                "installation_root": r"C:\pkg\wrapper", "command": [r"C:\shared\python.exe"],
                "paths": {"python": r"C:\shared\python.exe",
                    "native_executable": r"C:\pkg\platform\server-fixed.exe",
                    "original_native_executable": r"C:\pkg\platform\server.exe"}
            }));
            for path in [
                r"C:\pkg\platform\server-fixed.exe",
                r"c:\PKG\platform\server.exe",
            ] {
                assert!(target.executable_matches(&normalized(path).unwrap()));
                assert!(
                    target.argument_matches(&command(&format!("runner.exe --payload={path}")).0)
                );
            }
            for path in [
                r"C:\shared\python.exe",
                r"C:\pkg\platform\other.exe",
                r"C:\pkg\platform\server.exe-old",
                r"C:\pkg\platform2\server.exe",
            ] {
                assert!(!target.executable_matches(&normalized(path).unwrap()));
                assert!(!target.argument_matches(&command(path).0));
            }
        }

        #[test]
        fn path_normalization_is_lexical_and_retains_alias_distinctions() {
            assert!(equal(
                &normalized(r"\\?\UNC\server\share\tool\").unwrap(),
                &normalized(r"\\server\SHARE\tool").unwrap()
            ));
            assert!(equal(
                &normalized(r"C:\tool\bin\..\server.exe").unwrap(),
                &normalized(r"c:/tool/server.exe").unwrap()
            ));
            assert!(
                target(r"C:\Инструменты\Сервер")
                    .argument_matches(&command(r"python.exe c:/ИНСТРУМЕНТЫ/сервер/main.py").0)
            );
            assert!(
                target(r"\\server\share\tool").argument_matches(
                    &command(r#"python.exe "\\?\UNC\SERVER\share\tool\main.py""#).0
                )
            );
            for path in [
                "",
                "relative",
                r"C:relative",
                r"C:\..\escape",
                r"\\.\pipe\name",
                "C:\\bad\nname",
            ] {
                assert!(normalized(path).is_none());
            }
            // No disk access or broadening to a shared physical file identity.
            assert!(
                !target(r"C:\alias-a")
                    .executable_matches(&normalized(r"C:\alias-b\server.exe").unwrap())
            );
        }

        #[test]
        fn incomplete_and_unavailable_never_become_clean_negatives() {
            let mut records = vec![json!({"installation_root": r"C:\valid"}), json!({})];
            let targets: Vec<_> = records.iter().map(Target::new).collect();
            finish(&mut records, &targets, &[], false);
            assert!(
                records
                    .iter()
                    .all(|r| r["active_consumers"]["state"] == "incomplete")
            );
            finish(&mut records, &targets, &[], true);
            assert_eq!(records[0]["active_consumers"]["state"], "observed");
            assert_eq!(records[1]["active_consumers"]["state"], "incomplete");
            assert!(report(Vec::new(), "unavailable")["reason"].is_string());
        }

        #[test]
        fn invalid_record_is_a_fixed_error_without_partial_mutation() {
            let mut records = vec![json!({"active_consumers": "keep"}), json!("SECRET")];
            let before = records.clone();
            let error = super::super::observe(&mut records).unwrap_err();
            assert_eq!(records, before);
            assert_eq!(
                error.to_string(),
                "Dependency process records must be objects."
            );
        }

        #[test]
        fn native_command_reader_checks_actual_owned_process_without_disclosing_arguments() {
            let reader =
                CommandReader::new().expect("native command-line layout unavailable on this host");
            let private = reader.read(unsafe { GetCurrentProcess() });
            assert!(private.is_some());
            // Pointer, length and partial-read counterexamples never return data.
            assert!(
                read_command(unsafe { GetCurrentProcess() }, &UNICODE_STRING::default()).is_none()
            );
            let invalid = UNICODE_STRING {
                Length: 3,
                MaximumLength: 4,
                Buffer: null_mut(),
            };
            assert!(read_command(unsafe { GetCurrentProcess() }, &invalid).is_none());
        }

        #[test]
        fn owned_identity_mismatch_is_rejected() {
            let pid = std::process::id();
            let handle = open(pid).unwrap();
            let created = creation_time(handle.as_raw_handle()).unwrap();
            assert!(live_identity(handle.as_raw_handle(), pid, created));
            assert!(!live_identity(handle.as_raw_handle(), pid, created + 1));
            assert!(!live_identity(
                handle.as_raw_handle(),
                pid.wrapping_add(1),
                created
            ));
        }

        #[test]
        fn unreadable_command_scope_retains_only_verified_positive_identity() {
            let pid = std::process::id();
            let reader = CommandReader::new().unwrap();
            let raw = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            assert!(!raw.is_null());
            let limited = unsafe { OwnedHandle::from_raw_handle(raw) };
            // The object is live, but this handle deliberately lacks VM_READ.
            assert!(creation_time(limited.as_raw_handle()).is_some());
            assert!(reader.read(limited.as_raw_handle()).is_none());
            assert!(sample(u32::MAX, &[], Some(&reader)).is_none());

            let executable = std::env::current_exe().unwrap();
            let mut records = vec![json!({"installation_root": executable.parent().unwrap()})];
            let targets: Vec<_> = records.iter().map(Target::new).collect();
            let partial = sample(pid, &targets, None).unwrap();
            assert!(!partial.complete);
            finish(&mut records, &targets, &[partial], false);
            assert_eq!(records[0]["active_consumers"]["state"], "incomplete");
            assert_eq!(records[0]["active_consumers"]["processes"][0]["pid"], pid);
        }

        #[test]
        fn explicit_observe_sees_owned_host_process_with_alternate_dependency_roots() {
            let executable = std::env::current_exe().unwrap();
            let mut records = vec![
                json!({"installation_root": executable.parent().unwrap(), "update_safe": false}),
                json!({"installation_root": format!(r"C:\harness-absent-dependency-home-{}", std::process::id())}),
            ];
            super::super::observe(&mut records).unwrap();
            let process = records[0]["active_consumers"]["processes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["pid"] == std::process::id())
                .expect("owned process must be present");
            let keys: Vec<_> = process
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ["creation_time", "evidence", "executable", "pid"]);
            assert!(process["creation_time"].as_u64().unwrap() > 0);
            assert_eq!(records[0]["update_safe"], false);
            assert!(
                records[1]["active_consumers"]["processes"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }

        // Explicit owned subprocess acceptance; ordinary tests launch nothing.
        // Run this exact ignored test after building the integrated library.
        #[test]
        #[ignore = "starts one owned test child; run explicitly for argument/privacy acceptance"]
        fn owned_child_argument_privacy_acceptance() {
            use std::io::{BufRead, BufReader, Write};
            use std::process::{Command, Stdio};
            struct ChildGuard(std::process::Child);
            impl Drop for ChildGuard {
                fn drop(&mut self) {
                    // The retained Child handle is the only cleanup authority.
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
            let root = format!(
                r"C:\harness-consumer-fixture-{}\version 1",
                std::process::id()
            );
            let mut child = ChildGuard(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "dependency_process::windows::tests::consumer_fixture",
                        "--ignored",
                        "--nocapture",
                    ])
                    .args(["--skip", &root, "--skip", "PRIVATE-COMMAND-TOKEN-91d7"])
                    .env("HARNESS_DEPENDENCY_PROCESS_FIXTURE", "1")
                    .env(
                        "HARNESS_DEPENDENCY_PROCESS_PRIVATE",
                        "PRIVATE-ENVIRONMENT-TOKEN-4fa2",
                    )
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let mut output = BufReader::new(child.0.stdout.take().unwrap());
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                let mut line = String::new();
                while output.read_line(&mut line).is_ok_and(|count| count > 0) {
                    if line.contains("dependency-consumer-ready") {
                        let _ = ready_tx.send(());
                    }
                    line.clear();
                }
            });
            ready_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("owned fixture did not become ready");
            let mut records = vec![
                json!({"installation_root": root}),
                json!({"installation_root": format!("{root}0")}),
            ];
            super::super::observe(&mut records).unwrap();
            assert!(
                records[0]["active_consumers"]["processes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|process| process["pid"] == child.0.id())
            );
            assert!(
                !records[1]["active_consumers"]["processes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|process| process["pid"] == child.0.id())
            );
            let report = serde_json::to_string(&records).unwrap();
            for private in [
                "PRIVATE-COMMAND-TOKEN-91d7",
                "PRIVATE-ENVIRONMENT-TOKEN-4fa2",
                "CommandLine",
                "command_line",
            ] {
                assert!(!report.contains(private));
            }
            child.0.stdin.take().unwrap().write_all(b"q").unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                assert!(Instant::now() < deadline, "owned fixture did not exit");
                std::thread::sleep(Duration::from_millis(10));
            }
            reader.join().unwrap();
        }

        #[test]
        #[ignore = "private child entrypoint; invoked only by owned_child_argument_privacy_acceptance"]
        fn consumer_fixture() {
            use std::io::{Read, Write};
            if std::env::var_os("HARNESS_DEPENDENCY_PROCESS_FIXTURE").as_deref()
                != Some(std::ffi::OsStr::new("1"))
            {
                return;
            }
            println!("dependency-consumer-ready");
            std::io::stdout().flush().unwrap();
            let _ = std::io::stdin().read_exact(&mut [0u8; 1]);
        }
    }
}
