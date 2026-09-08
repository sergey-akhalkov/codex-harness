//! Model-free owned ConPTY fixture. Arguments: ROLE [ARTIFACT] [ARGS...].
//! All mutations stay in the explicitly supplied test artifacts.

#[cfg(windows)]
mod fixture {
    use serde_json::json;
    use std::io::{self, Read, Write};
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
        fn GetConsoleScreenBufferInfo(
            handle: *mut std::ffi::c_void,
            info: *mut ScreenBuffer,
        ) -> i32;
        fn SetConsoleCP(cp: u32) -> i32;
        fn SetConsoleOutputCP(cp: u32) -> i32;
    }

    #[repr(C)]
    struct Coord {
        x: i16,
        y: i16,
    }
    #[repr(C)]
    struct Rect {
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
    }
    #[repr(C)]
    struct ScreenBuffer {
        size: Coord,
        cursor: Coord,
        attributes: u16,
        window: Rect,
        maximum: Coord,
    }

    fn record(path: &Path, value: serde_json::Value) -> io::Result<()> {
        let staging = path.with_extension("pending");
        std::fs::write(&staging, serde_json::to_vec(&value)?)?;
        std::fs::rename(staging, path)
    }

    fn console_attached(output: &impl AsRawHandle) -> bool {
        let mut mode = 0;
        unsafe { GetConsoleMode(output.as_raw_handle(), &mut mode) != 0 }
    }

    fn window_size(output: &impl AsRawHandle) -> Option<(i16, i16)> {
        let mut info = ScreenBuffer {
            size: Coord { x: 0, y: 0 },
            cursor: Coord { x: 0, y: 0 },
            attributes: 0,
            window: Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            maximum: Coord { x: 0, y: 0 },
        };
        if unsafe { GetConsoleScreenBufferInfo(output.as_raw_handle(), &mut info) } == 0 {
            return None;
        }
        Some((
            info.window
                .right
                .saturating_sub(info.window.left)
                .saturating_add(1),
            info.window
                .bottom
                .saturating_sub(info.window.top)
                .saturating_add(1),
        ))
    }

    fn write_line(output: &mut impl Write, text: &str) -> io::Result<()> {
        writeln!(output, "{text}")?;
        output.flush()
    }

    pub fn run() -> io::Result<()> {
        let mut input = io::stdin().lock();
        let mut output = io::stdout().lock();
        unsafe {
            SetConsoleCP(65001);
            SetConsoleOutputCP(65001);
        }
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let role = args.first().and_then(|v| v.to_str()).unwrap_or("");
        match role {
            "report" => {
                let artifact = Path::new(
                    args.get(1)
                        .ok_or_else(|| io::Error::other("report ARTIFACT required"))?,
                );
                let mut text = String::new();
                input.read_to_string(&mut text)?;
                record(
                    artifact,
                    json!({
                        "pid": std::process::id(),
                        "cwd": std::env::current_dir()?,
                        "args": args.iter().skip(2).map(|s| s.to_string_lossy()).collect::<Vec<_>>(),
                        "system_root": std::env::var("SystemRoot").ok(),
                        "marker": std::env::var("HARNESS_CONSOLE_MARKER").ok(),
                        "console": console_attached(&output),
                        "window": window_size(&output),
                        "stdin": text,
                    }),
                )?;
                write_line(&mut output, "fixture stdout")?;
                write_line(&mut output, &text)?;
                writeln!(io::stderr().lock(), "fixture stderr")?;
                Ok(())
            }
            "nonzero" => {
                write_line(&mut output, "fixture failure")?;
                std::process::exit(19);
            }
            "flood" => {
                for _ in 0..2048 {
                    output.write_all(&[b'x'; 4096])?;
                }
                output.flush()
            }
            "hold" => {
                let artifact = Path::new(
                    args.get(1)
                        .ok_or_else(|| io::Error::other("hold ARTIFACT required"))?,
                );
                record(
                    artifact,
                    json!({
                        "pid": std::process::id(),
                        "console": console_attached(&output),
                    }),
                )?;
                std::thread::sleep(Duration::from_secs(45));
                std::process::exit(99);
            }
            "interactive" => {
                let artifact = Path::new(
                    args.get(1)
                        .ok_or_else(|| io::Error::other("interactive ARTIFACT required"))?,
                );
                record(
                    artifact,
                    json!({"pid": std::process::id(), "ready": true, "console": console_attached(&output)}),
                )?;
                write_line(&mut output, "prompt>")?;
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 1];
                loop {
                    let n = std::io::Read::read(&mut input, &mut chunk)?;
                    if n == 0 {
                        break;
                    }
                    bytes.push(chunk[0]);
                    if chunk[0] == b'\n' {
                        break;
                    }
                }
                let line = String::from_utf8_lossy(&bytes);
                write_line(&mut output, &format!("echo:{line}"))?;
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    if let Some((columns, rows)) = window_size(&output)
                        && columns == 80
                        && rows == 24
                    {
                        write_line(&mut output, &format!("resized:{columns}x{rows}"))?;
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                write_line(&mut output, "resized:timeout")?;
                Ok(())
            }
            _ => Err(io::Error::other("unknown console fixture role")),
        }
    }
}

fn main() {
    #[cfg(windows)]
    if let Err(error) = fixture::run() {
        eprintln!("console fixture: {error}");
        std::process::exit(2);
    }
    #[cfg(not(windows))]
    {
        eprintln!("console fixture requires Windows 10+");
        std::process::exit(2);
    }
}
