//! Owned native conversation windows. Window readiness never dispatches a model.
use crate::process::{CommandSpec, Job, Limits, OwnedProcess, ProcessIdentity};
use crate::process_service::ServiceProcess;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::Path,
    ptr::null_mut,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, RECT},
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
        SystemParametersInfoW,
    },
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub process: ProcessIdentity,
    pub window: usize,
    pub bounds: Bounds,
}

impl Snapshot {
    /// Check a retained view against the exact native executable, user and live window.
    pub fn is_visible(&self, executable: &Path, user: &str) -> io::Result<bool> {
        match Watch::open(self, executable, user)? {
            Some(watch) => watch.visible(),
            None => Ok(false),
        }
    }
}

/// A controller request for a native view, with no prompt or model dispatch.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Request {
    pub schema: u32,
    pub thread_id: String,
    pub title: String,
    pub slot: usize,
}

/// The Job contains the frontend only; a remote backend has a separate owner.
pub struct View {
    _job: Job,
    process: OwnedProcess,
    window: usize,
}

/// Read-only observation from the controller. Retaining the validated process
/// handle detects frontend death even when its last state file says visible.
pub(crate) struct Watch {
    process: ServiceProcess,
    window: usize,
}

impl Watch {
    pub(crate) fn open(
        snapshot: &Snapshot,
        executable: &Path,
        user: &str,
    ) -> io::Result<Option<Self>> {
        Ok(
            ServiceProcess::inspect(snapshot.process, executable, user)?.map(|process| Self {
                process,
                window: snapshot.window,
            }),
        )
    }

    pub(crate) fn visible(&self) -> io::Result<bool> {
        Ok(self.process.is_running()?
            && visible_bounds(self.window, self.process.identity().pid).is_ok())
    }
}

fn unavailable() -> io::Error {
    io::Error::other("conversation window is unavailable; suspend new model dispatch")
}

struct FindWindow {
    pid: u32,
    window: Option<usize>,
}

unsafe extern "system" fn find_window(window: HWND, parameter: LPARAM) -> i32 {
    // EnumWindows invokes this synchronously; the stack record outlives the call.
    let search = unsafe { &mut *(parameter as *mut FindWindow) };
    let mut pid = 0;
    if unsafe { GetWindowThreadProcessId(window, &mut pid) } != 0
        && pid == search.pid
        && unsafe { IsWindowVisible(window) } != 0
    {
        search.window = Some(window as usize);
    }
    1
}

impl View {
    /// The native TUI replaces the launch caption after loading its named thread.
    pub(crate) fn wait_for_title(&self, title: &str, timeout: Duration) -> io::Result<()> {
        let until = Instant::now() + timeout;
        let expected = format!("{title} | ");
        loop {
            self.snapshot()?;
            let mut buffer = [0u16; 1024];
            let count = unsafe {
                GetWindowTextW(
                    self.window as HWND,
                    buffer.as_mut_ptr(),
                    buffer.len() as i32,
                )
            };
            if count > 0
                && String::from_utf16_lossy(&buffer[..count as usize]).starts_with(&expected)
            {
                return Ok(());
            }
            if !self.is_running()? || Instant::now() >= until {
                return Err(io::Error::other(
                    "native conversation did not load its named thread before dispatch",
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn spawn(command: &CommandSpec, bounds: Bounds, timeout: Duration) -> io::Result<Self> {
        if command.new_console.is_none() || bounds.width <= 0 || bounds.height <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "visible console and positive bounds required",
            ));
        }
        let job = Job::new(Limits::default())?;
        let process = job.spawn(command)?;
        let until = Instant::now()
            .checked_add(timeout)
            .ok_or_else(unavailable)?;
        let mut search = FindWindow {
            pid: process.identity().pid,
            window: None,
        };
        loop {
            if !process.is_running()? || Instant::now() >= until {
                return Err(unavailable());
            }
            if unsafe {
                EnumWindows(
                    Some(find_window),
                    (&mut search as *mut FindWindow) as LPARAM,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if let Some(window) = search.window {
                let view = Self {
                    _job: job,
                    process,
                    window,
                };
                view.snapshot()?;
                if unsafe {
                    SetWindowPos(
                        window as HWND,
                        null_mut(),
                        bounds.x,
                        bounds.y,
                        bounds.width,
                        bounds.height,
                        SWP_NOACTIVATE | SWP_NOZORDER,
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                view.snapshot()?;
                return Ok(view);
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    pub fn identity(&self) -> ProcessIdentity {
        self.process.identity()
    }

    pub fn is_running(&self) -> io::Result<bool> {
        self.process.is_running()
    }

    pub fn exit_code(&self) -> io::Result<Option<u32>> {
        self.process.exit_code()
    }

    /// Recheck before dispatch. A saved HWND or a live process alone is insufficient.
    pub fn snapshot(&self) -> io::Result<Snapshot> {
        if !self.process.is_running()? {
            return Err(unavailable());
        }
        Ok(Snapshot {
            process: self.process.identity(),
            window: self.window,
            bounds: visible_bounds(self.window, self.process.identity().pid)?,
        })
    }
}

fn visible_bounds(window: usize, expected_pid: u32) -> io::Result<Bounds> {
    let window = window as HWND;
    let mut pid = 0;
    if unsafe { GetWindowThreadProcessId(window, &mut pid) } == 0
        || pid != expected_pid
        || unsafe { IsWindowVisible(window) } == 0
        || unsafe { IsIconic(window) } != 0
    {
        return Err(unavailable());
    }
    let mut bounds = RECT::default();
    if unsafe { GetWindowRect(window, &mut bounds) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if bounds.right <= bounds.left || bounds.bottom <= bounds.top {
        return Err(unavailable());
    }
    Ok(Bounds {
        x: bounds.left,
        y: bounds.top,
        width: bounds.right - bounds.left,
        height: bounds.bottom - bounds.top,
    })
}

/// Initial layout for the current one-lead, two-executor workflow.
pub fn three_windows() -> io::Result<[Bounds; 3]> {
    let mut work = RECT::default();
    if unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let width = work.right - work.left;
    let height = work.bottom - work.top;
    if width < 2 || height < 2 {
        return Err(unavailable());
    }
    let left = width / 2;
    let top = height / 2;
    Ok([
        Bounds {
            x: work.left,
            y: work.top,
            width: left,
            height,
        },
        Bounds {
            x: work.left + left,
            y: work.top,
            width: width - left,
            height: top,
        },
        Bounds {
            x: work.left + left,
            y: work.top + top,
            width: width - left,
            height: height - top,
        },
    ])
}
