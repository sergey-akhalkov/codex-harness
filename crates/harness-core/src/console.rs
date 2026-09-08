//! Headless Windows ConPTY sessions for native console acceptance.
//!
//! Process creation and job assignment remain in process.rs; this module owns
//! the pseudoconsole, its pipes, UTF-8 pumping and bounded cleanup.
//! https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session

use crate::process::{Cancellation, CommandSpec, Deadline, Limits, Outcome, ProcessIdentity};
use std::io;
use std::time::Duration;

const DEFAULT_COLUMNS: i16 = 140;
const DEFAULT_ROWS: i16 = 40;

#[derive(Clone, Copy, Debug)]
pub struct ConsoleSize {
    pub columns: i16,
    pub rows: i16,
}

impl Default for ConsoleSize {
    fn default() -> Self {
        Self {
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
        }
    }
}

#[derive(Debug)]
pub struct ConsoleSpec {
    pub command: CommandSpec,
    pub size: ConsoleSize,
    pub limits: Limits,
    /// Maximum retained transcript bytes; excess output is still drained.
    pub max_output_bytes: usize,
}

impl ConsoleSpec {
    pub fn new(command: CommandSpec) -> Self {
        Self {
            command,
            size: ConsoleSize::default(),
            limits: Limits::default(),
            max_output_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub struct ConsoleOutcome {
    pub outcome: Outcome,
    pub transcript: String,
    pub output_truncated: bool,
}

pub struct ConsoleSession {
    #[cfg(windows)]
    inner: windows::Session,
}

impl ConsoleSession {
    pub fn spawn(spec: ConsoleSpec) -> io::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows::spawn(spec)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = spec;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows 10+ ConPTY is required",
            ))
        }
    }

    pub fn identity(&self) -> ProcessIdentity {
        #[cfg(windows)]
        {
            self.inner.identity()
        }
        #[cfg(not(windows))]
        {
            unreachable!("Windows 10+ ConPTY is required")
        }
    }

    pub fn transcript(&self) -> String {
        #[cfg(windows)]
        {
            self.inner.transcript()
        }
        #[cfg(not(windows))]
        {
            String::new()
        }
    }

    pub fn send(&self, text: &str) -> io::Result<()> {
        #[cfg(windows)]
        {
            self.inner.send(text)
        }
        #[cfg(not(windows))]
        {
            let _ = text;
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }
    }

    pub fn close_input(&self) -> io::Result<()> {
        #[cfg(windows)]
        {
            self.inner.close_input()
        }
        #[cfg(not(windows))]
        {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }
    }

    pub fn resize(&self, size: ConsoleSize) -> io::Result<()> {
        #[cfg(windows)]
        {
            self.inner.resize(size)
        }
        #[cfg(not(windows))]
        {
            let _ = size;
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }
    }

    pub fn wait(
        self,
        deadline: Deadline,
        cancellation: &Cancellation,
        cleanup_timeout: Duration,
    ) -> io::Result<ConsoleOutcome> {
        #[cfg(windows)]
        {
            self.inner.wait(deadline, cancellation, cleanup_timeout)
        }
        #[cfg(not(windows))]
        {
            let _ = (deadline, cancellation, cleanup_timeout);
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::process::{Job, OwnedProcess};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;

    struct Shared {
        input: Mutex<Option<File>>,
        transcript: Mutex<Vec<u8>>,
        output_truncated: AtomicBool,
        console: Mutex<HPCON>,
    }

    pub(super) struct Session {
        job: Option<Job>,
        process: Option<OwnedProcess>,
        shared: Arc<Shared>,
        pump: Option<JoinHandle<()>>,
    }

    impl Session {
        pub(super) fn identity(&self) -> ProcessIdentity {
            self.process.as_ref().expect("console process").identity()
        }

        pub(super) fn transcript(&self) -> String {
            String::from_utf8_lossy(&self.shared.transcript.lock().expect("console transcript"))
                .into_owned()
        }

        pub(super) fn send(&self, text: &str) -> io::Result<()> {
            let mut guard = self.shared.input.lock().expect("console input");
            let input = guard
                .as_mut()
                .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "console input closed"))?;
            input.write_all(text.as_bytes())?;
            input.flush()
        }

        pub(super) fn close_input(&self) -> io::Result<()> {
            *self.shared.input.lock().expect("console input") = None;
            Ok(())
        }

        pub(super) fn resize(&self, size: ConsoleSize) -> io::Result<()> {
            if size.columns <= 0 || size.rows <= 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "console size must be positive",
                ));
            }
            let console = *self.shared.console.lock().expect("console handle");
            if console == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "console already closed",
                ));
            }
            hresult(unsafe {
                ResizePseudoConsole(
                    console,
                    COORD {
                        X: size.columns,
                        Y: size.rows,
                    },
                )
            })
        }

        pub(super) fn wait(
            mut self,
            deadline: Deadline,
            cancellation: &Cancellation,
            cleanup_timeout: Duration,
        ) -> io::Result<ConsoleOutcome> {
            let process = self.process.take().expect("console process");
            let job = self.job.take().expect("console job");
            let outcome = job.wait(&process, deadline, cancellation, cleanup_timeout)?;
            // Close ConPTY after the process so remaining output can drain,
            // then join the pump before returning the transcript.
            self.close_console();
            if let Some(pump) = self.pump.take() {
                let _ = pump.join();
            }
            let transcript = self.transcript();
            Ok(ConsoleOutcome {
                outcome,
                transcript,
                output_truncated: self.shared.output_truncated.load(Ordering::Relaxed),
            })
        }

        fn close_console(&self) {
            let console = {
                let mut guard = self.shared.console.lock().expect("console handle");
                let console = *guard;
                *guard = 0;
                console
            };
            if console != 0 {
                unsafe { ClosePseudoConsole(console) };
            }
            let _ = self.close_input();
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            if let (Some(job), Some(process)) = (self.job.take(), self.process.take()) {
                let cancellation = Cancellation::default();
                cancellation.cancel();
                if let Ok(deadline) = Deadline::after(Duration::from_secs(5)) {
                    let _ = job.wait(&process, deadline, &cancellation, Duration::from_secs(5));
                }
            }
            self.close_console();
            if let Some(pump) = self.pump.take() {
                let _ = pump.join();
            }
        }
    }

    fn hresult(status: i32) -> io::Result<()> {
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status))
        }
    }

    fn pipe_pair() -> io::Result<(OwnedHandle, OwnedHandle)> {
        let mut read = null_mut();
        let mut write = null_mut();
        if unsafe { CreatePipe(&mut read, &mut write, null_mut(), 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if read.is_null()
            || write.is_null()
            || read == INVALID_HANDLE_VALUE
            || write == INVALID_HANDLE_VALUE
        {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe {
            (
                OwnedHandle::from_raw_handle(read),
                OwnedHandle::from_raw_handle(write),
            )
        })
    }

    pub(super) fn spawn(spec: ConsoleSpec) -> io::Result<Session> {
        if spec.size.columns <= 0 || spec.size.rows <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "console size must be positive",
            ));
        }
        let (input_read, input_write) = pipe_pair()?;
        let (output_read, output_write) = pipe_pair()?;
        let mut console: HPCON = 0;
        let status = unsafe {
            CreatePseudoConsole(
                COORD {
                    X: spec.size.columns,
                    Y: spec.size.rows,
                },
                input_read.as_raw_handle(),
                output_write.as_raw_handle(),
                0,
                &mut console,
            )
        };
        hresult(status)?;
        if console == 0 {
            return Err(io::Error::other("CreatePseudoConsole returned no handle"));
        }
        let shared = Arc::new(Shared {
            input: Mutex::new(Some(unsafe {
                File::from_raw_handle(input_write.into_raw_handle())
            })),
            transcript: Mutex::new(Vec::new()),
            output_truncated: AtomicBool::new(false),
            console: Mutex::new(console),
        });
        let mut output = unsafe { File::from_raw_handle(output_read.into_raw_handle()) };
        let pump_shared = Arc::clone(&shared);
        let pump = thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match output.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut transcript =
                            pump_shared.transcript.lock().expect("console transcript");
                        let retained =
                            n.min(spec.max_output_bytes.saturating_sub(transcript.len()));
                        transcript.extend_from_slice(&buffer[..retained]);
                        if retained < n {
                            pump_shared.output_truncated.store(true, Ordering::Relaxed);
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let mut session = Session {
            job: None,
            process: None,
            shared,
            pump: Some(pump),
        };
        let job = Job::new(spec.limits)?;
        let created = unsafe { job.spawn_console(&spec.command, console) };
        drop(input_read);
        drop(output_write);
        match created {
            Ok(process) => {
                session.job = Some(job);
                session.process = Some(process);
                Ok(session)
            }
            Err(error) => Err(error),
        }
    }
}
