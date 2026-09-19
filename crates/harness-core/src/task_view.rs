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
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        Gdi::ClientToScreen,
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GW_HWNDPREV, GetClientRect, GetForegroundWindow, GetWindow, GetWindowRect,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SPI_GETWORKAREA,
        SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos, SystemParametersInfoW,
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
            && visible_bounds(self.window, self.process.identity().pid).is_ok()
            && unobscured(self.window as HWND)?)
    }

    pub(crate) fn process_running(&self) -> io::Result<bool> {
        self.process.is_running()
    }
}

fn unavailable() -> io::Error {
    io::Error::other("conversation window is unavailable; suspend new model dispatch")
}

/// Run `f` without leaving the user in a newly created window.
///
/// Creating a console or asking the current terminal to add a tab can still
/// activate that window. Restore the previous foreground window afterwards so
/// a spawn from the lead does not yank focus from another app.
pub fn preserve_foreground<T>(f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let previous = unsafe { GetForegroundWindow() };
    let result = f();
    if !previous.is_null() {
        unsafe {
            let _ = SetForegroundWindow(previous);
        }
    }
    result
}

/// Conservatively require the conversation client area to be unobscured.
/// IsWindowVisible checks a style bit, not whether another app covers the text.
fn unobscured(window: HWND) -> io::Result<bool> {
    if cloaked(window)? {
        return Ok(false);
    }
    let mut area = RECT::default();
    let mut origin = POINT::default();
    if unsafe { GetClientRect(window, &mut area) } == 0
        || unsafe { ClientToScreen(window, &mut origin) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    area.left += origin.x;
    area.right += origin.x;
    area.top += origin.y;
    area.bottom += origin.y;
    if area.left >= area.right || area.top >= area.bottom {
        return Ok(false);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut above = unsafe { GetWindow(window, GW_HWNDPREV) };
    while !above.is_null() {
        // Window destruction/reordering can invalidate a traversal. Never loop
        // indefinitely or infer visibility from an incomplete observation.
        if seen.len() >= 4096 || !seen.insert(above as usize) {
            return Ok(false);
        }
        if unsafe { IsWindowVisible(above) } != 0 && unsafe { IsIconic(above) } == 0 {
            let mut other = RECT::default();
            if unsafe { GetWindowRect(above, &mut other) } == 0 {
                return Ok(false);
            }
            if overlaps(&area, &other) && !cloaked(above)? {
                return Ok(false);
            }
        }
        above = unsafe { GetWindow(above, GW_HWNDPREV) };
    }
    Ok(true)
}

fn cloaked(window: HWND) -> io::Result<bool> {
    let mut value = 0u32;
    let result = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED as u32,
            (&mut value as *mut u32).cast(),
            std::mem::size_of_val(&value) as u32,
        )
    };
    if result < 0 {
        return Err(io::Error::other(
            "conversation composition state unavailable",
        ));
    }
    Ok(value != 0)
}

