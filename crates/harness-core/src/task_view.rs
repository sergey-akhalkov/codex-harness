//! Owned native conversation windows. Window readiness never dispatches a model.
use crate::process::{CommandSpec, Job, Limits, OwnedProcess, ProcessIdentity};
use crate::process_service::ServiceProcess;
use serde::{Deserialize, Serialize};
use std::process::{Command, ExitStatus};
use std::{
    io,
    path::Path,
    ptr::null_mut,
    thread,
    time::{Duration, Instant},
};
use windows::Win32::Foundation::HWND as ComHWND;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::IVirtualDesktopManager;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, VK_MENU, keybd_event};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        Gdi::ClientToScreen,
    },
    System::Console::GetConsoleWindow,
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        EnumWindows, GW_HWNDPREV, GetClassNameW, GetClientRect, GetForegroundWindow, GetParent,
        GetWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SPI_GETWORKAREA, SW_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER,
        SetForegroundWindow, SetWindowPos, ShowWindow, SwitchToThisWindow, SystemParametersInfoW,
    },
};

/// The COM coclass behind `IVirtualDesktopManager`, which the `windows` crate
/// does not publish as a constant.
const CLSID_VIRTUAL_DESKTOP_MANAGER: windows::core::GUID = windows::core::GUID::from_values(
    0xaa509086,
    0x5ca9,
    0x4c25,
    [0x8f, 0x95, 0x58, 0x9d, 0x3c, 0x07, 0xb4, 0x8a],
);

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

/// The top-level window class Windows Terminal uses for every terminal window.
/// Foreground moving to one of these while a terminal command runs is the
/// terminal summoning the window that received the commandline, not the user
/// switching applications.
const TERMINAL_WINDOW_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";

pub fn foreground_window() -> Option<usize> {
    let window = unsafe { GetForegroundWindow() };
    (!window.is_null()).then_some(window as usize)
}

/// The Windows Terminal window that hosts this process's console. Windows
/// Terminal parents the pseudo console window it reports through
/// `GetConsoleWindow` to the terminal window of that session, which is the one
/// supported identity of the calling process's own terminal window.
pub fn console_terminal_window() -> Option<usize> {
    unsafe {
        let console = GetConsoleWindow();
        if console.is_null() {
            return None;
        }
        let parent = GetParent(console);
        (!parent.is_null() && terminal_window(parent)).then_some(parent as usize)
    }
}

/// Best-effort virtual-desktop membership check. A window on another desktop
/// must not be activated to receive a tab: that would switch the user's
/// desktop, and `wt -w 0` resolves only within the current desktop anyway.
pub fn window_on_current_virtual_desktop(window: usize) -> bool {
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let owned = initialized.is_ok();
        let on_current = CoCreateInstance(&CLSID_VIRTUAL_DESKTOP_MANAGER, None, CLSCTX_ALL)
            .ok()
            .and_then(|manager: IVirtualDesktopManager| {
                manager
                    .IsWindowOnCurrentVirtualDesktop(ComHWND(window as *mut core::ffi::c_void))
                    .ok()
                    .map(|on_current| on_current.as_bool())
            });
        if owned {
            CoUninitialize();
        }
        on_current.unwrap_or(false)
    }
}

/// Every visible top-level Windows Terminal window, for dispatch diagnostics
/// and verification.
pub fn terminal_windows() -> Vec<usize> {
    struct Found {
        windows: Vec<usize>,
    }
    unsafe extern "system" fn collect(window: HWND, parameter: LPARAM) -> i32 {
        let found = unsafe { &mut *(parameter as *mut Found) };
        if unsafe { IsWindowVisible(window) } != 0 && terminal_window(window) {
            found.windows.push(window as usize);
        }
        1
    }
    let mut found = Found {
        windows: Vec::new(),
    };
    unsafe {
        EnumWindows(Some(collect), (&mut found as *mut Found) as LPARAM);
    }
    found.windows
}

