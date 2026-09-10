//! Cancellable synchronous Windows byte-pipe I/O, without owning the peer.
//!
//! Blocking ReadFile/WriteFile on anonymous pipes cannot be interrupted by
//! closing a foreign peer. This module owns only the local File endpoints and
//! a dedicated worker thread. Cancellation uses CancelSynchronousIo against a
//! duplicated real thread handle (GetCurrentThread is a pseudo-handle) plus
//! CancelIoEx on the file, then a bounded WaitForSingleObject. Returning
//! Cancelled/DeadlineExpired is not itself proof the worker has stopped; the
//! join outcome is. CancelSynchronousIo does not wait for the I/O to finish
//! (Microsoft Learn, ioapiset.h, last updated 2023-06-21). Never TerminateThread.
//!
//! The caller cancellation token is read-only. Cancellation before the first
//! I/O call executes no ReadFile/WriteFile. Unsupported handle types are
//! rejected with an explicit reason. JSON framing belongs to mcp_protocol.

#![cfg(windows)]

use crate::process::{Cancellation, Deadline};
use std::fs::File;
use std::io;
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_BROKEN_PIPE, ERROR_NO_DATA,
    ERROR_OPERATION_ABORTED, GetLastError, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType, ReadFile, WriteFile};
use windows_sys::Win32::System::IO::{CancelIoEx, CancelSynchronousIo, IO_STATUS_BLOCK};
use windows_sys::Win32::System::Pipes::GetNamedPipeInfo;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, INFINITE, WaitForSingleObject,
};

/// Matches the parent MCP framing read chunk. Protocol parsing stays elsewhere.
pub const READ_CHUNK: usize = 64 * 1024;
/// Matches parent MCP line-frame bound. This module does not parse JSON-RPC.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
/// Default anonymous-pipe buffer used by existing MCP CreatePipe call sites.
pub const PIPE_BUFFER: u32 = 64 * 1024;
const JOIN_POLL: Duration = Duration::from_millis(20);
const HANDLE_READY: u8 = 1;
const HANDLE_FAILED: u8 = 2;

// ABI matches windows-sys 0.61.2 and Microsoft's FileModeInformation contract.
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryInformationFile(
        file: windows_sys::Win32::Foundation::HANDLE,
        status: *mut IO_STATUS_BLOCK,
        information: *mut std::ffi::c_void,
        length: u32,
        class: i32,
    ) -> i32;
}

#[derive(Default)]
struct ModeQuery {
    status: IO_STATUS_BLOCK,
    mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeIoError {
    Cancelled { worker_joined: bool },
    DeadlineExpired { worker_joined: bool },
    EndOfFile,
    Io(String),
    Unsupported { reason: String },
    UnresolvedOwnership { reason: String },
}

impl PipeIoError {
    pub fn worker_joined(&self) -> Option<bool> {
        match self {
            Self::Cancelled { worker_joined } | Self::DeadlineExpired { worker_joined } => {
                Some(*worker_joined)
            }
            _ => None,
        }
    }
}

impl From<PipeIoError> for io::Error {
    fn from(error: PipeIoError) -> Self {
        match error {
            PipeIoError::Cancelled { .. } => io::Error::new(io::ErrorKind::Interrupted, error),
            PipeIoError::DeadlineExpired { .. } => io::Error::new(io::ErrorKind::TimedOut, error),
            PipeIoError::EndOfFile => io::Error::new(io::ErrorKind::UnexpectedEof, error),
            PipeIoError::Io(_) => io::Error::other(error),
            PipeIoError::Unsupported { .. } => io::Error::new(io::ErrorKind::Unsupported, error),
            PipeIoError::UnresolvedOwnership { .. } => {
                io::Error::new(io::ErrorKind::TimedOut, error)
            }
        }
    }
}

impl std::fmt::Display for PipeIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled { worker_joined } => {
                write!(f, "pipe I/O cancelled; worker_joined={worker_joined}")
            }
            Self::DeadlineExpired { worker_joined } => {
                write!(
                    f,
                    "pipe I/O deadline expired; worker_joined={worker_joined}"
                )
            }
            Self::EndOfFile => write!(f, "pipe reached EOF"),
            Self::Io(message) => write!(f, "{message}"),
            Self::Unsupported { reason } => write!(f, "unsupported pipe endpoint: {reason}"),
            Self::UnresolvedOwnership { reason } => write!(
                f,
                "pipe worker ownership unresolved after bounded wait: {reason}"
            ),
        }
    }
}

