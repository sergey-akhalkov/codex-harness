//! Native cancellable anonymous-pipe I/O. Peer handles stay in the test.
//! cargo test --locked -p harness-core --test cancellable_pipe -- --test-threads=1 --nocapture
#![cfg(windows)]

use harness_core::cancellable_pipe::{
    CancellablePipe, PIPE_BUFFER, PipeIoError, READ_CHUNK, anonymous_pipe,
};
use harness_core::process::{Cancellation, Deadline};
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_DELETE_ON_CLOSE, FILE_TYPE_PIPE, GetFileType,
};

fn deadline(ms: u64) -> Deadline {
    Deadline::after(Duration::from_millis(ms)).unwrap()
}

fn assert_cancelled(error: PipeIoError, started: bool) {
    match error {
        PipeIoError::Cancelled { worker_joined } => assert!(worker_joined, "cancel without join"),
        PipeIoError::UnresolvedOwnership { reason } => {
            panic!("unresolved ownership (started={started}): {reason}")
        }
        other => panic!("expected cancelled, got {other}"),
    }
}

fn assert_deadline(error: PipeIoError) {
    match error {
        PipeIoError::DeadlineExpired { worker_joined } => {
            assert!(worker_joined, "deadline without join")
        }
        PipeIoError::UnresolvedOwnership { reason } => {
            panic!("unresolved ownership on deadline: {reason}")
        }
        other => panic!("expected deadline, got {other}"),
    }
}

#[test]
fn ordinary_file_is_rejected_with_reason() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ordinary");
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
        .open(&path)
        .unwrap();
    match CancellablePipe::reader(file, Cancellation::default()) {
        Err(PipeIoError::Unsupported { reason }) => {
            assert!(reason.contains("FILE_TYPE_PIPE"), "{reason}");
        }
        Err(other) => panic!("expected unsupported, got {other}"),
        Ok(_) => panic!("ordinary file was admitted"),
    }
}

#[test]
fn cancellation_before_start_executes_no_io() {
    let (read, write) = anonymous_pipe(PIPE_BUFFER).unwrap();
    let cancel = Cancellation::default();
    cancel.cancel();
    match CancellablePipe::reader(read, cancel.clone()) {
        Err(PipeIoError::Cancelled { worker_joined }) => assert!(worker_joined),
        Err(other) => panic!("expected cancelled before spawn, got {other}"),
        Ok(_) => panic!("pre-cancelled endpoint was admitted"),
    }
    drop(write);
}

#[test]
fn eof_after_peer_writer_close() {
    let (read, write) = anonymous_pipe(PIPE_BUFFER).unwrap();
    let cancel = Cancellation::default();
    let mut reader = CancellablePipe::reader(read, cancel.clone()).unwrap();
    drop(write);
    match reader.read(16, deadline(5_000), &cancel) {
        Err(PipeIoError::EndOfFile) => {}
        Err(other) => panic!("expected EOF, got {other}"),
        Ok(bytes) => panic!("expected EOF, got {} bytes", bytes.len()),
    }
    reader.close(deadline(5_000)).unwrap();
}

#[test]
fn unicode_multibyte_roundtrip() {
    let (read, write) = anonymous_pipe(PIPE_BUFFER).unwrap();
    let cancel = Cancellation::default();
    let mut reader = CancellablePipe::reader(read, cancel.clone()).unwrap();
    let mut writer = CancellablePipe::writer(write, cancel.clone()).unwrap();
    let payload = "привет\n".as_bytes();
    writer.write_all(payload, deadline(5_000), &cancel).unwrap();
    let frame = reader.read(1024, deadline(5_000), &cancel).unwrap();
    assert_eq!(frame, payload);
    writer.close(deadline(5_000)).unwrap();
    reader.close(deadline(5_000)).unwrap();
}

#[test]
fn blocked_read_cancels_while_peer_writer_stays_open() {
    let (read, write) = anonymous_pipe(PIPE_BUFFER).unwrap();
    let cancel = Cancellation::default();
    let mut reader = CancellablePipe::reader(read, cancel.clone()).unwrap();
    let cancel_flag = cancel.clone();
    let helper = thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        cancel_flag.cancel();
    });
    let begin = Instant::now();
    let error = reader.read(8, deadline(8_000), &cancel).unwrap_err();
    helper.join().unwrap();
    let elapsed = begin.elapsed();
    assert!(
        elapsed < Duration::from_secs(4),
        "blocked read took {elapsed:?}"
    );
    assert_cancelled(error, reader.io_started());
    assert!(reader.io_started(), "fixture cancelled before entering I/O");
    // Closing the local reader legitimately breaks writes. Peer ownership is
    // proven by its still-valid retained handle, not by successful delivery.
    assert_eq!(
        unsafe { GetFileType(write.as_raw_handle()) },
        FILE_TYPE_PIPE
    );
}