fn overlaps(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
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
    pub fn wait_for_title(&self, title: &str, timeout: Duration) -> io::Result<()> {
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
                && loaded_title(
                    &String::from_utf16_lossy(&buffer[..count as usize]),
                    &expected,
                )
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
                        SWP_NOACTIVATE,
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

    pub(crate) fn place(&self, bounds: Bounds) -> io::Result<()> {
        if unsafe {
            SetWindowPos(
                self.window as HWND,
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
        Ok(())
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

fn loaded_title(caption: &str, expected: &str) -> bool {
    if caption.starts_with(expected) {
        return true;
    }
    // An already-running child renders the native braille activity spinner
    // while its first request is held for this window. Keep the thread name
    // exact, but do not wait for idle: that would deadlock first admission.
    let mut chars = caption.chars();
    matches!(chars.next(), Some('\u{2800}'..='\u{28ff}'))
        && chars.next() == Some(' ')
        && chars.as_str().starts_with(expected)
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

/// Tile lead, two executors, replacement lead, then extra helper panes.
pub fn layout(count: usize) -> io::Result<Vec<Bounds>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let three = three_windows()?;
    if count <= 3 {
        return Ok(three.into_iter().take(count).collect());
    }
    let mut placements = three.to_vec();
    let mut replacement = placements[0];
    replacement.y += replacement.height / 2;
    replacement.height -= replacement.height / 2;
    placements.push(replacement);
    if count == 4 {
        return Ok(placements);
    }
    let extra = count - 4;
    let base = placements[2];
    if extra == 0 || base.width < extra as i32 || base.height < extra as i32 {
        return Err(unavailable());
    }
    let strip = (base.height / (1 + extra as i32)).max(1);
    placements[2].height -= strip * extra as i32;
    for index in 0..extra {
        placements.push(Bounds {
            x: base.x,
            y: placements[2].y + placements[2].height + strip * index as i32,
            width: base.width,
            height: if index + 1 == extra {
                base.y + base.height
                    - (placements[2].y + placements[2].height + strip * index as i32)
            } else {
                strip
            },
        });
    }
    Ok(placements)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_running_child_caption_is_ready_without_waiting_for_its_model() {
        assert!(loaded_title("Executor 1 | workspace", "Executor 1 | "));
        assert!(loaded_title("⠸ Executor 1 | workspace", "Executor 1 | "));
        assert!(!loaded_title("⠸ Executor 2 | workspace", "Executor 1 | "));
        assert!(!loaded_title(
            "Opening Executor 1 | workspace",
            "Executor 1 | "
        ));
    }
    #[test]
    fn layout_grows_helper_tiles_without_dropping_executor_slots() {
        let four = layout(4).unwrap();
        assert_eq!(four.len(), 4);
        assert_eq!(four[0].x, three_windows().unwrap()[0].x);
        assert!(four[3].width > 0 && four[3].height > 0);
        let five = layout(5).unwrap();
        assert_eq!(five.len(), 5);
        assert_eq!(five[1], four[1]);
        assert!(five[4].width > 0 && five[4].height > 0);
        assert!(five[4].y >= five[2].y);
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SW_HIDE, ShowWindow, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
        WS_POPUP, WS_VISIBLE,
    };

    struct OwnedWindow(HWND);
    impl Drop for OwnedWindow {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
    fn owned_window(x: i32, y: i32, width: i32, height: i32) -> OwnedWindow {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "Owned conversation visibility check\0"
            .encode_utf16()
            .collect();
        let window = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                width,
                height,
                null_mut(),
                null_mut(),
                null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!window.is_null(), "{}", io::Error::last_os_error());
        OwnedWindow(window)
    }

    #[test]
    #[ignore = "creates two short-lived owned desktop windows; requires available interactive desktop"]
    fn obscured_chat_is_not_visible_until_cover_is_removed() {
        let chat = owned_window(30, 30, 400, 240);
        assert!(unobscured(chat.0).unwrap());
        let cover = owned_window(80, 80, 150, 100);
        assert_ne!(unsafe { IsWindowVisible(chat.0) }, 0);
        assert!(
            !unobscured(chat.0).unwrap(),
            "partial coverage hides conversation content"
        );
        unsafe {
            SetWindowPos(
                cover.0,
                null_mut(),
                30,
                30,
                400,
                240,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
        assert!(
            !unobscured(chat.0).unwrap(),
            "full coverage must also suspend admission"
        );
        unsafe {
            SetWindowPos(
                cover.0,
                null_mut(),
                430,
                30,
                150,
                100,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
        assert!(
            unobscured(chat.0).unwrap(),
            "touching edges do not obscure the client"
        );
        unsafe {
            SetWindowPos(
                cover.0,
                null_mut(),
                80,
                80,
                150,
                100,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            ShowWindow(cover.0, SW_HIDE);
        }
        assert!(
            unobscured(chat.0).unwrap(),
            "a hidden covering window does not obscure the client"
        );
    }
}