/// Activate `window` even though this background process lacks activation
/// rights, by borrowing the foreground input queue for the call. Returns once
/// the window is foreground or the attempt failed; the caller restores the
/// user's previous foreground window afterwards.
pub fn activate_window(window: usize) -> bool {
    let window = window as HWND;
    unsafe {
        let trace = std::env::var_os("HARNESS_TAB_TRACE").is_some();
        if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        let direct = SetForegroundWindow(window);
        if trace {
            eprintln!(
                "tab trace: direct activation of {:x} returned {}, foreground is {:x}",
                window as usize,
                direct,
                GetForegroundWindow() as usize
            );
        }
        if direct != 0 && GetForegroundWindow() == window {
            return true;
        }
        let foreground = GetForegroundWindow();
        if foreground.is_null() {
            return false;
        }
        let own_thread = GetCurrentThreadId();
        let foreground_thread = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
        let target_thread = GetWindowThreadProcessId(window, std::ptr::null_mut());
        let attached_foreground =
            foreground_thread != 0 && AttachThreadInput(own_thread, foreground_thread, 1) != 0;
        let attached_target = target_thread != 0
            && target_thread != foreground_thread
            && AttachThreadInput(own_thread, target_thread, 1) != 0;
        if trace {
            eprintln!(
                "tab trace: attach foreground {:x}/thread {} ok={}, target thread {} ok={}, last error {}",
                foreground as usize,
                foreground_thread,
                attached_foreground,
                target_thread,
                attached_target,
                std::io::Error::last_os_error()
            );
        }
        if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        let _ = SetForegroundWindow(window);
        if GetForegroundWindow() == window {
            if attached_target {
                let _ = AttachThreadInput(own_thread, target_thread, 0);
            }
            if attached_foreground {
                let _ = AttachThreadInput(own_thread, foreground_thread, 0);
            }
            return true;
        }
        // AttachThreadInput alone is not enough on current Windows: the shell
        // still refuses foreground transfer to a background process. The
        // legacy switch call and a momentary Alt press are the two remaining
        // escalation paths; both activate the window without synthetic clicks
        // or keys reaching the terminal content.
        SwitchToThisWindow(window, 0);
        if GetForegroundWindow() != window {
            keybd_event(VK_MENU as u8, 0, 0, 0);
            let _ = SetForegroundWindow(window);
            keybd_event(VK_MENU as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        if attached_target {
            let _ = AttachThreadInput(own_thread, target_thread, 0);
        }
        if attached_foreground {
            let _ = AttachThreadInput(own_thread, foreground_thread, 0);
        }
        GetForegroundWindow() == window
    }
}

/// Dispatch a terminal command whose tab is expected in `window`, and report
/// once that tab is observably created there.
///
/// The receiving window must stay foreground while the terminal resolves where
/// the commandline goes: `wt -w 0` picks the most recently used window at
/// resolution time, so restoring the user's window earlier would redirect the
/// tab back into it. A fixed tab title makes creation observable through the
/// window title, which follows the newly selected tab.
pub fn run_terminal_tab_in_window(
    command: &mut Command,
    window: usize,
    expected_title: &str,
    timeout: Duration,
) -> io::Result<ExitStatus> {
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    let mut exited: Option<ExitStatus> = None;
    loop {
        if exited.is_none()
            && let Some(status) = child.try_wait()?
        {
            exited = Some(status);
        }
        if let Some(status) = exited
            && !status.success()
        {
            // A failed launcher run creates no tab; waiting for one only
            // delays the error.
            break;
        }
        if window_title(window).contains(expected_title) {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    match exited {
        Some(status) => Ok(status),
        None => child.wait(),
    }
}

/// The current title of a top-level window, for targeted dispatch completion
/// checks and verification.
pub fn window_title(window: usize) -> String {
    let mut buffer = [0u16; 512];
    let copied =
        unsafe { GetWindowTextW(window as HWND, buffer.as_mut_ptr(), buffer.len() as i32) };
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..copied as usize])
}

/// Restore `window` as the foreground window after a dispatch that had to hold
/// the terminal window foreground for correct targeting.
pub fn restore_foreground_to(window: usize) {
    let window = window as HWND;
    if window.is_null() {
        return;
    }
    let foreground = unsafe { GetForegroundWindow() };
    if !foreground.is_null() && foreground != window {
        restore_foreground(foreground, window);
    }
}

/// Run a terminal command and undo the terminal's activation of the window that
/// received it, without touching a deliberate user switch; `previous` is the
/// foreground window the user must be left in.
///
/// Windows Terminal applies a dispatched commandline asynchronously and always
/// summons the receiving window, which can be a background window of another
/// project. Restoring only after the command exits loses that race, so watch
/// from launch: the moment a Windows Terminal window other than the previous
/// foreground window takes the foreground, restore the previous foreground
/// window. Foreground moving to any other window is the user's own switch and
/// is left alone.
pub fn run_restoring_foreground(
    command: &mut Command,
    settle: Duration,
    previous: usize,
) -> io::Result<ExitStatus> {
    let previous = previous as HWND;
    let mut child = command.spawn()?;
    let deadline = Instant::now() + settle;
    // The terminal can activate the receiving window more than once for one
    // commandline (the summon and the created tab). Stay armed until its
    // activations go quiet instead of stopping after the first restore.
    const QUIET_AFTER_EVENT: Duration = Duration::from_millis(600);
    const MAX_RESTORES: u32 = 8;
    let mut restores = 0;
    let mut exited: Option<ExitStatus> = None;
    let mut last_event = Instant::now();
    loop {
        let foreground = unsafe { GetForegroundWindow() };
        if !previous.is_null() && foreground != previous {
            if std::env::var_os("HARNESS_TAB_TRACE").is_some() {
                eprintln!(
                    "tab trace: foreground changed to {:x} (previous {:x}), terminal={}, restores={restores}",
                    foreground as usize,
                    previous as usize,
                    terminal_window(foreground)
                );
            }
            if terminal_window(foreground) && restores < MAX_RESTORES {
                restore_foreground(foreground, previous);
                restores += 1;
                last_event = Instant::now();
                if std::env::var_os("HARNESS_TAB_TRACE").is_some() {
                    eprintln!("tab trace: after restore foreground is {:x}", unsafe {
                        GetForegroundWindow()
                    }
                        as usize);
                }
            } else {
                // Either the user moved to their own window or the activation
                // keeps winning; stop fighting and keep that state.
                break;
            }
        }
        if exited.is_none()
            && let Some(status) = child.try_wait()?
        {
            exited = Some(status);
            last_event = Instant::now();
        }
        if exited.is_some() && last_event.elapsed() >= QUIET_AFTER_EVENT {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    match exited {
        Some(status) => Ok(status),
        None => child.wait(),
    }
}

fn terminal_window(window: HWND) -> bool {
    let mut class = [0u16; 64];
    let copied = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    copied > 0
        && TERMINAL_WINDOW_CLASS
            .encode_utf16()
            .eq(class[..copied as usize].iter().copied())
}

/// Restore `previous` after another window took the foreground. A background
/// process lacks activation rights even to undo a steal caused by its own
/// child, so borrow the involved input queues for the duration of the call.
fn restore_foreground(current: HWND, previous: HWND) {
    unsafe {
        if SetForegroundWindow(previous) != 0 && GetForegroundWindow() == previous {
            return;
        }
        let own_thread = GetCurrentThreadId();
        let current_thread = GetWindowThreadProcessId(current, std::ptr::null_mut());
        let previous_thread = GetWindowThreadProcessId(previous, std::ptr::null_mut());
        let attached_current =
            current_thread != 0 && AttachThreadInput(own_thread, current_thread, 1) != 0;
        let attached_previous = previous_thread != 0
            && previous_thread != current_thread
            && AttachThreadInput(own_thread, previous_thread, 1) != 0;
        let _ = SetForegroundWindow(previous);
        if attached_previous {
            let _ = AttachThreadInput(own_thread, previous_thread, 0);
        }
        if attached_current {
            let _ = AttachThreadInput(own_thread, current_thread, 0);
        }
    }
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
