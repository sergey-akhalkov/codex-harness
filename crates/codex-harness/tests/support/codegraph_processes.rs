//! Read-only native acceptance sampling of explicit owners and their descendants.
use harness_core::process::ProcessIdentity;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows_sys::Win32::{
    Foundation::{FILETIME, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
};

fn inspect(pid: u32) -> io::Result<(ProcessIdentity, usize)> {
    let raw = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut created = FILETIME::default();
    let (mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    if unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    if unsafe {
        GetProcessMemoryInfo(
            process.as_raw_handle(),
            (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            memory.cb,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((
        ProcessIdentity {
            pid,
            creation_time: (u64::from(created.dwHighDateTime) << 32)
                | u64::from(created.dwLowDateTime),
        },
        memory.PrivateUsage,
    ))
}

pub fn sample(roots: &[ProcessIdentity]) -> io::Result<Value> {
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut row = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut rows = Vec::new();
    let mut images = BTreeMap::new();
    let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut row) };
    while found != 0 {
        rows.push((row.th32ProcessID, row.th32ParentProcessID));
        let length = row
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(row.szExeFile.len());
        images.insert(
            row.th32ProcessID,
            String::from_utf16_lossy(&row.szExeFile[..length]).to_ascii_lowercase(),
        );
        if rows.len() > 32768 {
            return Err(io::Error::other("process snapshot allowance exceeded"));
        }
        found = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut row) };
    }
    let mut owned = BTreeMap::new();
    for root in roots {
        let (identity, bytes) = inspect(root.pid)?;
        if identity != *root {
            return Err(io::Error::other("acceptance process identity changed"));
        }
        owned.insert(root.pid, (identity, bytes));
    }
    loop {
        let before = owned.len();
        for &(pid, parent) in &rows {
            if owned.contains_key(&pid) {
                continue;
            }
            if let Some((owner, _)) = owned.get(&parent)
                && let Ok((identity, bytes)) = inspect(pid)
                && identity.creation_time >= owner.creation_time
            {
                owned.insert(pid, (identity, bytes));
            }
        }
        if owned.len() == before {
            break;
        }
    }
    let mut counts = BTreeMap::<String, usize>::new();
    for pid in owned.keys() {
        *counts
            .entry(images.get(pid).cloned().unwrap_or_default())
            .or_default() += 1;
    }
    Ok(
        json!({"processes":owned.len(),"images":counts,"private_bytes":owned.values().map(|(_, bytes)| *bytes).sum::<usize>(),
        "identities":owned.values().map(|(identity, _)| json!({"pid":identity.pid,"created":identity.creation_time})).collect::<Vec<_>>()}),
    )
}
