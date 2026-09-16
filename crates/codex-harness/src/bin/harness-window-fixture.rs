//! Owned off-screen Win32 window fixture for native Nuphus acceptance.
//! Arguments: --state PATH --stop PATH. Writes `{"pid":N,"hwnd":M}` after the
//! window exists, polls the stop path every 100 ms and exits 0 after cleanup.

#[cfg(windows)]
mod fixture {
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::COLOR_WINDOW;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer,
        MSG, PostQuitMessage, RegisterClassExW, SW_SHOWNOACTIVATE, SetTimer, ShowWindow,
        TranslateMessage, WM_DESTROY, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
    };

    static STOP_PATH: OnceLock<PathBuf> = OnceLock::new();

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn usage() -> i32 {
        eprintln!("usage: harness-window-fixture --state PATH --stop PATH");
        2
    }

    fn options() -> Result<(PathBuf, PathBuf), i32> {
        let mut state = None;
        let mut stop = None;
        let mut args = std::env::args_os().skip(1);
        while let Some(key) = args.next() {
            let Some(value) = args.next() else {
                return Err(usage());
            };
            match key.to_str() {
                Some("--state") if state.replace(PathBuf::from(&value)).is_none() => {}
                Some("--stop") if stop.replace(PathBuf::from(value)).is_none() => {}
                _ => return Err(usage()),
            }
        }
        match (state, stop) {
            (Some(state), Some(stop)) => Ok((state, stop)),
            _ => Err(usage()),
        }
    }

    pub fn run() -> i32 {
        let (state, stop) = match options() {
            Ok(value) => value,
            Err(code) => return code,
        };
        let _ = STOP_PATH.set(stop);

        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        if instance.is_null() {
            eprintln!("harness-window-fixture: GetModuleHandleW failed");
            return 3;
        }
        let class_name = wide("HarnessWindowFixture");
        let window_title = wide("Harness owned Nuphus fixture");
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(window_procedure),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: (COLOR_WINDOW + 1) as *mut c_void,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        if unsafe { RegisterClassExW(&class) } == 0 {
            eprintln!("harness-window-fixture: RegisterClassExW failed");
            return 3;
        }
        let window = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                window_title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                -10000,
                -10000,
                320,
                180,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if window.is_null() {
            eprintln!("harness-window-fixture: CreateWindowExW failed");
            return 3;
        }
        unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
        let metadata = serde_json::json!({
            "pid": std::process::id(),
            "hwnd": window as i64,
        });
        let bytes = serde_json::to_vec(&metadata).unwrap_or_default();
        if let Some(parent) = state.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&state, bytes).is_err() {
            eprintln!("harness-window-fixture: cannot write state");
            unsafe { DestroyWindow(window) };
            return 4;
        }
        unsafe { SetTimer(window, 1, 100, None) };

        let mut message = MSG {
            hwnd: std::ptr::null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT { x: 0, y: 0 },
        };
        loop {
            let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
            match result {
                0 => return 0,
                -1 => {
                    eprintln!("harness-window-fixture: GetMessageW failed");
                    unsafe { DestroyWindow(window) };
                    return 5;
                }
                _ => unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                },
            }
        }
    }

    unsafe extern "system" fn window_procedure(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_TIMER => {
                if let Some(stop) = STOP_PATH.get()
                    && stop.exists()
                {
                    unsafe { DestroyWindow(window) };
                }
                0
            }
            WM_DESTROY => {
                unsafe { KillTimer(window, 1) };
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }
}

#[cfg(windows)]
fn main() {
    std::process::exit(fixture::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("harness-window-fixture is Windows-only");
    std::process::exit(1);
}