impl std::error::Error for PipeIoError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Read,
    Write,
}

enum Command {
    Read { max: usize },
    Write { bytes: Vec<u8> },
}

enum Reply {
    Bytes(Vec<u8>),
    Written(usize),
    Failed(PipeIoError),
}

struct Shared {
    thread: Mutex<Option<OwnedHandle>>,
    file: Mutex<Option<File>>,
    handle_state: AtomicU8,
    started_io: AtomicBool,
    stop: AtomicBool,
    #[cfg(test)]
    pause_before_io: Mutex<Option<Arc<TestPause>>>,
}

#[cfg(test)]
#[derive(Default)]
struct TestPause {
    entered: AtomicBool,
    release: AtomicBool,
}

/// One owned local pipe endpoint with a dedicated I/O worker thread.
pub struct CancellablePipe {
    commands: Option<SyncSender<Command>>,
    replies: Option<mpsc::Receiver<Reply>>,
    worker: Option<JoinHandle<Result<(), PipeIoError>>>,
    cancel: Cancellation,
    shared: Arc<Shared>,
    direction: Direction,
}

impl CancellablePipe {
    pub fn reader(file: File, cancel: Cancellation) -> Result<Self, PipeIoError> {
        Self::spawn(file, Direction::Read, cancel)
    }

    pub fn writer(file: File, cancel: Cancellation) -> Result<Self, PipeIoError> {
        Self::spawn(file, Direction::Write, cancel)
    }