#[test]
fn blocked_write_cancels_while_peer_does_not_drain() {
    let (read, write) = anonymous_pipe(64).unwrap();
    let cancel = Cancellation::default();
    let mut writer = CancellablePipe::writer(write, cancel.clone()).unwrap();
    let flood = vec![0x61; 96 * 1024];
    let started = Instant::now();
    let helper_cancel = cancel.clone();
    let helper = thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        helper_cancel.cancel();
    });
    let error = writer
        .write_all(&flood, deadline(8_000), &cancel)
        .unwrap_err();
    helper.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_cancelled(error, writer.io_started());
    assert!(writer.io_started(), "fixture cancelled before entering I/O");
    drop(read);
}

#[test]
fn input_flood_is_bounded_by_chunk_and_deadline() {
    let (read, write) = anonymous_pipe(PIPE_BUFFER).unwrap();
    let cancel = Cancellation::default();
    let mut writer = CancellablePipe::writer(write, cancel.clone()).unwrap();
    let chunk = vec![0x62; READ_CHUNK];
    writer.write_all(&chunk, deadline(5_000), &cancel).unwrap();
    let mut reader = CancellablePipe::reader(read, cancel.clone()).unwrap();
    let mut got = 0usize;
    while got < READ_CHUNK {
        let bytes = reader.read(READ_CHUNK, deadline(5_000), &cancel).unwrap();
        assert!(!bytes.is_empty());
        got += bytes.len();
    }
    assert_eq!(got, READ_CHUNK);
    writer.close(deadline(5_000)).unwrap();
    reader.close(deadline(5_000)).unwrap();
}

#[test]
fn in_flight_deadline_does_not_kill_peer() {
    let (read, write) = anonymous_pipe(PIPE_BUFFER).unwrap();
    let cancel = Cancellation::default();
    let mut reader = CancellablePipe::reader(read, cancel.clone()).unwrap();
    let error = reader.read(8, deadline(80), &cancel).unwrap_err();
    assert_deadline(error);
    assert_eq!(
        unsafe { GetFileType(write.as_raw_handle()) },
        FILE_TYPE_PIPE
    );
    assert!(!cancel.is_cancelled(), "deadline changed the caller token");
}

#[test]
fn repeated_operation_and_drop_paths() {
    for _ in 0..3 {
        let (read, write) = anonymous_pipe(PIPE_BUFFER).unwrap();
        let cancel = Cancellation::default();
        let mut writer = CancellablePipe::writer(write, cancel.clone()).unwrap();
        let mut reader = CancellablePipe::reader(read, cancel.clone()).unwrap();
        writer
            .write_all(b"abc\n", deadline(5_000), &cancel)
            .unwrap();
        let frame = reader.read(16, deadline(5_000), &cancel).unwrap();
        assert_eq!(frame, b"abc\n");
        drop(writer);
        drop(reader);
    }
}

#[test]
fn overlapped_and_message_handles_are_rejected_before_io() {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
        },
        System::Pipes::{
            CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_TYPE_BYTE, PIPE_TYPE_MESSAGE,
        },
    };
    for (label, access, mode) in [
        ("overlapped", FILE_FLAG_OVERLAPPED, PIPE_TYPE_BYTE),
        ("message", 0, PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE),
    ] {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name: Vec<u16> = format!(
            r"\\.\pipe\harness-pipe-reject-{}-{unique}",
            std::process::id()
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | access,
                mode,
                1,
                4096,
                4096,
                0,
                std::ptr::null(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        let file = unsafe { std::fs::File::from_raw_handle(handle) };
        match CancellablePipe::reader(file, Cancellation::default()) {
            Err(PipeIoError::Unsupported { reason }) => assert!(
                reason.contains("synchronous byte pipe"),
                "{label}: {reason}"
            ),
            Err(error) => panic!("{label}: wrong failure: {error}"),
            Ok(_) => panic!("{label}: unsafe handle was admitted"),
        }
    }
}