    fn spawn(file: File, direction: Direction, cancel: Cancellation) -> Result<Self, PipeIoError> {
        if cancel.is_cancelled() {
            drop(file);
            return Err(PipeIoError::Cancelled {
                worker_joined: true,
            });
        }
        require_synchronous_byte_pipe(&file)?;
        let (commands_tx, commands_rx) = mpsc::sync_channel(1);
        let (replies_tx, replies_rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            thread: Mutex::new(None),
            file: Mutex::new(Some(file)),
            handle_state: AtomicU8::new(0),
            started_io: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            #[cfg(test)]
            pause_before_io: Mutex::new(None),
        });
        let worker_shared = shared.clone();
        let worker_cancel = cancel.clone();
        let worker = thread::Builder::new()
            .name(match direction {
                Direction::Read => "harness-pipe-read".into(),
                Direction::Write => "harness-pipe-write".into(),
            })
            .spawn(move || {
                worker_main(
                    direction,
                    commands_rx,
                    replies_tx,
                    worker_shared,
                    worker_cancel,
                )
            })
            .map_err(|error| PipeIoError::Io(format!("pipe worker spawn failed: {error}")))?;
        Ok(Self {
            commands: Some(commands_tx),
            replies: Some(replies_rx),
            worker: Some(worker),
            cancel,
            shared,
            direction,
        })
    }

    pub fn io_started(&self) -> bool {
        self.shared.started_io.load(Ordering::Acquire)
    }

    pub fn read(
        &mut self,
        max: usize,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> Result<Vec<u8>, PipeIoError> {
        self.require_read()?;
        if max == 0 || max > READ_CHUNK {
            return Err(PipeIoError::Unsupported {
                reason: format!("read size {max} is outside 1..={READ_CHUNK}"),
            });
        }
        match self.submit(Command::Read { max }, deadline, cancellation)? {
            Reply::Bytes(bytes) => Ok(bytes),
            Reply::Written(_) => Err(PipeIoError::Io(
                "pipe reader returned a write result".into(),
            )),
            Reply::Failed(error) => Err(self.finish_stop(error, deadline)),
        }
    }

    pub fn write(
        &mut self,
        bytes: &[u8],
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> Result<usize, PipeIoError> {
        self.require_write()?;
        if bytes.is_empty() || bytes.len() > READ_CHUNK {
            return Err(PipeIoError::Unsupported {
                reason: format!("write size {} is outside 1..={READ_CHUNK}", bytes.len()),
            });
        }
        match self.submit(
            Command::Write {
                bytes: bytes.to_vec(),
            },
            deadline,
            cancellation,
        )? {
            Reply::Written(count) => Ok(count),
            Reply::Bytes(_) => Err(PipeIoError::Io("pipe writer returned a read result".into())),
            Reply::Failed(error) => Err(self.finish_stop(error, deadline)),
        }
    }

    pub fn write_all(
        &mut self,
        bytes: &[u8],
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> Result<(), PipeIoError> {
        self.require_write()?;
        if bytes.is_empty() {
            return Err(PipeIoError::Unsupported {
                reason: "write_all requires a non-empty buffer".into(),
            });
        }
        if bytes.len() > MAX_FRAME {
            return Err(PipeIoError::Unsupported {
                reason: format!("write_all size {} exceeds {MAX_FRAME}", bytes.len()),
            });
        }
        let mut offset = 0;
        while offset < bytes.len() {
            let end = (offset + READ_CHUNK).min(bytes.len());
            let wrote = self.write(&bytes[offset..end], deadline, cancellation)?;
            if wrote == 0 {
                return Err(PipeIoError::Io("WriteFile wrote zero bytes".into()));
            }
            offset += wrote;
        }
        Ok(())
    }

    fn require_read(&self) -> Result<(), PipeIoError> {
        if self.direction != Direction::Read {
            Err(PipeIoError::Io(
                "write endpoint does not support read".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn require_write(&self) -> Result<(), PipeIoError> {
        if self.direction != Direction::Write {
            Err(PipeIoError::Io(
                "read endpoint does not support write".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn submit(
        &mut self,
        command: Command,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> Result<Reply, PipeIoError> {
        if let Err(error) = check_stop(deadline, cancellation, &self.cancel) {
            return Err(self.finish_stop(error, deadline));
        }
        let commands = self
            .commands
            .as_ref()
            .ok_or_else(|| PipeIoError::Io("pipe worker commands closed".into()))?;
        if let Err(error) = send_bounded(commands, command, deadline, cancellation, &self.cancel) {
            return Err(self.finish_stop(error, deadline));
        }
        let replies = self
            .replies
            .as_ref()
            .ok_or_else(|| PipeIoError::Io("pipe worker replies closed".into()))?;
        loop {
            if let Err(error) = check_stop(deadline, cancellation, &self.cancel) {
                return Err(self.finish_stop(error, deadline));
            }
            match replies.recv_timeout(JOIN_POLL.min(deadline.remaining())) {
                Ok(reply) => return Ok(reply),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(self.finish_stop(
                        PipeIoError::Io("pipe worker disconnected before reply".into()),
                        deadline,
                    ));
                }
            }
        }
    }

    fn finish_stop(&mut self, error: PipeIoError, _deadline: Deadline) -> PipeIoError {
        request_stop(&self.shared);
        let cleanup =
            Deadline::after(Duration::from_secs(2)).expect("bounded pipe cleanup duration");
        match (error, self.join_worker(cleanup)) {
            (PipeIoError::Cancelled { .. }, Ok(())) => PipeIoError::Cancelled {
                worker_joined: true,
            },
            (PipeIoError::DeadlineExpired { .. }, Ok(())) => PipeIoError::DeadlineExpired {
                worker_joined: true,
            },
            (PipeIoError::Cancelled { .. }, Err(unresolved))
            | (PipeIoError::DeadlineExpired { .. }, Err(unresolved)) => unresolved,
            (other, Ok(())) => other,
            (_, Err(unresolved)) => unresolved,
        }
    }

    fn join_worker(&mut self, deadline: Deadline) -> Result<(), PipeIoError> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        drop(self.commands.take());
        drop(self.replies.take());
        wait_join(worker, &self.shared, deadline, "pipe I/O worker")
    }

    pub fn close(mut self, deadline: Deadline) -> Result<(), PipeIoError> {
        request_stop(&self.shared);
        self.join_worker(deadline)
    }
}

impl Drop for CancellablePipe {
    fn drop(&mut self) {
        request_stop(&self.shared);
        if let Some(worker) = self.worker.take() {
            let timeout = Deadline::after(Duration::from_secs(2))
                .map(|deadline| deadline.remaining())
                .unwrap_or(Duration::from_secs(2));
            if let Err(error) =
                wait_join_once(worker, &self.shared, timeout, "pipe I/O worker drop")
            {
                eprintln!("native pipe cleanup incomplete: {error}");
            }
        }
        // Only the worker closes Shared::file. A failed join must not close a
        // handle that may still be inside a Windows syscall or reuse its value.
    }
}

fn worker_main(
    direction: Direction,
    commands: mpsc::Receiver<Command>,
    replies: mpsc::Sender<Reply>,
    shared: Arc<Shared>,
    cancel: Cancellation,
) -> Result<(), PipeIoError> {
    match duplicate_current_thread() {
        Ok(handle) => {
            *shared.thread.lock().expect("pipe worker thread handle") = Some(handle);
            shared.handle_state.store(HANDLE_READY, Ordering::Release);
        }
        Err(error) => {
            shared.handle_state.store(HANDLE_FAILED, Ordering::Release);
            return Err(error);
        }
    }
    let result = run_worker(direction, commands, replies, &shared, &cancel);
    drop(shared.file.lock().ok().and_then(|mut guard| guard.take()));
    drop(shared.thread.lock().ok().and_then(|mut guard| guard.take()));
    result
}

fn run_worker(
    direction: Direction,
    commands: mpsc::Receiver<Command>,
    replies: mpsc::Sender<Reply>,
    shared: &Shared,
    cancel: &Cancellation,
) -> Result<(), PipeIoError> {
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if cancel.is_cancelled() && !shared.started_io.load(Ordering::Acquire) {
            return Err(PipeIoError::Cancelled {
                worker_joined: false,
            });
        }
        match commands.recv_timeout(JOIN_POLL) {
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => {}
            Ok(Command::Read { max }) => {
                if direction != Direction::Read {
                    send_reply(
                        &replies,
                        Reply::Failed(PipeIoError::Io(
                            "write endpoint received a read command".into(),
                        )),
                    );
                    continue;
                }
                if shared.stop.load(Ordering::Acquire) || cancel.is_cancelled() {
                    send_reply(
                        &replies,
                        Reply::Failed(PipeIoError::Cancelled {
                            worker_joined: false,
                        }),
                    );
                    continue;
                }
                shared.started_io.store(true, Ordering::Release);
                match read_once(shared, max) {
                    Ok(bytes) if bytes.is_empty() => {
                        send_reply(&replies, Reply::Failed(PipeIoError::EndOfFile))
                    }
                    Ok(bytes) => send_reply(&replies, Reply::Bytes(bytes)),
                    Err(error) => send_reply(&replies, Reply::Failed(error)),
                }
            }
            Ok(Command::Write { bytes }) => {
                if direction != Direction::Write {
                    send_reply(
                        &replies,
                        Reply::Failed(PipeIoError::Io(
                            "read endpoint received a write command".into(),
                        )),
                    );
                    continue;
                }
                if shared.stop.load(Ordering::Acquire) || cancel.is_cancelled() {
                    send_reply(
                        &replies,
                        Reply::Failed(PipeIoError::Cancelled {
                            worker_joined: false,
                        }),
                    );
                    continue;
                }
                shared.started_io.store(true, Ordering::Release);
                match write_once(shared, &bytes) {
                    Ok(count) => send_reply(&replies, Reply::Written(count)),
                    Err(error) => send_reply(&replies, Reply::Failed(error)),
                }
            }
        }
    }
}

fn send_reply(replies: &mpsc::Sender<Reply>, reply: Reply) {
    let _ = replies.send(reply);
}

fn read_once(shared: &Shared, max: usize) -> Result<Vec<u8>, PipeIoError> {
    let mut buffer = vec![0_u8; max.min(READ_CHUNK)];
    let handle = {
        let guard = shared.file.lock().expect("pipe file");
        let file = guard
            .as_ref()
            .ok_or_else(|| PipeIoError::Io("pipe endpoint already closed".into()))?;
        file.as_raw_handle()
    };
    #[cfg(test)]
    if let Some(pause) = shared.pause_before_io.lock().unwrap().clone() {
        pause.entered.store(true, Ordering::Release);
        while !pause.release.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
    }
    let mut read = 0_u32;
    let ok = unsafe {
        ReadFile(
            handle,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            &mut read,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(map_io_error("ReadFile"));
    }
    buffer.truncate(read as usize);
    Ok(buffer)
}

fn write_once(shared: &Shared, bytes: &[u8]) -> Result<usize, PipeIoError> {
    let handle = {
        let guard = shared.file.lock().expect("pipe file");
        let file = guard
            .as_ref()
            .ok_or_else(|| PipeIoError::Io("pipe endpoint already closed".into()))?;
        file.as_raw_handle()
    };
    let mut written = 0_u32;
    let ok = unsafe {
        WriteFile(
            handle,
            bytes.as_ptr(),
            bytes.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(map_io_error("WriteFile"));
    }
    Ok(written as usize)
}

fn map_io_error(op: &str) -> PipeIoError {
    let code = unsafe { GetLastError() };
    if code == ERROR_OPERATION_ABORTED {
        return PipeIoError::Cancelled {
            worker_joined: false,
        };
    }
    if op == "ReadFile" && (code == ERROR_BROKEN_PIPE || code == ERROR_NO_DATA) {
        return PipeIoError::EndOfFile;
    }
    let error = io::Error::from_raw_os_error(code as i32);
    PipeIoError::Io(format!("{op} failed: {error}"))
}

fn require_synchronous_byte_pipe(file: &File) -> Result<(), PipeIoError> {
    let handle = file.as_raw_handle();
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(PipeIoError::Unsupported {
            reason: "endpoint handle is null or INVALID_HANDLE_VALUE".into(),
        });
    }
    let file_type = unsafe { GetFileType(handle) };
    if file_type != FILE_TYPE_PIPE {
        return Err(PipeIoError::Unsupported {
            reason: format!("GetFileType returned {file_type}, expected FILE_TYPE_PIPE (3)"),
        });
    }
    let mut flags = 0_u32;
    let mut out_size = 0_u32;
    let mut in_size = 0_u32;
    let mut instances = 0_u32;
    if unsafe {
        GetNamedPipeInfo(
            handle,
            &mut flags,
            &mut out_size,
            &mut in_size,
            &mut instances,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        return Err(PipeIoError::Unsupported {
            reason: format!("GetNamedPipeInfo rejected the handle: {error}"),
        });
    }
    if flags & windows_sys::Win32::System::Pipes::PIPE_TYPE_MESSAGE != 0 {
        return Err(PipeIoError::Unsupported {
            reason: "message-mode pipe; synchronous byte pipe required".into(),
        });
    }
    // Null OVERLAPPED is valid only for synchronous handles. Keep query storage
    // on the heap so even an unexpected pending mode query cannot outlive it.
    let mut query = Box::<ModeQuery>::default();
    let status = unsafe {
        NtQueryInformationFile(
            handle,
            &mut query.status,
            (&mut query.mode as *mut u32).cast(),
            4,
            16,
        )
    };
    if status == 0x103 {
        let _ = Box::leak(query);
        return Err(PipeIoError::Unsupported {
            reason: "pipe mode query unexpectedly pending; private query storage retained".into(),
        });
    }
    if status != 0 || query.status.Information != 4 || query.mode & (0x10 | 0x20) == 0 {
        return Err(PipeIoError::Unsupported {
            reason: "overlapped or unknown pipe I/O mode; synchronous byte pipe required".into(),
        });
    }
    Ok(())
}

fn duplicate_current_thread() -> Result<OwnedHandle, PipeIoError> {
    // GetCurrentThread returns a pseudo-handle. CancelSynchronousIo needs a
    // real thread HANDLE with THREAD_TERMINATE (Microsoft Learn, ioapiset.h).
    // std::thread::JoinHandle's Windows handle is the CreateThread handle; we
    // still duplicate from inside the worker so the handle exists before the
    // first blocking I/O, including the cancel-before-enter race.
    let mut handle = std::ptr::null_mut();
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            GetCurrentThread(),
            GetCurrentProcess(),
            &mut handle,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 || handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(PipeIoError::Io(format!(
            "DuplicateHandle(GetCurrentThread) failed: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn request_stop(shared: &Shared) {
    shared.stop.store(true, Ordering::Release);
    interrupt_worker(shared);
}

fn interrupt_worker(shared: &Shared) {
    if let Ok(guard) = shared.thread.lock()
        && let Some(handle) = guard.as_ref()
    {
        // Cancellation is advisory; only the subsequent bounded join proves
        // completion, including a request racing just before the syscall.
        let _ = unsafe { CancelSynchronousIo(handle.as_raw_handle()) };
    }
    if let Ok(guard) = shared.file.lock()
        && let Some(file) = guard.as_ref()
    {
        let _ = unsafe { CancelIoEx(file.as_raw_handle(), std::ptr::null()) };
    }
}

fn send_bounded(
    commands: &SyncSender<Command>,
    command: Command,
    deadline: Deadline,
    caller: &Cancellation,
    stored: &Cancellation,
) -> Result<(), PipeIoError> {
    loop {
        check_stop(deadline, caller, stored)?;
        match commands.try_send(clone_command(&command)) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(_)) => {
                std::thread::sleep(JOIN_POLL.min(deadline.remaining()));
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(PipeIoError::Io("pipe worker command channel closed".into()));
            }
        }
    }
}

fn clone_command(command: &Command) -> Command {
    match command {
        Command::Read { max } => Command::Read { max: *max },
        Command::Write { bytes } => Command::Write {
            bytes: bytes.clone(),
        },
    }
}

fn check_stop(
    deadline: Deadline,
    caller: &Cancellation,
    stored: &Cancellation,
) -> Result<(), PipeIoError> {
    if caller.is_cancelled() || stored.is_cancelled() {
        return Err(PipeIoError::Cancelled {
            worker_joined: false,
        });
    }
    if deadline.expired() {
        return Err(PipeIoError::DeadlineExpired {
            worker_joined: false,
        });
    }
    Ok(())
}

fn wait_join(
    worker: JoinHandle<Result<(), PipeIoError>>,
    shared: &Shared,
    deadline: Deadline,
    what: &str,
) -> Result<(), PipeIoError> {
    wait_join_once(worker, shared, deadline.remaining().max(JOIN_POLL), what)
}

fn wait_join_once(
    worker: JoinHandle<Result<(), PipeIoError>>,
    shared: &Shared,
    timeout: Duration,
    what: &str,
) -> Result<(), PipeIoError> {
    // JoinHandle implements AsHandle/AsRawHandle on Windows with the real
    // CreateThread HANDLE (std 1.63+ io_safety). WaitForSingleObject on that
    // handle is the same wait std uses for join, without an unbounded join().
    let handle = worker.as_handle().as_raw_handle();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        interrupt_worker(shared);
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() && unsafe { WaitForSingleObject(handle, 0) } != WAIT_OBJECT_0 {
            drop(worker);
            return Err(PipeIoError::UnresolvedOwnership {
                reason: format!(
                    "{what} still running after {} ms; cancellation retried; detached worker retains its endpoint until it exits; peer handles were not closed or killed",
                    timeout.as_millis()
                ),
            });
        }
        let millis = timeout_millis(remaining.min(JOIN_POLL));
        let waited = unsafe { WaitForSingleObject(handle, millis) };
        if waited == WAIT_OBJECT_0 {
            return match worker.join() {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => match error {
                    PipeIoError::Cancelled { .. } | PipeIoError::DeadlineExpired { .. } => Ok(()),
                    other => Err(other),
                },
                Err(_) => Err(PipeIoError::Io(format!("{what} panicked"))),
            };
        }
        if waited != WAIT_TIMEOUT {
            return Err(PipeIoError::Io(format!(
                "WaitForSingleObject({what}) failed: {}",
                io::Error::last_os_error()
            )));
        }
    }
}

fn timeout_millis(timeout: Duration) -> u32 {
    let millis = timeout.as_millis();
    if millis == 0 {
        1
    } else if millis >= u128::from(u32::MAX) {
        INFINITE - 1
    } else {
        millis as u32
    }
}

/// Real Windows anonymous pipe pair. Both ends are non-inheritable.
pub fn anonymous_pipe(buffer: u32) -> Result<(File, File), PipeIoError> {
    use windows_sys::Win32::System::Pipes::CreatePipe;
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), buffer) } == 0 {
        return Err(PipeIoError::Io(format!(
            "CreatePipe failed: {}",
            io::Error::last_os_error()
        )));
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
        return Err(PipeIoError::Io(
            "CreatePipe returned an invalid handle".into(),
        ));
    }
    Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_cleanup_keeps_endpoint_alive_until_worker_exits() {
        let (read, peer) = anonymous_pipe(PIPE_BUFFER).unwrap();
        let mut reader = CancellablePipe::reader(read, Cancellation::default()).unwrap();
        let shared = reader.shared.clone();
        let pause = Arc::new(TestPause::default());
        *shared.pause_before_io.lock().unwrap() = Some(pause.clone());
        let owner = thread::spawn(move || {
            let error = reader
                .read(
                    8,
                    Deadline::after(Duration::from_millis(200)).unwrap(),
                    &Cancellation::default(),
                )
                .unwrap_err();
            drop(reader);
            error
        });
        let limit = std::time::Instant::now() + Duration::from_secs(3);
        while !pause.entered.load(Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < limit,
                "worker did not reach controlled I/O boundary"
            );
            thread::sleep(Duration::from_millis(1));
        }
        let worker_handle = shared
            .thread
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .as_handle()
            .try_clone_to_owned()
            .unwrap();
        let error = owner.join().unwrap();
        let endpoint_retained = shared.file.lock().unwrap().is_some();
        // Release/close only our owned fixture endpoints even on the bad baseline.
        pause.release.store(true, Ordering::Release);
        drop(peer);
        let stopped =
            unsafe { WaitForSingleObject(worker_handle.as_raw_handle(), 3_000) } == WAIT_OBJECT_0;
        assert!(stopped, "controlled worker survived fixture cleanup");
        assert!(
            matches!(error, PipeIoError::UnresolvedOwnership { .. }),
            "{error}"
        );
        assert!(
            endpoint_retained,
            "Drop closed a File still used by an unresolved I/O worker"
        );
    }
}
